use super::classifier::{Bottleneck, ClassificationResult};
use crate::adaptive::TuningConfig;
use crate::transport::TransportMode;

/// A set of concrete parameter changes produced by the decision engine.
#[derive(Debug, Clone)]
pub struct ControlDecision {
    /// New congestion window (bytes) — None = no change
    pub new_cwnd: Option<u64>,
    /// New chunk size (bytes) — None = no change
    pub new_chunk_size: Option<usize>,
    /// New parallelism level — None = no change
    pub parallelism: Option<usize>,
    /// New pacing rate (Mbps) — None = no change
    pub pacing_rate: Option<f64>,
    /// New send buffer (KB) — None = no change
    pub send_buffer_kb: Option<usize>,
    /// Human-readable reason for this decision
    pub reason: String,
}

// ── Clamping bounds ────────────────────────────────────────────────────────

const MIN_CHUNK_BYTES: usize = 16 * 1024;       // 16 KB
const MAX_CHUNK_BYTES: usize = 2 * 1024 * 1024;  // 2 MB
const MIN_PARALLELISM: usize = 1;
const MAX_PARALLELISM: usize = 8;
const MIN_RATE_MBPS: f64 = 10.0;
const MAX_RATE_MBPS: f64 = 2000.0;

// ── Step limiting ──────────────────────────────────────────────────────────

/// Clamp `new` to at most `max_delta` away from `old`.
fn limit_step(old: f64, new: f64, max_delta: f64) -> f64 {
    if new > old {
        (old + max_delta).min(new)
    } else {
        (old - max_delta).max(new)
    }
}

/// Maps bottleneck classifications to concrete control actions.
///
/// Design principles:
/// - Cooldown: no more than one adjustment per 500ms to avoid oscillation
/// - Smoothing: changes are applied gradually via `limit_step` (max 15% / cycle)
/// - Clamping: every output is clamped to safe bounds
/// - Conservative: when unsure, reduce speed cautiously
pub struct DecisionEngine {
    /// Cooldown: prevent rapid oscillation
    pub cooldown_ms: u64,
    last_adjustment: std::time::Instant,
}

impl DecisionEngine {
    pub fn new() -> Self {
        Self {
            cooldown_ms: 500,
            last_adjustment: std::time::Instant::now()
                - std::time::Duration::from_millis(500),
        }
    }

    /// Produce a `ControlDecision` from the latest classification.
    ///
    /// No decision is returned if:
    /// - We're inside the cooldown window
    /// - Confidence is too low (< 0.4)
    /// - The bottleneck is Unknown
    ///
    /// ## Transport awareness
    ///
    /// On QUIC, the transport handles its own congestion control and
    /// retransmission, so only parallelism and pacing rate decisions apply.
    pub fn decide(
        &mut self,
        classification: &ClassificationResult,
        current_config: &TuningConfig,
    ) -> Option<ControlDecision> {
        self.decide_for_transport(classification, current_config, None)
    }

    /// Like `decide`, but aware of the active transport mode.
    pub fn decide_for_transport(
        &mut self,
        classification: &ClassificationResult,
        current_config: &TuningConfig,
        _transport: Option<TransportMode>,
    ) -> Option<ControlDecision> {
        // Respect cooldown
        let since_last = self.last_adjustment.elapsed().as_millis() as u64;
        if since_last < self.cooldown_ms {
            return None;
        }

        if classification.confidence < 0.4 {
            return None;
        }

        if classification.bottleneck == Bottleneck::Unknown {
            return None;
        }

        #[cfg(feature = "quic")]
        let is_quic = matches!(_transport, Some(TransportMode::Quic));
        #[cfg(not(feature = "quic"))]
        let is_quic = false;

        let decision = match classification.bottleneck {
            Bottleneck::NetworkCongestion => {
                // Back off aggressively — network is saturated
                let old_rate = current_config.throughput_limit_mbps.unwrap_or(1000.0);
                let new_rate = (old_rate * 0.7).max(50.0);
                let new_rate = limit_step(old_rate, new_rate, old_rate * 0.15);
                let new_rate = new_rate.clamp(MIN_RATE_MBPS, MAX_RATE_MBPS);

                let old_chunk = current_config.chunk_size_bytes as f64;
                let new_chunk = (old_chunk * 0.75) as usize;
                let new_chunk = new_chunk.clamp(MIN_CHUNK_BYTES, MAX_CHUNK_BYTES);

                let old_parallel = current_config.parallel_streams as f64;
                let new_parallel = (old_parallel - 1.0).max(1.0) as usize;
                let new_parallel = new_parallel.clamp(MIN_PARALLELISM, MAX_PARALLELISM);

                ControlDecision {
                    new_cwnd: Some((new_rate * 1024.0) as u64),
                    new_chunk_size: Some(new_chunk),
                    parallelism: Some(new_parallel),
                    pacing_rate: Some(new_rate),
                    send_buffer_kb: Some((current_config.send_buffer_kb as f64 * 0.8) as usize),
                    reason: format!(
                        "Network congestion: reduce chunk {}→{}KB, pacing {:.0}→{:.0}Mbps, parallel {}→{}",
                        current_config.chunk_size_bytes / 1024,
                        new_chunk / 1024,
                        current_config.throughput_limit_mbps.unwrap_or(1000.0),
                        new_rate,
                        current_config.parallel_streams,
                        new_parallel,
                    ),
                }
            }

            Bottleneck::ReceiverSlow => {
                // Remote can't keep up — reduce parallelism and chunk size
                let old_rate = current_config.throughput_limit_mbps.unwrap_or(1000.0);
                let new_rate = (old_rate * 0.85).max(50.0);
                let new_rate = limit_step(old_rate, new_rate, old_rate * 0.15);
                let new_rate = new_rate.clamp(MIN_RATE_MBPS, MAX_RATE_MBPS);

                let old_chunk = current_config.chunk_size_bytes as f64;
                let new_chunk = (old_chunk * 0.8) as usize;
                let new_chunk = new_chunk.clamp(MIN_CHUNK_BYTES, MAX_CHUNK_BYTES);

                let new_parallel = current_config.parallel_streams.saturating_sub(1).max(1);
                let new_parallel = new_parallel.clamp(MIN_PARALLELISM, MAX_PARALLELISM);

                ControlDecision {
                    new_cwnd: None,
                    new_chunk_size: Some(new_chunk),
                    parallelism: Some(new_parallel),
                    pacing_rate: Some(new_rate),
                    send_buffer_kb: Some(current_config.send_buffer_kb),
                    reason: format!(
                        "Receiver slow: reduce parallelism {}→{}, chunk {}→{}KB, rate {:.0}→{:.0}Mbps",
                        current_config.parallel_streams,
                        new_parallel,
                        current_config.chunk_size_bytes / 1024,
                        new_chunk / 1024,
                        current_config.throughput_limit_mbps.unwrap_or(1000.0),
                        new_rate,
                    ),
                }
            }

            Bottleneck::CpuBound => {
                // CPU is the bottleneck — reduce parallelism, keep throughput
                let old_parallel = current_config.parallel_streams;
                let new_parallel = 1.max(old_parallel.saturating_sub(2));
                let new_parallel = new_parallel.clamp(MIN_PARALLELISM, MAX_PARALLELISM);

                ControlDecision {
                    new_cwnd: None,
                    new_chunk_size: Some(current_config.chunk_size_bytes.clamp(MIN_CHUNK_BYTES, MAX_CHUNK_BYTES)),
                    parallelism: Some(new_parallel),
                    pacing_rate: None,
                    send_buffer_kb: None,
                    reason: format!(
                        "CPU bound: reduce parallelism {}→{}",
                        old_parallel,
                        new_parallel,
                    ),
                }
            }

            Bottleneck::ThermalThrottling => {
                // Gradual reduction — device is overheating
                let old_rate = current_config.throughput_limit_mbps.unwrap_or(1000.0);
                let new_rate = (old_rate * 0.9).max(50.0);
                let new_rate = limit_step(old_rate, new_rate, old_rate * 0.15);
                let new_rate = new_rate.clamp(MIN_RATE_MBPS, MAX_RATE_MBPS);

                let old_chunk = current_config.chunk_size_bytes as f64;
                let new_chunk = (old_chunk * 0.9) as usize;
                let new_chunk = new_chunk.clamp(MIN_CHUNK_BYTES, MAX_CHUNK_BYTES);

                let old_parallel = current_config.parallel_streams;
                let new_parallel = 1.max(old_parallel.saturating_sub(1));
                let new_parallel = new_parallel.clamp(MIN_PARALLELISM, MAX_PARALLELISM);

                ControlDecision {
                    new_cwnd: None,
                    new_chunk_size: Some(new_chunk),
                    parallelism: Some(new_parallel),
                    pacing_rate: Some(new_rate),
                    send_buffer_kb: None,
                    reason: format!(
                        "Thermal throttling: reduce pacing {:.0}→{:.0}Mbps, chunk {}→{}KB, parallel {}→{}",
                        current_config.throughput_limit_mbps.unwrap_or(1000.0),
                        new_rate,
                        current_config.chunk_size_bytes / 1024,
                        new_chunk / 1024,
                        old_parallel,
                        new_parallel,
                    ),
                }
            }

            Bottleneck::LinkLimited => {
                // At max bandwidth for current conditions — gentle AI increase
                let old_rate = current_config.throughput_limit_mbps.unwrap_or(1000.0);
                let new_rate = (old_rate * 1.05).min(MAX_RATE_MBPS);
                let new_rate = limit_step(old_rate, new_rate, old_rate * 0.15);
                let new_rate = new_rate.clamp(MIN_RATE_MBPS, MAX_RATE_MBPS);

                let old_chunk = current_config.chunk_size_bytes as f64;
                let new_chunk = (old_chunk * 1.1) as usize;
                let new_chunk = new_chunk.clamp(MIN_CHUNK_BYTES, MAX_CHUNK_BYTES);

                ControlDecision {
                    new_cwnd: None,
                    new_chunk_size: Some(new_chunk),
                    parallelism: Some(current_config.parallel_streams.clamp(MIN_PARALLELISM, MAX_PARALLELISM)),
                    pacing_rate: Some(new_rate),
                    send_buffer_kb: None,
                    reason: format!(
                        "Link limited: gentle increase {:.0}→{:.0}Mbps, chunk {}→{}KB",
                        current_config.throughput_limit_mbps.unwrap_or(0.0),
                        new_rate,
                        current_config.chunk_size_bytes / 1024,
                        new_chunk / 1024,
                    ),
                }
            }

            Bottleneck::Unknown => return None,
        };

        // Post-process: on QUIC, strip decisions the transport handles itself
        let decision = if is_quic {
            ControlDecision {
                new_cwnd: None,
                new_chunk_size: None,
                send_buffer_kb: None,
                ..decision
            }
        } else {
            decision
        };

        self.last_adjustment = std::time::Instant::now();
        Some(decision)
    }

    /// Apply a decision to a mutable `TuningConfig`.
    pub fn apply(&self, decision: &ControlDecision, config: &mut TuningConfig) {
        if let Some(cwnd) = decision.new_cwnd {
            let mbps = cwnd as f64 * 8.0 / 1024.0 / 1024.0;
            config.throughput_limit_mbps = Some(mbps.clamp(MIN_RATE_MBPS, MAX_RATE_MBPS));
        }
        if let Some(chunk) = decision.new_chunk_size {
            config.chunk_size_bytes = chunk.clamp(MIN_CHUNK_BYTES, MAX_CHUNK_BYTES);
        }
        if let Some(parallel) = decision.parallelism {
            config.parallel_streams = parallel.clamp(MIN_PARALLELISM, MAX_PARALLELISM);
        }
        if let Some(rate) = decision.pacing_rate {
            config.throughput_limit_mbps = Some(rate.clamp(MIN_RATE_MBPS, MAX_RATE_MBPS));
        }
        if let Some(buf) = decision.send_buffer_kb {
            config.send_buffer_kb = buf;
            config.recv_buffer_kb = buf;
        }
    }
}
