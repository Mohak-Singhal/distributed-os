use crate::control::metrics::NetworkMetrics;

/// What is limiting performance RIGHT NOW.
///
/// This is the core intelligence of the system. Getting this right
/// is what separates a smart adaptive engine from a naive one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bottleneck {
    /// Network is saturated — RTT ↑, loss ↑, throughput unstable
    NetworkCongestion,
    /// Remote endpoint can't consume data fast enough
    ReceiverSlow,
    /// Local CPU is the bottleneck (compression, syscalls)
    CpuBound,
    /// Device overheating (common on phones in hotspot mode)
    ThermalThrottling,
    /// Link bandwidth is genuinely maxed out at current conditions
    LinkLimited,
    /// Not enough data to classify yet
    Unknown,
}

/// Confidence-weighted classification result.
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub bottleneck: Bottleneck,
    /// 0.0 = guessing, 1.0 = certain
    pub confidence: f64,
    /// Human-readable rationale
    pub reason: String,
}

/// Detects the active bottleneck from a window of metrics.
///
/// Detection rules (checked in priority order):
/// 1. RTT ↑ AND loss ↑                    → NetworkCongestion
/// 2. throughput low AND loss ~0 AND queue ↑ → ReceiverSlow
/// 3. throughput ↓ AND cpu high AND loss 0 → CpuBound
/// 4. throughput slowly decays, RTT stable  → ThermalThrottling
/// 5. otherwise stable                    → LinkLimited
/// 6. insufficient data                   → Unknown
pub struct Classifier {
    /// How many consecutive samples must show a pattern before we commit
    pub confirmation_threshold: usize,
    /// Running counter of thermal-throttling detections
    thermal_counter: usize,
    /// Running counter of link-limited detections
    link_counter: usize,
}

impl Classifier {
    pub fn new() -> Self {
        Self {
            confirmation_threshold: 3,
            thermal_counter: 0,
            link_counter: 0,
        }
    }

    /// Classify the bottleneck from the recent metrics history.
    pub fn classify(&mut self, recent: &[NetworkMetrics]) -> ClassificationResult {
        if recent.len() < 3 {
            return ClassificationResult {
                bottleneck: Bottleneck::Unknown,
                confidence: 0.0,
                reason: format!("Not enough samples ({})", recent.len()),
            };
        }

        let latest = &recent[0];
        let prev = &recent[1];
        let older = &recent[2];

        // ── Compute trends ──────────────────────────────────────────────
        let rtt_trend = if prev.rtt_ms > 0.0 {
            (latest.rtt_ms - prev.rtt_ms) / prev.rtt_ms
        } else {
            0.0
        };
        let throughput_trend_short = if prev.throughput_mbps > 0.0 {
            (latest.throughput_mbps - prev.throughput_mbps) / prev.throughput_mbps
        } else {
            0.0
        };
        let throughput_trend_long = if older.throughput_mbps > 0.0 {
            (latest.throughput_mbps - older.throughput_mbps) / older.throughput_mbps
        } else {
            0.0
        };

        let has_loss = latest.packet_loss_pct > 0.5;
        let has_significant_loss = latest.packet_loss_pct > 2.0;
        let queue_high = latest.queue_delay_ms > 5.0;
        let cpu_known = latest.cpu_usage_pct > 1.0;
        let cpu_high = cpu_known && latest.cpu_usage_pct > 70.0;
        let throughput_low = latest.throughput_mbps < 50.0;
        let rtt_stable = rtt_trend.abs() < 0.03;

        // ── 1. Network congestion: RTT ↑ AND loss ↑ ────────────────────
        if rtt_trend > 0.15 && has_significant_loss {
            return ClassificationResult {
                bottleneck: Bottleneck::NetworkCongestion,
                confidence: 0.85,
                reason: format!(
                    "RTT ↑ {:.1}%, loss {:.1}% — network saturated",
                    rtt_trend * 100.0,
                    latest.packet_loss_pct,
                ),
            };
        }

        // ── 2. Receiver slow: throughput low, no loss, queue building ──
        if throughput_low && !has_loss && queue_high {
            return ClassificationResult {
                bottleneck: Bottleneck::ReceiverSlow,
                confidence: 0.80,
                reason: format!(
                    "Throughput {:.1} Mbps, loss ~0%, queue {:.1}ms — receiver can't keep up",
                    latest.throughput_mbps,
                    latest.queue_delay_ms,
                ),
            };
        }

        // ── 3. CPU bound: throughput dropping, CPU high, no loss ──────
        let cpu_indicated = throughput_trend_short < -0.1 && !has_loss
            && throughput_trend_long < -0.05
            && rtt_trend.abs() < 0.05;
        if (cpu_high || (!cpu_known && cpu_indicated && queue_high)) && throughput_trend_short < -0.1 && !has_loss {
            let cpu_display = if cpu_known {
                format!("{:.0}%", latest.cpu_usage_pct)
            } else {
                "unknown (proxy)".into()
            };
            return ClassificationResult {
                bottleneck: Bottleneck::CpuBound,
                confidence: if cpu_known { 0.75 } else { 0.55 },
                reason: format!(
                    "CPU {} throughput ↓ {:.1}%, no loss — CPU limited",
                    cpu_display,
                    throughput_trend_short * 100.0,
                ),
            };
        }

        // ── 4. Thermal throttling: slow throughput decay, stable RTT ──
        if throughput_trend_long < -0.05 && rtt_stable {
            self.thermal_counter += 1;
        } else {
            self.thermal_counter = 0;
        }

        if self.thermal_counter >= self.confirmation_threshold {
            self.thermal_counter = 0;
            return ClassificationResult {
                bottleneck: Bottleneck::ThermalThrottling,
                confidence: 0.70,
                reason: format!(
                    "Throughput decaying over {} windows ({:.1}%), RTT stable — thermal throttling",
                    self.confirmation_threshold,
                    throughput_trend_long * 100.0,
                ),
            };
        }

        // ── 4b. Link limited: stable, no loss, good RTT ────────────────
        if !has_loss && latest.rtt_ms < 30.0 && throughput_trend_short.abs() < 0.05 {
            self.link_counter += 1;
        } else {
            self.link_counter = 0;
        }

        if self.link_counter >= self.confirmation_threshold {
            self.link_counter = 0;
            return ClassificationResult {
                bottleneck: Bottleneck::LinkLimited,
                confidence: 0.75,
                reason: format!(
                    "Stable RTT {:.1}ms, no loss, throughput {:.1} Mbps — link limited",
                    latest.rtt_ms,
                    latest.throughput_mbps,
                ),
            };
        }

        // ── Fallback ──────────────────────────────────────────────────
        ClassificationResult {
            bottleneck: Bottleneck::Unknown,
            confidence: 0.3,
            reason: format!(
                "Throughput {:.1} Mbps, RTT {:.1}ms, loss {:.1}%, queue {:.1}ms — insufficient signal",
                latest.throughput_mbps,
                latest.rtt_ms,
                latest.packet_loss_pct,
                latest.queue_delay_ms,
            ),
        }
    }
}
