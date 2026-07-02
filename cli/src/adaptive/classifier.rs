/// Bottleneck Classification Engine
///
/// Uses a weighted scoring model that normalises all telemetry signals to [0,1]
/// and combines rule-based heuristics with statistical thresholds to produce a
/// dominant `Bottleneck` label with a confidence score.

use std::collections::HashMap;
use serde_json::Value;

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Bottleneck {
    CpuBound,
    NetworkBound,
    DiskBound,
    MemoryBound,
    SchedulerBound,
    ThermalThrottling,
    KernelCopyOverhead,
    Healthy,
}

impl Bottleneck {
    pub fn label(&self) -> &'static str {
        match self {
            Bottleneck::CpuBound          => "CPU_BOUND",
            Bottleneck::NetworkBound      => "NETWORK_BOUND",
            Bottleneck::DiskBound         => "DISK_BOUND",
            Bottleneck::MemoryBound       => "MEMORY_BOUND",
            Bottleneck::SchedulerBound    => "SCHEDULER_BOUND",
            Bottleneck::ThermalThrottling => "THERMAL_THROTTLING",
            Bottleneck::KernelCopyOverhead=> "KERNEL_COPY_OVERHEAD",
            Bottleneck::Healthy           => "HEALTHY",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BottleneckReport {
    pub bottleneck: Bottleneck,
    /// 0.0 – 1.0 confidence that this is the dominant bottleneck.
    pub confidence: f64,
    /// Human-readable signals that contributed to the decision.
    pub signals: Vec<String>,
    /// Raw normalised score per category (useful for debugging / SVG rendering).
    pub scores: HashMap<String, f64>,
    pub timestamp_ms: u64,
}

// ── Classifier ───────────────────────────────────────────────────────────────

pub struct Classifier;

impl Classifier {
    pub fn new() -> Self { Classifier }

    /// Classify the dominant bottleneck from the last N telemetry samples.
    pub fn classify(&self, samples: &[Value]) -> BottleneckReport {
        let n = samples.len();
        let ts = samples.last()
            .and_then(|s| s.get("timestamp_ms"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if n < 2 {
            return BottleneckReport {
                bottleneck: Bottleneck::Healthy,
                confidence: 0.0,
                signals: vec!["insufficient_samples".into()],
                scores: HashMap::new(),
                timestamp_ms: ts,
            };
        }

        // ── Extract aggregate metrics ────────────────────────────────────────
        let avg = |key: &str| -> f64 {
            let vals: Vec<f64> = samples.iter()
                .filter_map(|s| s.get(key).and_then(|v| v.as_f64()))
                .collect();
            if vals.is_empty() { 0.0 } else { vals.iter().sum::<f64>() / vals.len() as f64 }
        };
        let max_val = |key: &str| -> f64 {
            samples.iter()
                .filter_map(|s| s.get(key).and_then(|v| v.as_f64()))
                .fold(0.0f64, f64::max)
        };
        let sum_val = |key: &str| -> f64 {
            samples.iter()
                .filter_map(|s| s.get(key).and_then(|v| v.as_f64()))
                .sum()
        };

        let avg_cpu              = avg("cpu_pct");
        let avg_rtt              = avg("rtt_ms");
        let avg_tp               = avg("rolling_throughput_mbps");
        let max_rtt              = max_val("rtt_ms");
        let avg_retrans          = avg("sched_context_switches");      // proxy; real retransmit count
        let sum_retransmissions: f64 = {
            // If dedicated key exists use it; else estimate from context switches
            let r = sum_val("net_retransmissions");
            if r > 0.0 { r } else { sum_val("sched_context_switches") * 0.01 }
        };
        let avg_cwnd             = avg("cwnd");
        let avg_dirty_kb         = avg("fs_dirty_kb");
        let avg_writeback_kb     = avg("fs_writeback_kb");
        let avg_rss_growth: f64  = samples.last()
            .and_then(|s| s.get("mem_growth_bytes"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let avg_major_faults     = avg("mem_major_faults");
        let avg_run_q            = avg("sched_run_queue_length");
        let avg_ctx_sw           = avg("sched_context_switches");
        let avg_sched_latency    = avg("sched_latency_ms");
        let avg_throttle: f64    = samples.iter()
            .filter_map(|s| s.get("hw_thermal_throttle").and_then(|v| v.as_u64()))
            .map(|v| v as f64)
            .sum::<f64>() / n as f64;
        let avg_soc_temp         = avg("hw_soc_temp_c");
        let avg_freq_mhz         = avg("hw_cpu_freq_mhz");
        let avg_scaling_pct      = avg("hw_cpu_scaling_pct");
        let avg_loss             = avg("packet_loss_pct");
        let avg_rtt_var          = avg("rtt_variance_ms");

        // Compute throughput-vs-frequency Pearson r (thermal correlation)
        let mean_tp   = avg_tp;
        let mean_freq = avg_freq_mhz;
        let (mut num, mut den_tp, mut den_freq) = (0.0f64, 0.0f64, 0.0f64);
        for s in samples {
            let tp   = s.get("rolling_throughput_mbps").and_then(|v| v.as_f64()).unwrap_or(mean_tp);
            let freq = s.get("hw_cpu_freq_mhz").and_then(|v| v.as_f64()).unwrap_or(mean_freq);
            let dt = tp - mean_tp;
            let df = freq - mean_freq;
            num      += dt * df;
            den_tp   += dt * dt;
            den_freq += df * df;
        }
        let pearson_r = if den_tp > 0.0 && den_freq > 0.0 {
            num / (den_tp * den_freq).sqrt()
        } else { 0.0 };

        // ── Score each category (0.0 – 1.0) ─────────────────────────────────

        // CPU_BOUND: high cpu, normal network, normal disk
        let cpu_score = {
            let s1 = norm(avg_cpu, 50.0, 100.0);           // cpu > 50% → 1.0
            let s2 = (1.0 - norm(avg_rtt, 5.0, 150.0)).max(0.0); // low rtt is good
            let s3 = (1.0 - norm(sum_retransmissions, 0.0, 50.0)).max(0.0);
            ((s1 * 0.6 + s2 * 0.2 + s3 * 0.2).min(1.0) * s1).min(1.0)
        };

        // NETWORK_BOUND: high rtt, retransmissions, cwnd saturated, low cpu, packet loss, or high rtt variance
        let net_score = {
            let s1 = norm(avg_rtt, 5.0, 200.0);
            let s2 = norm(sum_retransmissions, 0.0, 100.0);
            let s3 = (1.0 - norm(avg_cwnd, 5.0, 64.0)).max(0.0); // small cwnd = saturated
            let s4 = (1.0 - norm(avg_cpu, 20.0, 80.0)).max(0.0);
            let s5 = norm(avg_loss, 0.0, 15.0);
            let s6 = norm(avg_rtt_var, 0.0, 50.0);
            let raw = (s1 * 0.25 + s2 * 0.20 + s3 * 0.15 + s4 * 0.10 + s5 * 0.20 + s6 * 0.10).min(1.0);
            let presence = (s1 + s2 + s5).min(1.0);
            (raw * presence).min(1.0)
        };

        // DISK_BOUND: dirty pages spike, writeback high
        let disk_score = {
            let s1 = norm(avg_dirty_kb, 8_000.0, 64_000.0);
            let s2 = norm(avg_writeback_kb, 2_000.0, 32_000.0);
            (s1 * 0.5 + s2 * 0.5).min(1.0)
        };

        // MEMORY_BOUND: large RSS growth, major faults
        let mem_score = {
            let s1 = norm(avg_rss_growth, 15.0 * 1024.0 * 1024.0, 100.0 * 1024.0 * 1024.0);
            let s2 = norm(avg_major_faults, 10.0, 200.0);
            (s1 * 0.6 + s2 * 0.4).min(1.0)
        };

        // SCHEDULER_BOUND: run queue, context switches, latency
        let sched_score = {
            let s1 = norm(avg_run_q, 4.0, 12.0);
            let s2 = norm(avg_ctx_sw, 200.0, 600.0);
            let s3 = norm(avg_sched_latency, 8.0, 30.0);
            (s1 * 0.4 + s2 * 0.3 + s3 * 0.3).min(1.0)
        };

        // THERMAL_THROTTLING: throttle flag, high temp, negative tp-freq correlation
        let thermal_score = {
            let s1 = norm(avg_throttle, 0.0, 1.0);
            let s2 = norm(avg_soc_temp, 65.0, 90.0);
            let s3 = norm(100.0 - avg_scaling_pct, 0.0, 40.0);
            let s4 = if pearson_r < -0.2 { norm(-pearson_r, 0.2, 1.0) } else { 0.0 };
            (s1 * 0.35 + s2 * 0.25 + s3 * 0.20 + s4 * 0.20).min(1.0)
        };

        // KERNEL_COPY_OVERHEAD: high syscall rate, low zero-copy, high recv/send latency
        let kernel_score = {
            let syscall_rate: f64 = samples.iter()
                .filter_map(|s| s.get("syscall_rate_per_sec").and_then(|v| v.as_f64()))
                .sum::<f64>() / n as f64;
            let s1 = norm(syscall_rate, 5_000.0, 50_000.0);
            // Proxy: if cpu is moderate but throughput is low → copy overhead
            let tp_efficiency = if avg_cpu > 5.0 { avg_tp / avg_cpu } else { 0.0 };
            let s2 = (1.0 - norm(tp_efficiency, 0.5, 5.0)).max(0.0);
            (s1 * 0.5 + s2 * 0.5).min(1.0)
        };

        let mut scores: HashMap<String, f64> = [
            ("CPU_BOUND".into(), cpu_score),
            ("NETWORK_BOUND".into(), net_score),
            ("DISK_BOUND".into(), disk_score),
            ("MEMORY_BOUND".into(), mem_score),
            ("SCHEDULER_BOUND".into(), sched_score),
            ("THERMAL_THROTTLING".into(), thermal_score),
            ("KERNEL_COPY_OVERHEAD".into(), kernel_score),
        ].into();

        // ── Pick winner ──────────────────────────────────────────────────────
        let (winner_label, &winner_score) = scores
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();

        let second_score = scores
            .iter()
            .filter(|(k, _)| k.as_str() != winner_label.as_str())
            .map(|(_, &v)| v)
            .fold(0.0f64, f64::max);

        // Confidence = margin over second-best, normalised; clamp to [0,1]
        let confidence = if winner_score < 0.05 {
            0.0
        } else {
            ((winner_score - second_score) / winner_score.max(0.01)).clamp(0.0, 1.0)
        };

        let bottleneck = if winner_score < 0.08 {
            Bottleneck::Healthy
        } else {
            match winner_label.as_str() {
                "CPU_BOUND"           => Bottleneck::CpuBound,
                "NETWORK_BOUND"       => Bottleneck::NetworkBound,
                "DISK_BOUND"          => Bottleneck::DiskBound,
                "MEMORY_BOUND"        => Bottleneck::MemoryBound,
                "SCHEDULER_BOUND"     => Bottleneck::SchedulerBound,
                "THERMAL_THROTTLING"  => Bottleneck::ThermalThrottling,
                "KERNEL_COPY_OVERHEAD"=> Bottleneck::KernelCopyOverhead,
                _                     => Bottleneck::Healthy,
            }
        };

        // ── Build signals list ───────────────────────────────────────────────
        let mut signals = Vec::new();
        if avg_cpu > 70.0         { signals.push(format!("high_cpu_utilization ({:.0}%)", avg_cpu)); }
        if avg_rtt > 30.0         { signals.push(format!("elevated_rtt ({:.1}ms)", avg_rtt)); }
        if max_rtt > 100.0        { signals.push(format!("rtt_spike ({:.1}ms peak)", max_rtt)); }
        if sum_retransmissions > 5.0 { signals.push(format!("retransmissions_detected ({:.0})", sum_retransmissions)); }
        if avg_dirty_kb > 16_000.0  { signals.push(format!("high_dirty_pages ({:.0} KB)", avg_dirty_kb)); }
        if avg_writeback_kb > 4_000.0 { signals.push(format!("writeback_pressure ({:.0} KB)", avg_writeback_kb)); }
        if avg_rss_growth > 20.0 * 1024.0 * 1024.0 { signals.push(format!("rss_growth ({:.1} MB)", avg_rss_growth / 1024.0 / 1024.0)); }
        if avg_major_faults > 20.0  { signals.push(format!("major_page_faults ({:.0})", avg_major_faults)); }
        if avg_run_q > 4.0          { signals.push(format!("run_queue_saturation ({:.1})", avg_run_q)); }
        if avg_ctx_sw > 300.0       { signals.push(format!("high_context_switches ({:.0}/sample)", avg_ctx_sw)); }
        if avg_sched_latency > 10.0 { signals.push(format!("sched_latency ({:.1}ms)", avg_sched_latency)); }
        if avg_throttle > 0.0       { signals.push(format!("thermal_throttle_active ({:.0}% ticks)", avg_throttle * 100.0)); }
        if avg_soc_temp > 70.0      { signals.push(format!("soc_temperature ({:.1}°C)", avg_soc_temp)); }
        if pearson_r < -0.3         { signals.push(format!("tp_freq_anticorrelation (r={:.2})", pearson_r)); }
        if avg_cpu < 20.0 && avg_tp < 50.0 { signals.push("low_cpu_with_low_throughput".into()); }
        if avg_loss > 1.0         { signals.push(format!("packet_loss_detected ({:.2}%)", avg_loss)); }
        if avg_rtt_var > 15.0     { signals.push(format!("rtt_jitter ({:.1}ms variance)", avg_rtt_var)); }

        BottleneckReport { bottleneck, confidence, signals, scores, timestamp_ms: ts }
    }
}

// ── Helper: linear normalisation → [0,1] ────────────────────────────────────
/// Maps `value` linearly from [low, high] → [0, 1], clamped.
fn norm(value: f64, low: f64, high: f64) -> f64 {
    if high <= low { return 0.0; }
    ((value - low) / (high - low)).clamp(0.0, 1.0)
}

// ── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_sample(overrides: serde_json::Value) -> serde_json::Value {
        let mut base = json!({
            "timestamp_ms": 1000,
            "rolling_throughput_mbps": 80.0,
            "cpu_pct": 15.0,
            "rtt_ms": 2.0,
            "cwnd": 32,
            "sched_run_queue_length": 1.0,
            "sched_context_switches": 80.0,
            "sched_latency_ms": 1.0,
            "fs_dirty_kb": 500.0,
            "fs_writeback_kb": 100.0,
            "mem_growth_bytes": 0.0,
            "mem_major_faults": 0.0,
            "hw_thermal_throttle": 0,
            "hw_soc_temp_c": 45.0,
            "hw_cpu_freq_mhz": 2400.0,
            "hw_cpu_scaling_pct": 100.0
        });
        if let (Some(m), Some(o)) = (base.as_object_mut(), overrides.as_object()) {
            for (k, v) in o { m.insert(k.clone(), v.clone()); }
        }
        base
    }

    #[test]
    fn test_healthy_baseline() {
        let c = Classifier::new();
        let samples: Vec<_> = (0..5).map(|i| make_sample(json!({ "timestamp_ms": i * 100 }))).collect();
        let r = c.classify(&samples);
        assert!(matches!(r.bottleneck, Bottleneck::Healthy), "Expected Healthy, got {:?}", r.bottleneck);
    }

    #[test]
    fn test_cpu_bound_detection() {
        let c = Classifier::new();
        let samples: Vec<_> = (0..5).map(|i| make_sample(json!({
            "timestamp_ms": i * 100,
            "cpu_pct": 92.0,
            "rtt_ms": 2.0,
        }))).collect();
        let r = c.classify(&samples);
        assert!(matches!(r.bottleneck, Bottleneck::CpuBound), "Expected CpuBound, got {:?}", r.bottleneck);
        assert!(r.confidence > 0.0);
    }

    #[test]
    fn test_network_bound_detection() {
        let c = Classifier::new();
        let samples: Vec<_> = (0..5).map(|i| make_sample(json!({
            "timestamp_ms": i * 100,
            "rtt_ms": 120.0,
            "cwnd": 3,
            "cpu_pct": 8.0,
            "net_retransmissions": 40.0,
        }))).collect();
        let r = c.classify(&samples);
        assert!(matches!(r.bottleneck, Bottleneck::NetworkBound), "Expected NetworkBound, got {:?}", r.bottleneck);
    }

    #[test]
    fn test_thermal_throttling_detection() {
        let c = Classifier::new();
        let samples: Vec<_> = (0..5).map(|i| make_sample(json!({
            "timestamp_ms": i * 100,
            "hw_thermal_throttle": 1,
            "hw_soc_temp_c": 82.0,
            "hw_cpu_scaling_pct": 65.0,
            "hw_cpu_freq_mhz": 1000.0,
            "rolling_throughput_mbps": 30.0,
        }))).collect();
        let r = c.classify(&samples);
        assert!(matches!(r.bottleneck, Bottleneck::ThermalThrottling), "Expected ThermalThrottling, got {:?}", r.bottleneck);
        assert!(r.signals.iter().any(|s| s.contains("thermal_throttle_active")));
    }

    #[test]
    fn test_scheduler_bound_detection() {
        let c = Classifier::new();
        let samples: Vec<_> = (0..5).map(|i| make_sample(json!({
            "timestamp_ms": i * 100,
            "sched_run_queue_length": 9.0,
            "sched_context_switches": 500.0,
            "sched_latency_ms": 22.0,
            "cpu_pct": 45.0,
        }))).collect();
        let r = c.classify(&samples);
        assert!(matches!(r.bottleneck, Bottleneck::SchedulerBound), "Expected SchedulerBound, got {:?}", r.bottleneck);
    }
}
