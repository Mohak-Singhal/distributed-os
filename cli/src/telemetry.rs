use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use chrono::Local;
use serde::{Serialize, Deserialize};

lazy_static::lazy_static! {
    pub static ref TRANSFER_SESSIONS: Mutex<Vec<TransferSession>> = Mutex::new(Vec::new());
    pub static ref PROTOCOL_COUNTERS: Mutex<ProtocolCounters> = Mutex::new(ProtocolCounters::default());
    pub static ref NETWORK_HISTORY: Mutex<VecDeque<NetworkSnapshot>> = Mutex::new(VecDeque::with_capacity(3600));
    pub static ref THERMAL_STATE: Mutex<ThermalState> = Mutex::new(ThermalState::default());
    pub static ref PROCESS_METRICS: Mutex<ProcessMetrics> = Mutex::new(ProcessMetrics::default());
    pub static ref BUFFER_STATE: Mutex<BufferMetrics> = Mutex::new(BufferMetrics::default());
    pub static ref STORAGE_INFO: Mutex<StorageInfo> = Mutex::new(StorageInfo::default());
    pub static ref TRANSFER_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
}

use std::collections::VecDeque;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransferSession {
    pub id: String,
    pub filename: String,
    pub file_type: String,
    pub file_extension: String,
    pub original_size: u64,
    pub compressed_size: Option<u64>,
    pub compression_ratio: Option<f64>,
    pub compression_time_ms: Option<u64>,
    pub cpu_used_compression: Option<f64>,
    pub bandwidth_saved: Option<u64>,
    pub time_saved_sec: Option<f64>,

    pub start_time: String,
    pub end_time: Option<String>,
    pub duration_secs: Option<f64>,

    pub sha256: Option<String>,

    pub average_speed_mbps: f64,
    pub peak_speed_mbps: f64,
    pub min_speed_mbps: f64,
    pub median_speed_mbps: f64,
    pub p95_speed_mbps: f64,

    pub average_rtt_ms: f64,
    pub peak_rtt_ms: f64,
    pub packet_loss_pct: f64,
    pub retransmissions: u64,
    pub reconnects: u32,

    pub disk_read_mbps: f64,
    pub disk_write_mbps: f64,
    pub disk_queue_depth: f64,
    pub disk_flush_latency_ms: f64,
    pub bytes_buffered: u64,

    pub average_cpu_pct: f64,
    pub peak_cpu_pct: f64,
    pub average_ram_mb: f64,
    pub peak_ram_mb: f64,

    pub completed: bool,
    pub verified: bool,
    pub resumed: bool,
    pub interrupted: bool,
    pub error: Option<String>,

    pub health_score: Option<f64>,
    pub bottleneck: Option<String>,
    pub recommendation: Option<String>,

    pub phases: Vec<PhaseRecord>,
    pub speed_samples: Vec<SpeedSample>,
    pub network_changes: Vec<NetworkChange>,
    pub protocol_events: Vec<ProtocolEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseRecord {
    pub name: String,
    pub start: f64,
    pub end: Option<f64>,
    pub duration_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedSample {
    pub time_offset_sec: f64,
    pub speed_mbps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkChange {
    pub time: String,
    pub interface: String,
    pub ip: String,
    pub rssi: Option<f64>,
    pub link_speed: Option<f64>,
    pub event: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolEvent {
    pub time: String,
    pub event_type: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProtocolCounters {
    pub discovery_packets: u64,
    pub auth_requests: u64,
    pub transfer_requests: u64,
    pub resume_requests: u64,
    pub cancelled_transfers: u64,
    pub completed_transfers: u64,
    pub failed_transfers: u64,
    pub tls_handshakes: u64,
    pub range_requests: u64,
    pub heartbeat_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSnapshot {
    pub time: String,
    pub interface: String,
    pub ip: String,
    pub rssi: Option<f64>,
    pub link_speed: Option<f64>,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub signal_event: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThermalState {
    pub cpu_temp_c: Option<f64>,
    pub thermal_state: String,
    pub fan_rpm: Option<f64>,
    pub battery_pct: Option<f64>,
    pub battery_temp_c: Option<f64>,
    pub thermal_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProcessMetrics {
    pub rust_daemon_cpu: f64,
    pub rust_daemon_ram: f64,
    pub compression_thread_cpu: f64,
    pub compression_thread_ram: f64,
    pub tls_thread_cpu: f64,
    pub tls_thread_ram: f64,
    pub hash_thread_cpu: f64,
    pub hash_thread_ram: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BufferMetrics {
    pub read_buffer_kb: u64,
    pub write_buffer_kb: u64,
    pub average_queue_depth: f64,
    pub max_queue_depth: u64,
    pub backpressure_events: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageInfo {
    pub total_gb: f64,
    pub free_gb: f64,
    pub file_size_gb: f64,
    pub remaining_gb: f64,
    pub enough_space: bool,
}

pub fn new_transfer_session(filename: &str, size: u64) -> TransferSession {
    let now = Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string();
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();
    let file_type = ext.clone();
    let id = format!("txn_{}", TRANSFER_ID_COUNTER.fetch_add(1, Ordering::SeqCst));

    TransferSession {
        id,
        filename: filename.to_string(),
        file_type,
        file_extension: ext,
        original_size: size,
        compressed_size: None,
        compression_ratio: None,
        compression_time_ms: None,
        cpu_used_compression: None,
        bandwidth_saved: None,
        time_saved_sec: None,
        start_time: now,
        end_time: None,
        duration_secs: None,
        sha256: None,
        average_speed_mbps: 0.0,
        peak_speed_mbps: 0.0,
        min_speed_mbps: f64::MAX,
        median_speed_mbps: 0.0,
        p95_speed_mbps: 0.0,
        average_rtt_ms: 0.0,
        peak_rtt_ms: 0.0,
        packet_loss_pct: 0.0,
        retransmissions: 0,
        reconnects: 0,
        disk_read_mbps: 0.0,
        disk_write_mbps: 0.0,
        disk_queue_depth: 0.0,
        disk_flush_latency_ms: 0.0,
        bytes_buffered: 0,
        average_cpu_pct: 0.0,
        peak_cpu_pct: 0.0,
        average_ram_mb: 0.0,
        peak_ram_mb: 0.0,
        completed: false,
        verified: false,
        resumed: false,
        interrupted: false,
        error: None,
        health_score: None,
        bottleneck: None,
        recommendation: None,
        phases: Vec::new(),
        speed_samples: Vec::new(),
        network_changes: Vec::new(),
        protocol_events: Vec::new(),
    }
}

pub fn compute_health_score(session: &TransferSession) -> f64 {
    let mut score = 100.0;

    if session.interrupted { score -= 20.0; }
    if session.error.is_some() { score -= 15.0; }
    if session.reconnects > 0 { score -= (session.reconnects as f64) * 5.0; }
    if session.resumed { score -= 5.0; }
    if session.packet_loss_pct > 1.0 { score -= (session.packet_loss_pct * 2.0).min(15.0); }
    if session.average_speed_mbps < 1.0 { score -= 10.0; }
    if session.peak_cpu_pct > 90.0 { score -= 5.0; }

    score.max(0.0).min(100.0)
}

pub fn detect_bottleneck(session: &TransferSession, network_mbps: f64) -> (String, String) {
    let disk_mbps = session.disk_write_mbps.max(session.disk_read_mbps);
    let cpu_pct = session.peak_cpu_pct;

    if cpu_pct > 90.0 && disk_mbps < network_mbps * 0.5 {
        ("CPU".to_string(), "High CPU usage is limiting throughput.".to_string())
    } else if disk_mbps < network_mbps * 0.3 && disk_mbps > 0.0 {
        ("Receiver Disk".to_string(), "Receiver storage is limiting throughput.".to_string())
    } else if session.packet_loss_pct > 2.0 {
        ("Network".to_string(), "High packet loss is degrading performance.".to_string())
    } else if session.retransmissions > 100 {
        ("Network".to_string(), "Excessive retransmissions detected.".to_string())
    } else if cpu_pct > 80.0 {
        ("CPU".to_string(), "Disable compression for this file type.".to_string())
    } else {
        ("Network".to_string(), "Network bandwidth is the limiting factor.".to_string())
    }
}

pub fn get_storage_info(path: &str) -> StorageInfo {
    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    let output = std::process::Command::new("df")
        .arg("-k")
        .arg(path)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok());

    if let Some(out) = output {
        for line in out.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let total_blocks: f64 = parts[1].parse().unwrap_or(0.0);
                let avail_blocks: f64 = parts[3].parse().unwrap_or(0.0);
                let total = total_blocks * 1024.0;
                let free = avail_blocks * 1024.0;
                return StorageInfo {
                    total_gb: total / 1e9,
                    free_gb: free / 1e9,
                    file_size_gb: file_size as f64 / 1e9,
                    remaining_gb: (free - file_size as f64) / 1e9,
                    enough_space: free > file_size as f64,
                };
            }
        }
    }
    StorageInfo {
        file_size_gb: file_size as f64 / 1e9,
        ..Default::default()
    }
}

pub fn finalize_session(session: &mut TransferSession, network_mbps: f64) {
    session.end_time = Some(Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string());
    if let Some(start) = chrono::DateTime::parse_from_rfc3339(&session.start_time).ok() {
        let end = Local::now();
        let dur = end.signed_duration_since(start.with_timezone(&Local));
        session.duration_secs = Some(dur.num_milliseconds() as f64 / 1000.0);
    }

    if !session.speed_samples.is_empty() {
        let speeds: Vec<f64> = session.speed_samples.iter().map(|s| s.speed_mbps).collect();
        session.average_speed_mbps = speeds.iter().sum::<f64>() / speeds.len() as f64;
        session.peak_speed_mbps = speeds.iter().cloned().fold(0.0_f64, f64::max);
        session.min_speed_mbps = speeds.iter().cloned().fold(f64::MAX, f64::min);
        let mut sorted = speeds.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let len = sorted.len();
        session.median_speed_mbps = sorted[len / 2];
        session.p95_speed_mbps = sorted[(len as f64 * 0.95) as usize].min(sorted[len - 1]);
    }

    if let Some(comp_size) = session.compressed_size {
        if session.original_size > 0 && comp_size < session.original_size {
            session.compression_ratio = Some(comp_size as f64 / session.original_size as f64);
            session.bandwidth_saved = Some(session.original_size - comp_size);
        }
    }

    let (bottleneck, recommendation) = detect_bottleneck(session, network_mbps);
    session.bottleneck = Some(bottleneck);
    session.recommendation = Some(recommendation);

    session.health_score = Some(compute_health_score(session));
}

pub fn save_history_to_disk() {
    let path = get_history_path();
    if let Some(dir) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(sessions) = TRANSFER_SESSIONS.lock() {
        let json = serde_json::to_string_pretty(&*sessions).unwrap_or_default();
        let _ = std::fs::write(&path, json);
    }
}

pub fn load_history_from_disk() {
    let path = get_history_path();
    if let Ok(json) = std::fs::read_to_string(&path) {
        if let Ok(sessions) = serde_json::from_str::<Vec<TransferSession>>(&json) {
            if let Ok(mut locked) = TRANSFER_SESSIONS.lock() {
                *locked = sessions;
            }
        }
    }
}

fn get_history_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    format!("{}/.pdos/transfer_history.json", home)
}

pub fn export_session_json(session: &TransferSession) -> serde_json::Value {
    serde_json::json!({
        "transfer_summary": {
            "transfer_id": session.id,
            "start_time": session.start_time,
            "end_time": session.end_time,
            "duration_secs": session.duration_secs,
        },
        "file": {
            "name": session.filename,
            "type": session.file_type,
            "extension": session.file_extension,
            "sha256": session.sha256,
            "original_size": session.original_size,
            "compressed_size": session.compressed_size,
            "compression_ratio": session.compression_ratio,
        },
        "transfer": {
            "average_speed_mbps": session.average_speed_mbps,
            "peak_speed_mbps": session.peak_speed_mbps,
            "min_speed_mbps": session.min_speed_mbps,
            "median_speed_mbps": session.median_speed_mbps,
            "p95_speed_mbps": session.p95_speed_mbps,
        },
        "network": {
            "average_rtt_ms": session.average_rtt_ms,
            "peak_rtt_ms": session.peak_rtt_ms,
            "packet_loss_pct": session.packet_loss_pct,
            "retransmissions": session.retransmissions,
            "reconnects": session.reconnects,
        },
        "resources": {
            "average_cpu_pct": session.average_cpu_pct,
            "peak_cpu_pct": session.peak_cpu_pct,
            "average_ram_mb": session.average_ram_mb,
            "peak_ram_mb": session.peak_ram_mb,
            "disk_read_mbps": session.disk_read_mbps,
            "disk_write_mbps": session.disk_write_mbps,
        },
        "result": {
            "completed": session.completed,
            "verified": session.verified,
            "resumed": session.resumed,
            "interrupted": session.interrupted,
            "error": session.error,
        },
        "health": {
            "health_score": session.health_score,
            "bottleneck": session.bottleneck,
            "recommendation": session.recommendation,
        },
        "waterfall": session.phases.iter().map(|p| serde_json::json!({
            "name": p.name,
            "start_ms": p.start,
            "end_ms": p.end,
            "duration_ms": p.duration_ms,
        })).collect::<Vec<_>>(),
        "speed_samples": session.speed_samples.iter().map(|s| serde_json::json!({
            "time_offset_sec": s.time_offset_sec,
            "speed_mbps": s.speed_mbps,
        })).collect::<Vec<_>>(),
        "network_changes": session.network_changes.iter().map(|n| serde_json::json!({
            "time": n.time,
            "interface": n.interface,
            "ip": n.ip,
            "rssi": n.rssi,
            "link_speed": n.link_speed,
            "event": n.event,
        })).collect::<Vec<_>>(),
    })
}
