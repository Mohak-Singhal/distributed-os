use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Mutex as TokioMutex};

use super::metrics::{MetricsHistory, NetworkMetrics};
use super::classifier::Classifier;
use super::decision::DecisionEngine;
use crate::adaptive::TuningConfig;
use crate::transport::TransportMode;

/// The closed-loop controller.
///
/// Runs a background task every ~100ms that:
/// 1. Snapshot metrics from the shared `MetricsHistory`
/// 2. Classify the current bottleneck
/// 3. Decide on control actions
/// 4. Apply them to the shared `TuningConfig`
///
/// The streaming loop reads from `TuningConfig` every chunk,
/// so changes take effect immediately.
pub struct ControlLoop {
    /// Shared metrics history (writer = streaming task, reader = loop)
    pub metrics: Arc<TokioMutex<MetricsHistory>>,
    /// Shared tuning config (writer = loop, reader = streaming task)
    pub config: Arc<RwLock<TuningConfig>>,
    /// Classifier instance
    classifier: Arc<TokioMutex<Classifier>>,
    /// Decision engine
    decision_engine: Arc<TokioMutex<DecisionEngine>>,
    /// Whether the loop is running
    running: Arc<std::sync::atomic::AtomicBool>,
    /// Timestamp of the last decision (for cooldown gating)
    last_decision: Arc<TokioMutex<Instant>>,
    /// Cooldown between decisions (prevents oscillation)
    cooldown: Duration,
    /// Active transport mode (None = TCP default)
    transport_mode: Arc<std::sync::Mutex<Option<TransportMode>>>,
}

impl ControlLoop {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(TokioMutex::new(MetricsHistory::new(30))),
            config: Arc::new(RwLock::new(TuningConfig::default())),
            classifier: Arc::new(TokioMutex::new(Classifier::new())),
            decision_engine: Arc::new(TokioMutex::new(DecisionEngine::new())),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            last_decision: Arc::new(TokioMutex::new(
                Instant::now() - Duration::from_millis(500),
            )),
            cooldown: Duration::from_millis(500),
            transport_mode: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Spawn the background control loop.
    ///
    /// The loop runs every 100ms and:
    /// - Snapshots current metrics
    /// - Classifies the bottleneck
    /// - Decides on adjustments
    /// - Applies changes to the shared config
    pub fn spawn(&self) {
        self.running.store(true, std::sync::atomic::Ordering::SeqCst);
        let metrics = self.metrics.clone();
        let config = self.config.clone();
        let classifier = self.classifier.clone();
        let decision_engine = self.decision_engine.clone();
        let running = self.running.clone();
        let last_decision = self.last_decision.clone();
        let cooldown = self.cooldown;
        let transport_mode = self.transport_mode.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut tick_count: u64 = 0;
            let mut last_decision_str = String::from("none");

            while running.load(std::sync::atomic::Ordering::Relaxed) {
                interval.tick().await;
                tick_count += 1;

                // 1. Snapshot metrics
                let latest_metrics = {
                    let mut m = metrics.lock().await;
                    if m.len() == 0 {
                        continue;
                    }
                    m.snapshot()
                };

                // 2. Get recent history for classification
                let recent = {
                    let m = metrics.lock().await;
                    m.last_n(5)
                };

                if recent.is_empty() {
                    continue;
                }

                // 3. Classify bottleneck
                let classification = {
                    let mut c = classifier.lock().await;
                    c.classify(&recent)
                };

                // 4. Get current config snapshot
                let current_config = {
                    let cfg = config.read().await;
                    cfg.clone()
                };

                // 4b. Read active transport mode
                let active_transport = *transport_mode.lock().unwrap();

                // 5. Cooldown gate — prevent oscillation
                {
                    let since_last = last_decision.lock().await.elapsed();
                    if since_last < cooldown {
                        // Print observability log every ~500ms even during cooldown
                        if tick_count % 5 == 0 {
                            println!(
                                "[ctrl] rate={:.1}MB/s rtt={:.1}ms loss={:.1}% queue={:.1}ms chunk={}KB parallel={} bottleneck={:?} decision={}",
                                latest_metrics.throughput_mbps / 8.0,
                                latest_metrics.rtt_ms,
                                latest_metrics.packet_loss_pct,
                                latest_metrics.queue_delay_ms,
                                current_config.chunk_size_bytes / 1024,
                                current_config.parallel_streams,
                                classification.bottleneck,
                                last_decision_str,
                            );
                        }
                        continue;
                    }
                }

                // 6. Decide (transport-aware)
                let decision = {
                    let mut de = decision_engine.lock().await;
                    de.decide_for_transport(&classification, &current_config, active_transport)
                };

                if let Some(d) = decision {
                    // 7. Apply
                    {
                        let mut cfg = config.write().await;
                        decision_engine.lock().await.apply(&d, &mut *cfg);
                        *last_decision.lock().await = Instant::now();
                    }

                    last_decision_str = format!("{} — {}", format!("{:?}", classification.bottleneck), d.reason);

                    // Log the decision for observability
                    tracing::info!(
                        "[control] {} — {}",
                        format!("{:?}", classification.bottleneck),
                        d.reason,
                    );
                }

                // Print observability log every ~500ms (5 ticks)
                if tick_count % 5 == 0 {
                    println!(
                        "[ctrl] rate={:.1}MB/s rtt={:.1}ms loss={:.1}% queue={:.1}ms chunk={}KB parallel={} bottleneck={:?} decision={}",
                        latest_metrics.throughput_mbps / 8.0,
                        latest_metrics.rtt_ms,
                        latest_metrics.packet_loss_pct,
                        latest_metrics.queue_delay_ms,
                        current_config.chunk_size_bytes / 1024,
                        current_config.parallel_streams,
                        classification.bottleneck,
                        last_decision_str,
                    );
                }
            }
        });
    }

    /// Stop the control loop.
    pub fn stop(&self) {
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Record a write event into the shared metrics.
    pub async fn record_write(&self, bytes: u64, duration_us: u64, is_retransmit: bool) {
        let mut metrics = self.metrics.lock().await;
        metrics.record_write(bytes, duration_us, is_retransmit);
    }

    /// Record an RTT measurement.
    pub async fn record_rtt(&self, rtt_ms: f64) {
        let mut metrics = self.metrics.lock().await;
        metrics.record_rtt(rtt_ms);
    }

    /// Get a snapshot of the latest metrics (for status reporting).
    pub async fn latest_metrics(&self) -> Option<NetworkMetrics> {
        let metrics = self.metrics.lock().await;
        metrics.last_n(1).into_iter().next()
    }

    /// Get current config from the shared state.
    pub async fn current_config(&self) -> TuningConfig {
        self.config.read().await.clone()
    }

    /// Set the active transport mode (changes decision behavior).
    pub fn set_transport(&mut self, mode: TransportMode) {
        *self.transport_mode.lock().unwrap() = Some(mode);
    }

    /// Reset loop state for a resume transfer.
    ///
    /// Clears metrics history and resets pacing so the resumed transfer
    /// isn't penalized for pre-resume measurements.
    pub async fn reset_for_resume(&self) {
        let mut metrics = self.metrics.lock().await;
        metrics.clear();
        // Reset config to defaults
        let mut cfg = self.config.write().await;
        *cfg = crate::adaptive::TuningConfig::default();
    }
}
