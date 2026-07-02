use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

pub struct TransferMonitor {
    pub start: Instant,
    pub last_sample: std::sync::Mutex<Instant>,
    pub bytes_last_sample: tokio::sync::Mutex<u64>,
    pub bytes_total: tokio::sync::Mutex<u64>,
    pub speed_samples: tokio::sync::Mutex<Vec<f64>>,
    pub peak_speed_mbps: tokio::sync::Mutex<f64>,
    pub retransmits: tokio::sync::Mutex<u64>,
    pub packet_loss: tokio::sync::Mutex<f64>,
    pub running: Arc<std::sync::atomic::AtomicBool>,
}

impl TransferMonitor {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            start: now,
            last_sample: std::sync::Mutex::new(now),
            bytes_last_sample: tokio::sync::Mutex::new(0),
            bytes_total: tokio::sync::Mutex::new(0),
            speed_samples: tokio::sync::Mutex::new(Vec::with_capacity(100)),
            peak_speed_mbps: tokio::sync::Mutex::new(0.0),
            retransmits: tokio::sync::Mutex::new(0),
            packet_loss: tokio::sync::Mutex::new(0.0),
            running: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        }
    }

    pub async fn record_bytes(&self, n: u64) {
        let mut total = self.bytes_total.lock().await;
        *total += n;
    }

    pub async fn sample_speed(&self) -> f64 {
        let now = Instant::now();
        let elapsed = now.duration_since(*self.last_sample.lock().unwrap()).as_secs_f64();
        if elapsed < 0.5 { return self.speed_samples.lock().await.last().copied().unwrap_or(0.0); }

        let total = *self.bytes_total.lock().await;
        let last = *self.bytes_last_sample.lock().await;
        let delta = total.saturating_sub(last);
        *self.bytes_last_sample.lock().await = total;
        *self.last_sample.lock().unwrap() = now;

        let speed = if elapsed > 0.0 { (delta as f64 * 8.0) / (elapsed * 1_000_000.0) } else { 0.0 };
        let mut samples = self.speed_samples.lock().await;
        samples.push(speed);
        if samples.len() > 60 { samples.remove(0); }

        let mut peak = self.peak_speed_mbps.lock().await;
        if speed > *peak { *peak = speed; }

        speed
    }

    pub async fn current_speed(&self) -> f64 {
        self.speed_samples.lock().await.last().copied().unwrap_or(0.0)
    }

    pub async fn average_speed(&self) -> f64 {
        let samples = self.speed_samples.lock().await;
        if samples.is_empty() { return 0.0; }
        samples.iter().sum::<f64>() / samples.len() as f64
    }

    pub async fn elapsed_secs(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    pub async fn eta_secs(&self, remaining: u64) -> f64 {
        let speed = self.current_speed().await;
        if speed <= 0.0 { return f64::INFINITY; }
        (remaining as f64 * 8.0) / (speed * 1_000_000.0)
    }
}

// ── Live Adaptive Controller ──────────────────────────────────────────────

/// Parameters the adaptive controller can tune mid-transfer.
#[derive(Debug, Clone)]
pub struct AdaptiveHints {
    pub suggested_chunk_size: usize,
    pub suggested_streams: u32,
    pub should_pause: bool,
}

/// Run a live adaptive loop that monitors throughput and suggests profile
/// adjustments every `interval_ms` milliseconds.
pub async fn adaptive_controller_loop(
    monitor: &TransferMonitor,
    expected_mbps: f64,
    interval_ms: u64,
) -> AdaptiveHints {
    let speed = monitor.sample_speed().await;
    let avg = monitor.average_speed().await;
    let elapsed = monitor.elapsed_secs().await;

    let mut chunk_size: usize = 1_048_576;
    let mut streams: u32 = 1;
    let mut should_pause = false;

    if elapsed > 2.0 && avg > 0.0 {
        let utilization = avg / expected_mbps.max(1.0);

        // Dynamic chunk sizing based on throughput
        if avg > 500.0 {
            chunk_size = 8_388_608; // 8 MB for >500 Mbps
        } else if avg > 200.0 {
            chunk_size = 4_194_304; // 4 MB for >200 Mbps
        } else if avg > 50.0 {
            chunk_size = 1_048_576; // 1 MB for >50 Mbps
        } else {
            chunk_size = 262_144; // 256 KB for slow links
        }

        // Stream count AIMD: increase if under-utilized
        if utilization < 0.5 {
            streams = 4;
        } else if utilization < 0.7 {
            streams = 2;
        } else {
            streams = 1;
        }

        // Pause if utilization is critically low (<10% of expected)
        if utilization < 0.1 && elapsed > 5.0 {
            should_pause = true;
        }
    }

    AdaptiveHints {
        suggested_chunk_size: chunk_size,
        suggested_streams: streams,
        should_pause,
    }
}
