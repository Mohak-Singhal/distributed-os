use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex, RwLock};

use crate::transport::TransportMode;

/// Tuning configuration — adjusted live by the feedback loop.
#[derive(Debug, Clone)]
pub struct TuningConfig {
    pub chunk_size_bytes: usize,
    pub parallel_streams: usize,
    pub send_buffer_kb: usize,
    pub recv_buffer_kb: usize,
    pub write_batch_size: usize,
    pub worker_threads: usize,
    pub throughput_limit_mbps: Option<f64>,
    pub transport_mode: TransportMode,
}

impl Default for TuningConfig {
    fn default() -> Self {
        Self {
            chunk_size_bytes: 1_048_576,
            parallel_streams: 1,
            send_buffer_kb: 4096,
            recv_buffer_kb: 4096,
            write_batch_size: 4,
            worker_threads: num_cpus::get().max(2),
            throughput_limit_mbps: None,
            transport_mode: TransportMode::TcpBuffered,
        }
    }
}

/// Bottleneck types that the classifier can detect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bottleneck {
    NetworkLatency,
    NetworkBandwidth,
    NetworkCongestion,
    CpuBound,
    MemoryBound,
    DiskIo,
    ThermalThrottling,
    Unknown,
}

/// Shared adaptive state, accessible from any transfer thread.
pub struct AdaptiveState {
    pub samples: Arc<Mutex<VecDeque<serde_json::Value>>>,
    pub current_config: Arc<RwLock<TuningConfig>>,
    pub running: Arc<AtomicBool>,
    pub file_size: u64,
    pub zero_copy_failed: Arc<AtomicBool>,
    pub udp_handshake_failed: Arc<AtomicBool>,
    pub packet_loss_pct: Arc<Mutex<f64>>,
    pub rtt_variance_ms: Arc<Mutex<f64>>,
}

impl AdaptiveState {
    pub fn new(file_size: u64) -> Arc<Self> {
        Arc::new(Self {
            samples: Arc::new(Mutex::new(VecDeque::with_capacity(30))),
            current_config: Arc::new(RwLock::new(TuningConfig::default())),
            running: Arc::new(AtomicBool::new(true)),
            file_size,
            zero_copy_failed: Arc::new(AtomicBool::new(false)),
            udp_handshake_failed: Arc::new(AtomicBool::new(false)),
            packet_loss_pct: Arc::new(Mutex::new(0.0)),
            rtt_variance_ms: Arc::new(Mutex::new(0.0)),
        })
    }

    pub fn config_snapshot(&self) -> TuningConfig {
        self.current_config
            .try_read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

lazy_static::lazy_static! {
    pub static ref ACTIVE_ADAPTIVE_STATE:
        std::sync::RwLock<Option<Arc<AdaptiveState>>> = std::sync::RwLock::new(None);
    pub static ref OVERRIDE_TRANSPORT_MODE:
        std::sync::Mutex<Option<TransportMode>> = std::sync::Mutex::new(None);
}

pub fn register_active_state(state: Arc<AdaptiveState>) {
    if let Ok(mut lock) = ACTIVE_ADAPTIVE_STATE.write() {
        *lock = Some(state);
    }
}

pub fn deregister_active_state() {
    if let Ok(mut lock) = ACTIVE_ADAPTIVE_STATE.write() {
        *lock = None;
    }
}

pub fn get_active_config() -> TuningConfig {
    if let Ok(lock) = ACTIVE_ADAPTIVE_STATE.read() {
        if let Some(ref state) = *lock {
            return state.config_snapshot();
        }
    }
    TuningConfig::default()
}
