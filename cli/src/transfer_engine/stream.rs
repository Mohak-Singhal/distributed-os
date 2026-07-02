use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

pub struct StreamStats {
    pub active: Arc<AtomicBool>,
    pub bytes_sent: Arc<AtomicU64>,
    pub speed_mbps: Arc<AtomicU64>,
    pub started_at: Instant,
    pub stream_id: u32,
}

pub struct StreamManager {
    pub streams: Vec<StreamStats>,
    pub target_streams: Arc<AtomicU64>,
    pub total_bytes: Arc<AtomicU64>,
}

impl StreamManager {
    pub fn new() -> Self {
        Self {
            streams: Vec::new(),
            target_streams: Arc::new(AtomicU64::new(1)),
            total_bytes: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn adjust_stream_count(&mut self, measured_mbps: f64, expected_mbps: f64, loss_pct: f64) -> u32 {
        let current = self.streams.len() as u32;
        let utilization = if expected_mbps > 0.0 { measured_mbps / expected_mbps } else { 0.0 };

        let target = if utilization < 0.5 && loss_pct > 1.0 {
            // Lossy link and under-utilized → more streams
            (current * 2).min(16)
        } else if utilization < 0.7 && current < 4 {
            // Slightly under-utilized → modest increase
            current + 1
        } else if utilization > 0.95 && current > 1 {
            // Saturated → reduce overhead
            current - 1
        } else {
            current
        };

        self.target_streams.store(target as u64, Ordering::Relaxed);
        target
    }
}
