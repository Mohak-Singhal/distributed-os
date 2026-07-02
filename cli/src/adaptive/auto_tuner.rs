/// Auto-Tuner — Hill-Climbing Parameter Optimizer
///
/// Empirically discovers the optimal `TuningConfig` during an active transfer
/// by running controlled micro-experiments (500 ms observation window each)
/// and adopting changes that improve rolling throughput by ≥ 2%.

use std::collections::VecDeque;

// ── TuningConfig ─────────────────────────────────────────────────────────────

use crate::transport::TransportMode;

/// All tunable transfer parameters.  The feedback loop writes this via an
/// `Arc<RwLock<TuningConfig>>`; the hot send path reads it with `try_read()`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TuningConfig {
    /// Bytes sent per `write_all` call.  Range: 16 KB → 1 MB.
    pub chunk_size_bytes: usize,
    /// Number of concurrent TCP streams.  Range: 1 → 8.
    pub parallel_streams: u32,
    /// `SO_SNDBUF` hint in KB.
    pub send_buffer_kb: u32,
    /// `SO_RCVBUF` hint in KB.
    pub recv_buffer_kb: u32,
    /// Number of chunks grouped in one async write batch.
    pub write_batch_size: usize,
    /// Target Tokio worker thread count.
    pub worker_threads: usize,
    /// Optional hard throughput cap in Mbps (`None` = unlimited).
    pub throughput_limit_mbps: Option<f64>,
    /// Active data path transfer mode.
    pub transport_mode: TransportMode,
}

// Default is defined in adaptive/mod.rs via `impl Default for TuningConfig`.

// ── AutoTuner ────────────────────────────────────────────────────────────────

/// Internal state for one dimension of the hill-climb.
#[derive(Clone, Debug)]
struct Dimension {
    current: usize,
    min: usize,
    max: usize,
    step: usize,
    direction: i32,   // +1 or -1
}

impl Dimension {
    fn new(current: usize, min: usize, max: usize, step: usize) -> Self {
        Dimension { current, min, max, step, direction: 1 }
    }

    fn step_up(&self) -> usize   { (self.current + self.step).min(self.max) }
    fn step_down(&self) -> usize { self.current.saturating_sub(self.step).max(self.min) }

    fn apply_direction(&mut self) -> usize {
        let val = if self.direction > 0 {
            let next = self.step_up();
            if next == self.current && self.current == self.max {
                self.direction = -1;
                self.step_down()
            } else {
                next
            }
        } else {
            let next = self.step_down();
            if next == self.current && self.current == self.min {
                self.direction = 1;
                self.step_up()
            } else {
                next
            }
        };
        self.current = val;
        val
    }
}

pub struct AutoTuner {
    /// Hill-climb dimension: chunk size
    chunk_dim: Dimension,
    /// Hill-climb dimension: parallel streams (stored as usize × 1)
    stream_dim: Dimension,
    /// Hill-climb dimension: send buffer KB
    buffer_dim: Dimension,
    /// Best throughput seen so far.
    best_throughput: f64,
    /// Best config seen so far.
    best_config: Option<TuningConfig>,
    /// Number of steps taken without improvement (used for restart).
    stale_steps: u32,
    /// Maximum number of iterations before forcing a restart.
    max_stale: u32,
    /// Rolling window of recent throughput measurements.
    recent_tp: VecDeque<f64>,
    /// Whether we are currently testing a candidate config.
    probe_active: bool,
    /// Config being tested in the current probe.
    probe_config: Option<TuningConfig>,
    /// Throughput before the current probe was applied.
    pre_probe_tp: f64,
}

impl AutoTuner {
    pub fn new() -> Self {
        AutoTuner {
            chunk_dim: Dimension::new(1_048_576, 16_384, 1_048_576, 131_072),
            stream_dim: Dimension::new(1, 1, 8, 1),
            buffer_dim: Dimension::new(4096, 256, 8192, 1024),
            best_throughput: 0.0,
            best_config: None,
            stale_steps: 0,
            max_stale: 8,
            recent_tp: VecDeque::with_capacity(10),
            probe_active: false,
            probe_config: None,
            pre_probe_tp: 0.0,
        }
    }

    /// Call once per feedback cycle (typically every 3 seconds).
    /// Returns `Some(TuningConfig)` if the tuner wants to try a new config.
    pub fn step(
        &mut self,
        current: &TuningConfig,
        current_tp: f64,
        samples: &[serde_json::Value],
    ) -> Option<TuningConfig> {
        // Sync dimensions with current configuration
        self.chunk_dim.current = current.chunk_size_bytes;
        self.stream_dim.current = current.parallel_streams as usize;
        self.buffer_dim.current = current.send_buffer_kb as usize;

        self.recent_tp.push_back(current_tp);
        if self.recent_tp.len() > 5 {
            self.recent_tp.pop_front();
        }
        let smooth_tp = self.smooth_tp();

        // ── Evaluate ongoing probe ───────────────────────────────────────────
        if self.probe_active {
            let improved = smooth_tp >= self.pre_probe_tp * 1.02;

            if improved {
                // Accept: update best
                self.best_throughput = smooth_tp;
                self.best_config = Some(current.clone());
                self.stale_steps = 0;
            } else {
                // Reject: signal caller to revert (return None → caller uses best_config)
                self.stale_steps += 1;
                // Flip direction on a stuck dimension
                self.flip_direction();
            }
            self.probe_active = false;
            self.probe_config = None;
        }

        // Initialise best if needed
        if self.best_config.is_none() {
            self.best_throughput = smooth_tp;
            self.best_config = Some(current.clone());
        }

        // ── Determine if we should restart ──────────────────────────────────
        if self.stale_steps >= self.max_stale {
            self.restart(current);
            self.stale_steps = 0;
        }

        // ── Generate next candidate ──────────────────────────────────────────
        let _ = samples; // could be used for guided step sizing in future
        let mut candidate = current.clone();

        // Cycle through dimensions round-robin
        let tick = self.stale_steps as usize;
        match tick % 3 {
            0 => {
                candidate.chunk_size_bytes = self.chunk_dim.apply_direction();
            }
            1 => {
                let new_streams = self.stream_dim.apply_direction() as u32;
                candidate.parallel_streams = new_streams;
            }
            _ => {
                let new_buf = self.buffer_dim.apply_direction() as u32;
                candidate.send_buffer_kb = new_buf;
                candidate.recv_buffer_kb = new_buf;
            }
        }

        // Don't emit the same config we already have
        if self.configs_equal(&candidate, current) {
            return None;
        }

        self.probe_active = true;
        self.probe_config = Some(candidate.clone());
        self.pre_probe_tp = smooth_tp;

        Some(candidate)
    }

    /// The best configuration discovered so far.
    pub fn best(&self) -> Option<&TuningConfig> {
        self.best_config.as_ref()
    }

    /// Human-readable summary of the tuner state.
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "best_throughput_mbps": self.best_throughput,
            "best_config": self.best_config,
            "stale_steps": self.stale_steps,
            "probe_active": self.probe_active,
        })
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn smooth_tp(&self) -> f64 {
        if self.recent_tp.is_empty() { return 0.0; }
        self.recent_tp.iter().sum::<f64>() / self.recent_tp.len() as f64
    }

    fn flip_direction(&mut self) {
        // Flip the least-recently-toggled dimension
        let tick = self.stale_steps as usize;
        match tick % 3 {
            0 => self.chunk_dim.direction = -self.chunk_dim.direction,
            1 => self.stream_dim.direction = -self.stream_dim.direction,
            _ => self.buffer_dim.direction = -self.buffer_dim.direction,
        }
    }

    fn restart(&mut self, base: &TuningConfig) {
        // Hard-reset dimensions to current values, alternate direction
        self.chunk_dim  = Dimension::new(base.chunk_size_bytes, 16_384, 1_048_576, 131_072);
        self.stream_dim = Dimension::new(base.parallel_streams as usize, 1, 8, 1);
        self.buffer_dim = Dimension::new(base.send_buffer_kb as usize, 256, 8192, 1024);
        // Reverse all directions on restart to explore the other side
        self.chunk_dim.direction  = -1;
        self.stream_dim.direction = -1;
        self.buffer_dim.direction = -1;
        self.recent_tp.clear();
    }

    fn configs_equal(&self, a: &TuningConfig, b: &TuningConfig) -> bool {
        a.chunk_size_bytes  == b.chunk_size_bytes  &&
        a.parallel_streams  == b.parallel_streams  &&
        a.send_buffer_kb    == b.send_buffer_kb
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn default_cfg() -> TuningConfig { TuningConfig::default() }

    #[test]
    fn test_first_step_generates_candidate() {
        let mut tuner = AutoTuner::new();
        let cfg = default_cfg();
        let candidate = tuner.step(&cfg, 100.0, &[]);
        // First step should always emit a candidate (no previous probe to evaluate)
        assert!(candidate.is_some(), "Expected a candidate on first step");
    }

    #[test]
    fn test_hill_climb_converges() {
        // Synthetic throughput: peaks at chunk_size = 524288 (step 3)
        fn mock_tp(cfg: &TuningConfig) -> f64 {
            let dist = (cfg.chunk_size_bytes as f64 - 524288.0).abs();
            100.0 - dist / 131072.0 * 5.0
        }

        let mut tuner = AutoTuner::new();
        let mut cfg = default_cfg();
        cfg.chunk_size_bytes = 1_048_576;

        let mut last_tp = mock_tp(&cfg);
        let mut improvements = 0;

        for _ in 0..16 {
            if let Some(candidate) = tuner.step(&cfg, last_tp, &[]) {
                let new_tp = mock_tp(&candidate);
                if new_tp > last_tp {
                    cfg = candidate;
                    last_tp = new_tp;
                    improvements += 1;
                } else {
                    // revert — tuner already noted the miss
                }
            }
        }

        // Should have found at least one improvement in 16 steps
        assert!(improvements > 0, "Hill climbing made no improvements in 16 steps");
    }

    #[test]
    fn test_stale_triggers_restart() {
        let mut tuner = AutoTuner::new();
        tuner.max_stale = 3;
        let cfg = default_cfg();

        // Drive stale_steps to max by reporting no improvement
        for _ in 0..10 {
            if let Some(_candidate) = tuner.step(&cfg, 80.0, &[]) {
                // Pretend we accepted it then rejected it on next cycle
                // (probe_active gets flipped each step)
            }
        }
        // Should not panic; stale-restart path was exercised
    }
}
