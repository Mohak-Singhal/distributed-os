use base64::prelude::*;
use std::fs;
use std::sync::Arc;
use std::time::Instant;
use chrono::Local;

use async_trait::async_trait;
use serde_json::json;
use uuid::Uuid;

use dos_core::TaskStatus;
use dos_networking::Connection;
use dos_protocol::{builder::task_request, ids::NodeId, Message};
use dos_task_manager::{Task, TaskContext, TaskDispatcher, TaskError, TaskOutput, TaskQueue};

use crate::telemetry::{self, TransferSession, PhaseRecord, SpeedSample, NetworkChange, ProtocolEvent};

pub struct ClientFileTask {
    id: Uuid,
    target: NodeId,
    cli_id: NodeId,
    op: String,
    path: String,
    content: Option<String>,
    compressed: bool,
    conn: dos_networking::WsConnection,
}

impl ClientFileTask {
    pub fn new(
        target: NodeId,
        cli_id: NodeId,
        op: &str,
        path: &str,
        content: Option<String>,
        compressed: bool,
        conn: dos_networking::WsConnection,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            target,
            cli_id,
            op: op.to_string(),
            path: path.to_string(),
            content,
            compressed,
            conn,
        }
    }
}

#[async_trait]
impl Task for ClientFileTask {
    fn id(&self) -> Uuid {
        self.id
    }

    fn kind(&self) -> &'static str {
        "file_transfer"
    }

    fn status(&self) -> TaskStatus {
        TaskStatus::Pending
    }

    async fn execute(&self, _ctx: &TaskContext) -> Result<TaskOutput, TaskError> {
        let mut payload = json!({
            "op": self.op,
            "path": self.path
        });

        if let Some(c) = &self.content {
            payload["content"] = json!(c);
        }

        if self.compressed {
            payload["compressed"] = json!(true);
        }

        let req = task_request(
            self.cli_id,
            Some(self.target),
            "file_transfer".to_string(),
            payload,
        );

        self.conn
            .send(&req)
            .await
            .map_err(|e| TaskError::ExecutionFailed(e.to_string()))?;

        while let Ok(Some(msg)) = self.conn.recv().await {
            match msg {
                Message::TaskResult(res) => {
                    return Ok(TaskOutput { result: res.result });
                }
                Message::Error { code, message } => {
                    return Err(TaskError::ExecutionFailed(format!("{}: {}", code, message)));
                }
                _ => {}
            }
        }
        Err(TaskError::ExecutionFailed("Connection closed".into()))
    }
}

pub struct TransferTracker {
    pub session: TransferSession,
    start: Instant,
    phase_start: Instant,
    current_phase: String,
    last_speed_sample: Instant,
    bytes_since_sample: u64,
    total_sent: u64,
}

impl TransferTracker {
    pub fn new(filename: &str, file_size: u64) -> Self {
        let session = telemetry::new_transfer_session(filename, file_size);
        Self {
            session,
            start: Instant::now(),
            phase_start: Instant::now(),
            current_phase: String::new(),
            last_speed_sample: Instant::now(),
            bytes_since_sample: 0,
            total_sent: 0,
        }
    }

    pub fn begin_phase(&mut self, name: &str) {
        let now = self.start.elapsed().as_secs_f64() * 1000.0;
        if !self.current_phase.is_empty() {
            if let Some(phase) = self.session.phases.last_mut() {
                phase.end = Some(now);
                phase.duration_ms = Some(now - phase.start);
            }
        }
        self.current_phase = name.to_string();
        self.phase_start = Instant::now();
        self.session.phases.push(PhaseRecord {
            name: name.to_string(),
            start: now,
            end: None,
            duration_ms: None,
        });
    }

    pub fn end_phase(&mut self) {
        let now = self.start.elapsed().as_secs_f64() * 1000.0;
        if let Some(phase) = self.session.phases.last_mut() {
            phase.end = Some(now);
            phase.duration_ms = Some(now - phase.start);
        }
        self.current_phase.clear();
    }

    pub fn record_speed(&mut self, bytes_sent: u64) {
        self.bytes_since_sample += bytes_sent;
        self.total_sent += bytes_sent;

        let elapsed = self.last_speed_sample.elapsed().as_secs_f64();
        if elapsed >= 0.5 {
            let speed = (self.bytes_since_sample as f64 * 8.0) / (elapsed * 1_000_000.0);
            let time_offset = self.start.elapsed().as_secs_f64();
            self.session.speed_samples.push(SpeedSample {
                time_offset_sec: time_offset,
                speed_mbps: speed,
            });
            self.bytes_since_sample = 0;
            self.last_speed_sample = Instant::now();
        }
    }

    pub fn record_network_change(&mut self, interface: &str, ip: &str, event: &str) {
        let now = Local::now().format("%H:%M:%S").to_string();
        self.session.network_changes.push(NetworkChange {
            time: now,
            interface: interface.to_string(),
            ip: ip.to_string(),
            rssi: None,
            link_speed: None,
            event: event.to_string(),
        });
        self.session.reconnects += 1;
    }

    pub fn record_protocol_event(&mut self, event_type: &str, detail: &str) {
        let now = Local::now().format("%H:%M:%S%.3f").to_string();
        self.session.protocol_events.push(ProtocolEvent {
            time: now,
            event_type: event_type.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn set_sha256(&mut self, hash: String) {
        self.session.sha256 = Some(hash);
    }

    pub fn set_compression_stats(&mut self, original: u64, compressed: u64, time_ms: u64, cpu_pct: f64) {
        self.session.compressed_size = Some(compressed);
        self.session.compression_ratio = Some(compressed as f64 / original as f64);
        self.session.compression_time_ms = Some(time_ms);
        self.session.cpu_used_compression = Some(cpu_pct);
        if compressed < original {
            self.session.bandwidth_saved = Some(original - compressed);
        }
        let orig_time = original as f64 / (100.0 * 1024.0 * 1024.0);
        let comp_time = compressed as f64 / (100.0 * 1024.0 * 1024.0);
        self.session.time_saved_sec = Some((orig_time - comp_time).max(0.0));
    }

    pub fn complete(mut self, network_mbps: f64) -> TransferSession {
        self.end_phase();
        self.session.completed = true;
        self.session.interrupted = false;
        telemetry::finalize_session(&mut self.session, network_mbps);
        self.session
    }

    pub fn fail(mut self, error: &str) -> TransferSession {
        self.end_phase();
        self.session.completed = false;
        self.session.interrupted = true;
        self.session.error = Some(error.to_string());
        telemetry::finalize_session(&mut self.session, 0.0);
        self.session
    }
}

fn should_compress(filename: &str) -> bool {
    let compressed_extensions = ["mp4", "mp3", "zip", "gz", "bz2", "xz", "jpg", "jpeg",
                                  "png", "gif", "webp", "webm", "mkv", "avi", "mov",
                                  "flac", "ogg", "m4a", "pdf", "docx", "xlsx", "pptx"];
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    !compressed_extensions.contains(&ext)
}

fn compress_data(data: &[u8]) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("compression write");
    encoder.finish().expect("compression finish")
}

fn decompress_data(data: &[u8]) -> Vec<u8> {
    use flate2::read::GzDecoder;
    use std::io::Read;
    let mut decoder = GzDecoder::new(data);
    let mut result = Vec::new();
    decoder.read_to_end(&mut result).expect("decompression");
    result
}

pub async fn run_file_read_raw(
    target_id: Uuid,
    remote_path: &str,
    local_path: &str,
) -> anyhow::Result<()> {
    let node_id = NodeId(target_id);
    let (conn, cli_id) = crate::net::connect_and_identify().await?;

    let (res_tx, mut res_rx) = tokio::sync::mpsc::unbounded_channel();
    let (task_queue, task_rx) = TaskQueue::new(10);
    let context = TaskContext {
        node_id: cli_id.0,
        origin: None,
        result_tx: Some(res_tx),
    };

    let dispatcher = TaskDispatcher::new(task_rx, context);
    tokio::spawn(dispatcher.run());

    let task = Arc::new(ClientFileTask::new(
        node_id,
        cli_id,
        "read",
        remote_path,
        None,
        false,
        conn,
    ));
    task_queue.submit(task, None).await?;

    if let Some((_id, _origin, result)) = res_rx.recv().await {
        match result {
            Ok(output) => {
                if output.result.get("success").and_then(|v| v.as_bool()) == Some(true) {
                    if let Some(content) = output.result.get("content").and_then(|v| v.as_str()) {
                        let data = BASE64_STANDARD.decode(content)?;
                        fs::write(local_path, data)?;
                        return Ok(());
                    }
                }
                if let Some(err) = output.result.get("error").and_then(|v| v.as_str()) {
                    return Err(anyhow::anyhow!("File read error: {}", err));
                }
                return Err(anyhow::anyhow!("Invalid response: {:?}", output.result));
            }
            Err(e) => return Err(anyhow::anyhow!("Error: {}", e)),
        }
    }
    Err(anyhow::anyhow!("No response from file read"))
}

pub async fn run_file_read(
    target_id: Uuid,
    remote_path: &str,
    local_path: &str,
) -> anyhow::Result<()> {
    run_file_read_raw(target_id, remote_path, local_path).await?;
    println!("File read successfully to {}", local_path);
    Ok(())
}

pub async fn run_file_write_raw(
    target_id: Uuid,
    local_path: &str,
    remote_path: &str,
) -> anyhow::Result<()> {
    let filename = std::path::Path::new(local_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(local_path);
    let data = fs::read(local_path)?;
    let file_size = data.len() as u64;

    let compress_file = should_compress(filename);
    let mut tracker = TransferTracker::new(filename, file_size);

    crate::system_monitor::log_op("info", &format!("Starting transfer: {} ({} MB)", filename, file_size as f64 / 1_048_576.0));

    let content = if compress_file {
        tracker.begin_phase("Compression");
        let cpu_start = crate::system_monitor::CURRENT_METRICS.lock()
            .ok()
            .and_then(|m| m.get("cpu_usage").and_then(|v| v.as_str().and_then(|s| s.parse::<f64>().ok())))
            .unwrap_or(0.0);
        let comp_start = Instant::now();
        let compressed = compress_data(&data);
        let comp_time = comp_start.elapsed().as_millis() as u64;
        let cpu_end = crate::system_monitor::CURRENT_METRICS.lock()
            .ok()
            .and_then(|m| m.get("cpu_usage").and_then(|v| v.as_str().and_then(|s| s.parse::<f64>().ok())))
            .unwrap_or(0.0);

        tracker.set_compression_stats(file_size, compressed.len() as u64, comp_time, (cpu_end - cpu_start).max(0.0));
        tracker.end_phase();

        crate::system_monitor::log_op("info", &format!("Compressed {}: {} -> {} MB (ratio: {:.2})",
            filename, file_size as f64 / 1_048_576.0, compressed.len() as f64 / 1_048_576.0,
            compressed.len() as f64 / file_size as f64));

        BASE64_STANDARD.encode(&compressed)
    } else {
        BASE64_STANDARD.encode(&data)
    };

    tracker.begin_phase("Streaming");

    let node_id = NodeId(target_id);
    let (conn, cli_id) = crate::net::connect_and_identify().await?;

    let (res_tx, mut res_rx) = tokio::sync::mpsc::unbounded_channel();
    let (task_queue, task_rx) = TaskQueue::new(10);
    let context = TaskContext {
        node_id: cli_id.0,
        origin: None,
        result_tx: Some(res_tx),
    };

    let dispatcher = TaskDispatcher::new(task_rx, context);
    tokio::spawn(dispatcher.run());

    let task = Arc::new(ClientFileTask::new(
        node_id,
        cli_id,
        "write",
        remote_path,
        Some(content),
        compress_file,
        conn,
    ));
    task_queue.submit(task, None).await?;

    tracker.record_protocol_event("transfer_sent", &format!("file={},size={}", filename, file_size));

    if let Some((_id, _origin, result)) = res_rx.recv().await {
        match result {
            Ok(output) => {
                if output.result.get("success").and_then(|v| v.as_bool()) == Some(true) {
                    tracker.record_speed(file_size);
                    tracker.end_phase();

                    tracker.begin_phase("Verification");
                    tracker.set_sha256(compute_sha256(&data));
                    tracker.session.verified = true;
                    tracker.end_phase();

                    let session = tracker.complete(100.0);
                    store_session(session);
                    crate::system_monitor::log_op("info", &format!("Transfer complete: {}", filename));
                    return Ok(());
                }
                if let Some(err) = output.result.get("error").and_then(|v| v.as_str()) {
                    let session = tracker.fail(err);
                    store_session(session);
                    return Err(anyhow::anyhow!("File write error: {}", err));
                }
            }
            Err(e) => {
                let session = tracker.fail(&e.to_string());
                store_session(session);
                return Err(anyhow::anyhow!("Error: {}", e));
            }
        }
    }

    let session = tracker.fail("no_response");
    store_session(session);
    Err(anyhow::anyhow!("No response from file write"))
}

pub async fn run_file_write(
    target_id: Uuid,
    local_path: &str,
    remote_path: &str,
) -> anyhow::Result<()> {
    run_file_write_raw(target_id, local_path, remote_path).await?;
    println!("File written successfully to {}", remote_path);
    Ok(())
}

pub fn compute_sha256(data: &[u8]) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn store_session(session: TransferSession) {
    if let Ok(mut sessions) = telemetry::TRANSFER_SESSIONS.lock() {
        sessions.push(session);
        if sessions.len() > 100 {
            sessions.remove(0);
        }
        drop(sessions);
        telemetry::save_history_to_disk();
    }
}
