use std::collections::VecDeque;
use std::time::Instant;

/// Exponentially Weighted Moving Average for smoothing noisy samples.
pub struct Ewma {
    value: f64,
    alpha: f64,
    initialized: bool,
}

impl Ewma {
    pub fn new(alpha: f64) -> Self {
        Self { value: 0.0, alpha, initialized: false }
    }

    pub fn update(&mut self, sample: f64) -> f64 {
        if !self.initialized {
            self.value = sample;
            self.initialized = true;
        } else {
            self.value = self.alpha * sample + (1.0 - self.alpha) * self.value;
        }
        self.value
    }

    pub fn get(&self) -> f64 {
        self.value
    }
}

/// Every 100ms we snapshot the connection state into one of these.
#[derive(Debug, Clone)]
pub struct NetworkMetrics {
    /// Throughput over the last window (Mbps)
    pub throughput_mbps: f64,
    /// Smoothed round-trip time (ms)
    pub rtt_ms: f64,
    /// Packet loss estimate derived from write stalls / retries
    pub packet_loss_pct: f64,
    /// How many bytes are sitting in the socket send buffer
    pub in_flight_bytes: u64,
    /// Current congestion window (bytes)
    pub cwnd_bytes: u64,
    /// Retransmissions in this window
    pub retransmits: u32,
    /// How long the last write took beyond expected (ms)
    pub queue_delay_ms: f64,
    /// CPU usage % of the transfer task
    pub cpu_usage_pct: f64,
    /// Wall-clock timestamp
    pub timestamp: Instant,
}

impl Default for NetworkMetrics {
    fn default() -> Self {
        Self {
            throughput_mbps: 0.0,
            rtt_ms: 50.0,
            packet_loss_pct: 0.0,
            in_flight_bytes: 0,
            cwnd_bytes: 65535,
            retransmits: 0,
            queue_delay_ms: 0.0,
            cpu_usage_pct: 0.0,
            timestamp: Instant::now(),
        }
    }
}

/// Rolling window of the last N metrics snapshots.
pub struct MetricsHistory {
    samples: VecDeque<NetworkMetrics>,
    max_samples: usize,
    /// Cumulative bytes written since the previous sample (incl. retransmits)
    pub window_bytes: u64,
    /// Cumulative unique bytes (excl. retransmits) — used for goodput
    pub window_goodput_bytes: u64,
    /// Cumulative stalled bytes
    pub window_retransmits: u32,
    /// Sum of write durations in the window (for queue delay)
    pub window_write_us: u64,
    /// Write call count in window
    pub window_write_count: u64,
    /// Start of the current 100 ms window
    pub window_start: Instant,
    /// Last recorded RTT sample (request-response from receiver)
    pub last_rtt_ms: f64,
    /// EWMA for throughput (α = 0.25)
    ewma_throughput: Ewma,
    /// EWMA for RTT (α = 0.25)
    ewma_rtt: Ewma,
    /// EWMA for queue delay (α = 0.25)
    ewma_queue_delay: Ewma,
}

impl MetricsHistory {
    pub fn new(max: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(max),
            max_samples: max,
            window_bytes: 0,
            window_goodput_bytes: 0,
            window_retransmits: 0,
            window_write_us: 0,
            window_write_count: 0,
            window_start: Instant::now(),
            last_rtt_ms: 50.0,
            ewma_throughput: Ewma::new(0.25),
            ewma_rtt: Ewma::new(0.25),
            ewma_queue_delay: Ewma::new(0.25),
        }
    }

    /// Clear all metrics history (for resume).
    pub fn clear(&mut self) {
        self.samples.clear();
        self.window_bytes = 0;
        self.window_goodput_bytes = 0;
        self.window_retransmits = 0;
        self.window_write_us = 0;
        self.window_write_count = 0;
        self.window_start = Instant::now();
        self.last_rtt_ms = 50.0;
        self.ewma_throughput = Ewma::new(0.25);
        self.ewma_rtt = Ewma::new(0.25);
        self.ewma_queue_delay = Ewma::new(0.25);
    }

    /// Record a single write operation.
    pub fn record_write(&mut self, bytes: u64, duration_us: u64, is_retransmit: bool) {
        self.window_bytes += bytes;
        if !is_retransmit {
            self.window_goodput_bytes += bytes;
        }
        self.window_write_us += duration_us;
        self.window_write_count += 1;
        if is_retransmit {
            self.window_retransmits += 1;
        }
    }

    /// Record an RTT measurement (e.g. from ACK timing).
    pub fn record_rtt(&mut self, rtt_ms: f64) {
        self.last_rtt_ms = rtt_ms;
    }

    /// Snapshot the current window into a `NetworkMetrics` and reset counters.
    pub fn snapshot(&mut self) -> NetworkMetrics {
        let elapsed = self.window_start.elapsed();
        let elapsed_s = elapsed.as_secs_f64().max(0.001);

        let throughput = (self.window_goodput_bytes.max(1) as f64 * 8.0) / (elapsed_s * 1_000_000.0);
        let queue_delay = if self.window_write_count > 0 {
            self.window_write_us as f64 / self.window_write_count as f64 / 1000.0
        } else {
            0.0
        };
        let loss = if self.window_bytes > 0 {
            (self.window_retransmits as f64 * 100.0)
                / (self.window_bytes as f64 / self.window_write_count.max(1) as f64)
        } else {
            0.0
        };

        // Smooth all three signals with EWMA
        let smoothed_throughput = self.ewma_throughput.update(throughput);
        let smoothed_rtt = if self.last_rtt_ms > 0.0 {
            self.ewma_rtt.update(self.last_rtt_ms)
        } else {
            self.ewma_rtt.get()
        };
        let smoothed_queue_delay = self.ewma_queue_delay.update(queue_delay);

        let metrics = NetworkMetrics {
            throughput_mbps: smoothed_throughput,
            rtt_ms: smoothed_rtt,
            packet_loss_pct: loss.min(100.0),
            in_flight_bytes: self.window_bytes,
            cwnd_bytes: (self.window_bytes * 2).max(65535), // heuristic
            retransmits: self.window_retransmits,
            queue_delay_ms: smoothed_queue_delay,
            cpu_usage_pct: 0.0, // set externally if available
            timestamp: Instant::now(),
        };

        // Push to rolling history
        self.samples.push_back(metrics.clone());
        while self.samples.len() > self.max_samples {
            self.samples.pop_front();
        }

        // Reset window counters
        self.window_bytes = 0;
        self.window_goodput_bytes = 0;
        self.window_retransmits = 0;
        self.window_write_us = 0;
        self.window_write_count = 0;
        self.window_start = Instant::now();
        self.last_rtt_ms = 0.0;

        metrics
    }

    /// Return the last N samples (for classifier).
    pub fn last_n(&self, n: usize) -> Vec<NetworkMetrics> {
        let n = n.min(self.samples.len());
        self.samples.iter().rev().take(n).cloned().collect()
    }

    /// Return the number of samples collected so far.
    pub fn len(&self) -> usize {
        self.samples.len()
    }
}
