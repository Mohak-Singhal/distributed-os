/// Adaptive Optimization & Auto-Tuning Engine
///
/// Transforms the telemetry pipeline from:
///   Collect → Analyze → Report
/// into:
///   Collect → Analyze → Decide → Act → Re-measure → Optimize
///
/// # Modules
/// - `classifier`     — Weighted bottleneck classification with confidence score
/// - `decision_engine`— Rule-based mapping of bottleneck → concrete action plan
/// - `auto_tuner`     — Hill-climbing parameter optimizer
/// - `feedback_loop`  — Async controller tying all layers together

pub mod classifier;
pub mod decision_engine;
pub mod auto_tuner;
pub mod feedback_loop;

use std::collections::VecDeque;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tokio::sync::{Mutex, RwLock};

// ── Re-exports ───────────────────────────────────────────────────────────────

pub use classifier::{Bottleneck, BottleneckReport, Classifier};
pub use decision_engine::{ActionPlan, OptimizationAction, DecisionEngine};
pub use auto_tuner::{TuningConfig, AutoTuner};
pub use feedback_loop::FeedbackLoop;
use crate::transport::TransportMode;

// ── TuningConfig defaults ────────────────────────────────────────────────────

impl Default for TuningConfig {
    fn default() -> Self {
        TuningConfig {
            chunk_size_bytes: 1_048_576,    // 1 MB
            parallel_streams: 1,
            send_buffer_kb: 4096,
            recv_buffer_kb: 4096,
            write_batch_size: 4,
            worker_threads: num_cpus::get().max(2),
            throughput_limit_mbps: None,    // unlimited
            transport_mode: TransportMode::TcpBuffered,
        }
    }
}

// ── Shared State ─────────────────────────────────────────────────────────────

/// Thread-safe state shared between `BenchmarkSession` and `FeedbackLoop`.
pub struct AdaptiveState {
    /// Rolling ring of the last 30 telemetry samples (100 ms each → 3 s window).
    pub samples: Arc<Mutex<VecDeque<serde_json::Value>>>,
    /// Active tuning configuration, updated in-place by the feedback loop.
    pub current_config: Arc<RwLock<TuningConfig>>,
    /// History of every bottleneck classification produced during the transfer.
    pub bottleneck_history: Arc<Mutex<Vec<BottleneckReport>>>,
    /// (plan, throughput_before_mbps, throughput_after_mbps)
    pub action_history: Arc<Mutex<Vec<(ActionPlan, f64, f64)>>>,
    /// Set to `false` to signal the feedback loop to exit cleanly.
    pub running: Arc<AtomicBool>,
    /// Size of the file currently being transferred.
    pub file_size: u64,
    /// Shared atomic flag to mark if zero-copy syscalls failed (forcing fallback).
    pub zero_copy_failed: Arc<AtomicBool>,
    /// Shared atomic flag to mark if UDP handshake failed.
    pub udp_handshake_failed: Arc<AtomicBool>,
    /// Latest packet loss percentage tracked during user-space transfer.
    pub packet_loss_pct: Arc<Mutex<f64>>,
    /// Latest RTT variance tracked during user-space transfer.
    pub rtt_variance_ms: Arc<Mutex<f64>>,
}

impl AdaptiveState {
    pub fn new(file_size: u64) -> Arc<Self> {
        let mut initial_cfg = TuningConfig::default();
        if let Ok(lock) = OVERRIDE_TRANSPORT_MODE.lock() {
            if let Some(mode) = *lock {
                initial_cfg.transport_mode = mode;
            }
        }
        Arc::new(Self {
            samples: Arc::new(Mutex::new(VecDeque::with_capacity(30))),
            current_config: Arc::new(RwLock::new(initial_cfg)),
            bottleneck_history: Arc::new(Mutex::new(Vec::new())),
            action_history: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(true)),
            file_size,
            zero_copy_failed: Arc::new(AtomicBool::new(false)),
            udp_handshake_failed: Arc::new(AtomicBool::new(false)),
            packet_loss_pct: Arc::new(Mutex::new(0.0)),
            rtt_variance_ms: Arc::new(Mutex::new(0.0)),
        })
    }

    /// Push a new telemetry sample into the ring, evicting the oldest if full.
    pub async fn push_sample(&self, sample: serde_json::Value) {
        let mut q = self.samples.lock().await;
        if q.len() >= 30 {
            q.pop_front();
        }
        q.push_back(sample);
    }

    /// Snapshot the current ring as a `Vec` (cheap clone).
    pub async fn snapshot(&self) -> Vec<serde_json::Value> {
        self.samples.lock().await.iter().cloned().collect()
    }

    /// Read the current tuning config without blocking (falls back to default).
    pub fn config_snapshot(&self) -> TuningConfig {
        self.current_config
            .try_read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Stop the feedback loop.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

lazy_static::lazy_static! {
    /// Hot-read path: called on every chunk iteration.  RwLock allows concurrent readers
    /// without contention, eliminating the ~1.9% futex overhead from stress-test profiling.
    pub static ref ACTIVE_ADAPTIVE_STATE: std::sync::RwLock<Option<Arc<AdaptiveState>>> = std::sync::RwLock::new(None);
    pub static ref OVERRIDE_TRANSPORT_MODE: std::sync::Mutex<Option<TransportMode>> = std::sync::Mutex::new(None);
}

/// Register the active adaptive state for the current transfer session.
pub fn register_active_state(state: Arc<AdaptiveState>) {
    if let Ok(mut lock) = ACTIVE_ADAPTIVE_STATE.write() {
        *lock = Some(state);
    }
}

/// Deregister the active adaptive state.
pub fn deregister_active_state() {
    if let Ok(mut lock) = ACTIVE_ADAPTIVE_STATE.write() {
        *lock = None;
    }
}

/// Retrieve the active tuning configuration safely (falls back to default).
pub fn get_active_config() -> TuningConfig {
    if let Ok(lock) = ACTIVE_ADAPTIVE_STATE.read() {
        if let Some(ref state) = *lock {
            return state.config_snapshot();
        }
    }
    TuningConfig::default()
}
