/// Feedback Loop — Async Controller
///
/// Spawns a Tokio background task that:
///   1. Classifies the dominant bottleneck every 1 s (adaptive interval).
///   2. Derives an ActionPlan and applies it to `AdaptiveState::current_config`.
///   3. Measures Δthroughput after the next interval.
///   4. Reverts the plan if throughput regressed by > 2%.
///   5. Delegates to the AutoTuner every 3rd cycle for empirical hill-climbing.

use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time;

use super::{AdaptiveState, Bottleneck};
use super::classifier::Classifier;
use super::decision_engine::DecisionEngine;
use super::auto_tuner::AutoTuner;

// ── Public API ────────────────────────────────────────────────────────────────

pub struct FeedbackLoop {
    handle: JoinHandle<()>,
}

impl FeedbackLoop {
    /// Start the feedback loop as an independent Tokio task.
    pub fn start(state: Arc<AdaptiveState>) -> Self {
        let handle = tokio::spawn(run_loop(state));
        FeedbackLoop { handle }
    }

    /// Signal the loop to stop and await its exit.
    pub async fn stop(self) {
        self.handle.abort();
        let _ = self.handle.await;
    }
}

// ── Core loop ─────────────────────────────────────────────────────────────────

async fn run_loop(state: Arc<AdaptiveState>) {
    let classifier     = Classifier::new();
    let decision_engine= DecisionEngine::new();
    let mut tuner      = AutoTuner::new();

    let mut interval = Duration::from_secs(1);
    let mut cycle: u32 = 0;
    let mut prev_tp: f64 = 0.0;
    let mut prev_config = state.config_snapshot();

    while state.running.load(std::sync::atomic::Ordering::Relaxed) {
        time::sleep(interval).await;

        let samples = state.snapshot().await;
        if samples.len() < 3 {
            continue;
        }

        // ── Current throughput ────────────────────────────────────────────────
        let current_tp = samples.iter()
            .filter_map(|s| s.get("rolling_throughput_mbps").and_then(|v| v.as_f64()))
            .rev().take(3).sum::<f64>() / 3.0_f64.min(samples.len() as f64);

        // ── Phase 1: Classification ───────────────────────────────────────────
        let report = classifier.classify(&samples);

        {
            let mut history = state.bottleneck_history.lock().await;
            history.push(report.clone());
        }

        let current_config = state.config_snapshot();

        let mut plan = decision_engine.plan(&report, &current_config);
        
        let zero_copy_failed = state.zero_copy_failed.load(std::sync::atomic::Ordering::Relaxed);
        let udp_handshake_failed = state.udp_handshake_failed.load(std::sync::atomic::Ordering::Relaxed);
        let latest_sample = samples.last();
        let rtt_ms = latest_sample
            .and_then(|s| s.get("rtt_ms").and_then(|v| v.as_f64()))
            .unwrap_or(20.0);
        let packet_loss_pct = {
            if let Ok(lock) = state.packet_loss_pct.try_lock() {
                *lock
            } else {
                0.0
            }
        };

        let (mut target_mode, _reason) = crate::transport_switcher::TransportSwitcher::select_transport(
            &report,
            state.file_size,
            zero_copy_failed,
            udp_handshake_failed,
            rtt_ms,
            packet_loss_pct,
        );

        if let Ok(lock) = super::OVERRIDE_TRANSPORT_MODE.lock() {
            if let Some(mode) = *lock {
                target_mode = mode;
            }
        }

        let mut has_action = !matches!(plan.actions.first(), Some(super::decision_engine::OptimizationAction::NoOp));
        if target_mode != current_config.transport_mode {
            has_action = true;
            match target_mode {
                crate::transport::TransportMode::TcpZeroCopy => {
                    plan.actions.push(super::decision_engine::OptimizationAction::EnableZeroCopy);
                }
                crate::transport::TransportMode::Quic => {
                    plan.actions.push(super::decision_engine::OptimizationAction::SwitchToQuic);
                }
                crate::transport::TransportMode::UdpCustom => {
                    plan.actions.push(super::decision_engine::OptimizationAction::SwitchToUdpCustom);
                }
                _ => {}
            }
        }

        if has_action {
            let mut new_config = plan.apply_all(current_config.clone());
            new_config.transport_mode = target_mode; // Ensure switcher override takes priority

            // Apply
            {
                let mut cfg = state.current_config.write().await;
                *cfg = new_config.clone();
            }

            // Wait one more interval, then measure impact
            time::sleep(interval).await;
            let post_samples = state.snapshot().await;
            let post_tp = post_samples.iter()
                .filter_map(|s| s.get("rolling_throughput_mbps").and_then(|v| v.as_f64()))
                .rev().take(3).sum::<f64>() / 3.0_f64.min(post_samples.len() as f64);

            let regressed = post_tp < prev_tp * 0.98 && prev_tp > 0.0;

            if regressed {
                // Revert
                let mut cfg = state.current_config.write().await;
                *cfg = prev_config.clone();
                eprintln!(
                    "[adaptive] Reverted: throughput dropped {:.1} → {:.1} Mbps",
                    prev_tp, post_tp
                );
            } else {
                // Commit
                prev_config = new_config;
                eprintln!(
                    "[adaptive] Applied ({}): {:.1} → {:.1} Mbps",
                    report.bottleneck.label(), prev_tp, post_tp
                );
            }

            // Record in history
            {
                let mut history = state.action_history.lock().await;
                history.push((plan, prev_tp, post_tp));
            }

            prev_tp = post_tp;
        }

        // ── Phase 3: Auto-tuner (every 3rd cycle) ────────────────────────────
        if cycle % 3 == 2 {
            let snap_cfg = state.config_snapshot();
            if let Some(candidate) = tuner.step(&snap_cfg, current_tp, &samples) {
                {
                    let mut cfg = state.current_config.write().await;
                    *cfg = candidate;
                }
            }
            // If tuner rejected the last probe, it has already reset internally.
        }

        // ── Adaptive interval ─────────────────────────────────────────────────
        // Speed up if a significant bottleneck is detected; slow down when healthy.
        interval = if matches!(report.bottleneck, Bottleneck::Healthy) && report.confidence < 0.2 {
            Duration::from_secs(2)
        } else {
            Duration::from_secs(1)
        };

        if prev_tp == 0.0 {
            prev_tp = current_tp;
        }

        cycle = cycle.wrapping_add(1);
    }
}
