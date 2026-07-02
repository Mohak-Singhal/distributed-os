/// Decision Engine — Rule-Based Action Planner
///
/// Maps a `BottleneckReport` to a concrete `ActionPlan` describing what
/// configuration changes to apply.  All functions are stateless pure functions.

use super::classifier::{Bottleneck, BottleneckReport};
use super::auto_tuner::TuningConfig;

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum OptimizationAction {
    SetChunkSize(usize),
    SetParallelStreams(u32),
    SetSocketBuffer { send_kb: u32, recv_kb: u32 },
    EnableZeroCopy,
    ThrottleThroughput { target_mbps: f64 },
    IncreaseBatchSize(usize),
    SetWorkerThreads(usize),
    SwitchToQuic,
    SwitchToUdpCustom,
    NoOp,
}

impl OptimizationAction {
    pub fn description(&self) -> String {
        match self {
            OptimizationAction::SetChunkSize(b)          => format!("Set chunk size to {} KB", b / 1024),
            OptimizationAction::SetParallelStreams(n)     => format!("Set parallel streams to {}", n),
            OptimizationAction::SetSocketBuffer { send_kb, recv_kb } =>
                format!("Set socket buffers send={} KB recv={} KB", send_kb, recv_kb),
            OptimizationAction::EnableZeroCopy            => "Enable zero-copy (mmap/sendfile)".into(),
            OptimizationAction::ThrottleThroughput { target_mbps } =>
                format!("Throttle throughput to {:.1} Mbps", target_mbps),
            OptimizationAction::IncreaseBatchSize(n)     => format!("Increase write batch to {}", n),
            OptimizationAction::SetWorkerThreads(n)      => format!("Set Tokio worker threads to {}", n),
            OptimizationAction::SwitchToQuic              => "Switch to user-space QUIC transport".into(),
            OptimizationAction::SwitchToUdpCustom         => "Switch to user-space custom UDP transport".into(),
            OptimizationAction::NoOp                      => "No action required".into(),
        }
    }

    /// Apply this action to a `TuningConfig`, returning the mutated copy.
    pub fn apply(&self, mut cfg: TuningConfig) -> TuningConfig {
        match *self {
            OptimizationAction::SetChunkSize(b)          => cfg.chunk_size_bytes = b,
            OptimizationAction::SetParallelStreams(n)     => cfg.parallel_streams = n,
            OptimizationAction::SetSocketBuffer { send_kb, recv_kb } => {
                cfg.send_buffer_kb = send_kb;
                cfg.recv_buffer_kb = recv_kb;
            }
            OptimizationAction::EnableZeroCopy            => cfg.transport_mode = crate::transport::TransportMode::TcpZeroCopy,
            OptimizationAction::ThrottleThroughput { target_mbps } =>
                cfg.throughput_limit_mbps = Some(target_mbps),
            OptimizationAction::IncreaseBatchSize(n)     => cfg.write_batch_size = n,
            OptimizationAction::SetWorkerThreads(n)      => cfg.worker_threads = n,
            OptimizationAction::SwitchToQuic              => cfg.transport_mode = crate::transport::TransportMode::Quic,
            OptimizationAction::SwitchToUdpCustom         => cfg.transport_mode = crate::transport::TransportMode::UdpCustom,
            OptimizationAction::NoOp                      => {}
        }
        cfg
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActionPlan {
    /// Ordered list of actions to apply (applied left-to-right).
    pub actions: Vec<OptimizationAction>,
    /// Human-readable explanation of why these actions were chosen.
    pub rationale: String,
    /// The bottleneck this plan is targeting.
    pub target_bottleneck: Bottleneck,
    /// Confidence of the underlying classification.
    pub confidence: f64,
}

impl ActionPlan {
    pub fn noop(bottleneck: Bottleneck) -> Self {
        ActionPlan {
            actions: vec![OptimizationAction::NoOp],
            rationale: "System is operating within healthy parameters.".into(),
            target_bottleneck: bottleneck,
            confidence: 0.0,
        }
    }

    /// Apply all actions in order and return the new config.
    pub fn apply_all(&self, config: TuningConfig) -> TuningConfig {
        self.actions.iter().fold(config, |c, a| a.apply(c))
    }

    /// Summary string for the Markdown report.
    pub fn summary(&self) -> String {
        let acts: Vec<String> = self.actions.iter().map(|a| a.description()).collect();
        format!(
            "Target: **{}** (confidence: {:.0}%) → {}",
            self.target_bottleneck.label(),
            self.confidence * 100.0,
            acts.join("; ")
        )
    }
}

// ── DecisionEngine ───────────────────────────────────────────────────────────

pub struct DecisionEngine {
    cpu_count: usize,
}

impl DecisionEngine {
    pub fn new() -> Self {
        DecisionEngine { cpu_count: num_cpus::get().max(2) }
    }

    /// Produce an `ActionPlan` from a bottleneck classification report.
    pub fn plan(&self, report: &BottleneckReport, current: &TuningConfig) -> ActionPlan {
        if report.confidence < 0.15 {
            return ActionPlan::noop(report.bottleneck.clone());
        }

        let plan = match &report.bottleneck {
            Bottleneck::NetworkBound => self.plan_network(current),
            Bottleneck::CpuBound    => self.plan_cpu(current),
            Bottleneck::DiskBound   => self.plan_disk(current),
            Bottleneck::MemoryBound => self.plan_memory(current),
            Bottleneck::SchedulerBound     => self.plan_scheduler(current),
            Bottleneck::ThermalThrottling  => self.plan_thermal(current),
            Bottleneck::KernelCopyOverhead => self.plan_kernel_copy(current),
            Bottleneck::Healthy => {
                return ActionPlan::noop(Bottleneck::Healthy);
            }
        };

        ActionPlan {
            target_bottleneck: report.bottleneck.clone(),
            confidence: report.confidence,
            ..plan
        }
    }

    // ── Network bottleneck ────────────────────────────────────────────────────
    fn plan_network(&self, current: &TuningConfig) -> ActionPlan {
        let mut actions = Vec::new();

        // Larger socket buffers absorb burst variance
        if current.send_buffer_kb < 4096 {
            actions.push(OptimizationAction::SetSocketBuffer { send_kb: 4096, recv_kb: 4096 });
        }

        // Larger chunks amortise per-packet overhead
        let target_chunk = (current.chunk_size_bytes * 2).min(2 * 1024 * 1024);
        if target_chunk > current.chunk_size_bytes {
            actions.push(OptimizationAction::SetChunkSize(target_chunk));
        }

        // More parallel streams if RTT is high (fill the pipe)
        let target_streams = (current.parallel_streams + 2).min(6);
        if target_streams > current.parallel_streams {
            actions.push(OptimizationAction::SetParallelStreams(target_streams));
        }

        if actions.is_empty() { actions.push(OptimizationAction::NoOp); }

        ActionPlan {
            actions,
            rationale: "Network bottleneck: enlarging socket buffers and chunk size to saturate \
                        available bandwidth; adding parallel streams to hide RTT latency.".into(),
            target_bottleneck: Bottleneck::NetworkBound,
            confidence: 0.0, // filled in by caller
        }
    }

    // ── CPU bottleneck ────────────────────────────────────────────────────────
    fn plan_cpu(&self, current: &TuningConfig) -> ActionPlan {
        let mut actions = Vec::new();

        // Fewer, larger chunks → fewer system-call transitions
        let target_chunk = (current.chunk_size_bytes / 2).max(128 * 1024);
        if target_chunk < current.chunk_size_bytes {
            actions.push(OptimizationAction::SetChunkSize(target_chunk));
        }

        // Batch more writes to reduce per-write overhead
        let batch = (current.write_batch_size + 4).min(16);
        actions.push(OptimizationAction::IncreaseBatchSize(batch));

        // Reduce streams — fewer threads competing for the same CPU cores
        let target_streams = (current.parallel_streams.saturating_sub(1)).max(1);
        if target_streams < current.parallel_streams {
            actions.push(OptimizationAction::SetParallelStreams(target_streams));
        }

        ActionPlan {
            actions,
            rationale: "CPU bottleneck: reducing chunk size to lower per-iteration CPU cost; \
                        batching writes to amortise syscall overhead; reducing parallel streams \
                        to avoid CPU over-subscription.".into(),
            target_bottleneck: Bottleneck::CpuBound,
            confidence: 0.0,
        }
    }

    // ── Disk bottleneck ────────────────────────────────────────────────────────
    fn plan_disk(&self, current: &TuningConfig) -> ActionPlan {
        let mut actions = Vec::new();

        // Smaller chunks give writeback time to drain
        let target_chunk = (current.chunk_size_bytes / 2).max(64 * 1024);
        if target_chunk < current.chunk_size_bytes {
            actions.push(OptimizationAction::SetChunkSize(target_chunk));
        }

        // Reduce streams to reduce concurrent writeback pressure
        if current.parallel_streams > 1 {
            actions.push(OptimizationAction::SetParallelStreams(1));
        }

        // Throttle to disk sequential write speed (conservative 200 Mbps)
        actions.push(OptimizationAction::ThrottleThroughput { target_mbps: 200.0 });

        ActionPlan {
            actions,
            rationale: "Disk bottleneck: throttling throughput to let the writeback queue drain; \
                        reducing chunk size to avoid large dirty-page spikes.".into(),
            target_bottleneck: Bottleneck::DiskBound,
            confidence: 0.0,
        }
    }

    // ── Memory bottleneck ──────────────────────────────────────────────────────
    fn plan_memory(&self, current: &TuningConfig) -> ActionPlan {
        let mut actions = Vec::new();

        // Smaller chunks → smaller peak in-flight allocation
        let target_chunk = (current.chunk_size_bytes / 2).max(128 * 1024);
        if target_chunk < current.chunk_size_bytes {
            actions.push(OptimizationAction::SetChunkSize(target_chunk));
        }

        // Single stream reduces buffer duplication
        if current.parallel_streams > 1 {
            actions.push(OptimizationAction::SetParallelStreams(1));
        }

        ActionPlan {
            actions,
            rationale: "Memory bottleneck: reducing chunk size and parallel streams to lower \
                        peak heap allocation and major page-fault rate.".into(),
            target_bottleneck: Bottleneck::MemoryBound,
            confidence: 0.0,
        }
    }

    // ── Scheduler bottleneck ─────────────────────────────────────────────────
    fn plan_scheduler(&self, current: &TuningConfig) -> ActionPlan {
        let mut actions = Vec::new();

        // Add more Tokio workers to absorb run-queue backlog
        let target_workers = (current.worker_threads + 2).min(self.cpu_count * 3);
        if target_workers > current.worker_threads {
            actions.push(OptimizationAction::SetWorkerThreads(target_workers));
        }

        // Larger chunks → fewer context-switch inducing write calls
        let target_chunk = (current.chunk_size_bytes * 2).min(2 * 1024 * 1024);
        if target_chunk > current.chunk_size_bytes {
            actions.push(OptimizationAction::SetChunkSize(target_chunk));
        }

        ActionPlan {
            actions,
            rationale: "Scheduler bottleneck: increasing Tokio worker pool to drain run queue; \
                        enlarging chunk size to reduce the frequency of async task yields.".into(),
            target_bottleneck: Bottleneck::SchedulerBound,
            confidence: 0.0,
        }
    }

    // ── Thermal throttling ────────────────────────────────────────────────────
    fn plan_thermal(&self, current: &TuningConfig) -> ActionPlan {
        let mut actions = Vec::new();

        // Halve throughput to allow the CPU to cool
        let current_tp = current.throughput_limit_mbps.unwrap_or(1000.0);
        let target_tp = (current_tp * 0.6).max(50.0);
        actions.push(OptimizationAction::ThrottleThroughput { target_mbps: target_tp });

        // Single stream reduces CPU heat generation
        if current.parallel_streams > 1 {
            actions.push(OptimizationAction::SetParallelStreams(1));
        }

        // Larger chunks → fewer CPU wake-ups per MB transferred
        let target_chunk = (current.chunk_size_bytes * 2).min(2 * 1024 * 1024);
        if target_chunk > current.chunk_size_bytes {
            actions.push(OptimizationAction::SetChunkSize(target_chunk));
        }

        ActionPlan {
            actions,
            rationale: "Thermal throttling detected: reducing throughput and parallel streams \
                        to allow the SoC to cool and restore full CPU frequency.".into(),
            target_bottleneck: Bottleneck::ThermalThrottling,
            confidence: 0.0,
        }
    }

    // ── Kernel copy overhead ──────────────────────────────────────────────────
    fn plan_kernel_copy(&self, current: &TuningConfig) -> ActionPlan {
        let mut actions = Vec::new();
        actions.push(OptimizationAction::EnableZeroCopy);

        // Larger chunks → fewer copies per MB (each mmap read covers more data)
        let target_chunk = (current.chunk_size_bytes * 2).min(2 * 1024 * 1024);
        if target_chunk > current.chunk_size_bytes {
            actions.push(OptimizationAction::SetChunkSize(target_chunk));
        }

        ActionPlan {
            actions,
            rationale: "Kernel copy overhead: enabling zero-copy (mmap/sendfile) and enlarging \
                        chunk size to reduce per-MB kernel↔user copy count.".into(),
            target_bottleneck: Bottleneck::KernelCopyOverhead,
            confidence: 0.0,
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptive::classifier::{Bottleneck, BottleneckReport};
    use std::collections::HashMap;

    fn report(b: Bottleneck, conf: f64) -> BottleneckReport {
        BottleneckReport {
            bottleneck: b,
            confidence: conf,
            signals: vec![],
            scores: HashMap::new(),
            timestamp_ms: 0,
        }
    }

    fn default_cfg() -> TuningConfig {
        TuningConfig::default()
    }

    #[test]
    fn test_low_confidence_returns_noop() {
        let de = DecisionEngine::new();
        let plan = de.plan(&report(Bottleneck::NetworkBound, 0.10), &default_cfg());
        assert!(matches!(plan.actions[0], OptimizationAction::NoOp));
    }

    #[test]
    fn test_network_plan_increases_buffers() {
        let de = DecisionEngine::new();
        let mut cfg = default_cfg();
        cfg.send_buffer_kb = 256;
        let plan = de.plan(&report(Bottleneck::NetworkBound, 0.80), &cfg);
        let has_buf = plan.actions.iter().any(|a| matches!(a, OptimizationAction::SetSocketBuffer { .. }));
        assert!(has_buf, "Expected SetSocketBuffer action");
    }

    #[test]
    fn test_thermal_throttles_throughput() {
        let de = DecisionEngine::new();
        let plan = de.plan(&report(Bottleneck::ThermalThrottling, 0.90), &default_cfg());
        let has_tp = plan.actions.iter().any(|a| matches!(a, OptimizationAction::ThrottleThroughput { .. }));
        assert!(has_tp, "Expected ThrottleThroughput action");
    }

    #[test]
    fn test_kernel_copy_enables_zero_copy() {
        let de = DecisionEngine::new();
        let plan = de.plan(&report(Bottleneck::KernelCopyOverhead, 0.70), &default_cfg());
        let has_zc = plan.actions.iter().any(|a| matches!(a, OptimizationAction::EnableZeroCopy));
        assert!(has_zc, "Expected EnableZeroCopy action");
    }

    #[test]
    fn test_apply_all_mutates_config() {
        let de = DecisionEngine::new();
        let mut cfg = default_cfg();
        cfg.send_buffer_kb = 256;
        let plan = de.plan(&report(Bottleneck::NetworkBound, 0.85), &cfg);
        let new_cfg = plan.apply_all(cfg.clone());
        // At least one field must differ
        assert!(
            new_cfg.send_buffer_kb != cfg.send_buffer_kb
            || new_cfg.chunk_size_bytes != cfg.chunk_size_bytes
            || new_cfg.parallel_streams != cfg.parallel_streams,
            "apply_all should change at least one field"
        );
    }
}
