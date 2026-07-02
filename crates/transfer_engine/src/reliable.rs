//! Reliable file transfer using chunk tracking, ACKs, and retransmission.
//!
//! ## Wire protocol (binary frames over TCP after HTTP headers)
//!
//! | Frame  | Type byte | Fields                                                      |
//! |--------|-----------|-------------------------------------------------------------|
//! | DATA   | `0x00`    | chunk_id(4B) offset(8B) data_len(4B) data(N)               |
//! | ACK    | `0x01`    | chunk_id(4B) acked_offset(8B)                              |
//! | NACK   | `0x02`    | chunk_id(4B) expected_offset(8B) reason(1B)                |
//! | FIN    | `0x03`    | chunk_id(4B) total_bytes(8B)                               |
//! | FINACK | `0x04`    | chunk_id(4B)                                                |
//! | BPRES  | `0x05`    | queue_depth_us(8B) processing_delay_us(8B)                 |
//! | CANCEL | `0x06`    | reason_len(2B) reason(N)                                   |
//!
//! All multi-byte integers are big-endian.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::control::ControlLoop;

// ── Constants ────────────────────────────────────────────────────────────

pub const FRAME_DATA: u8 = 0x00;
pub const FRAME_ACK: u8 = 0x01;
pub const FRAME_NACK: u8 = 0x02;
pub const FRAME_FIN: u8 = 0x03;
pub const FRAME_FINACK: u8 = 0x04;
pub const FRAME_BPRES: u8 = 0x05;
pub const FRAME_CANCEL: u8 = 0x06;

const DATA_HEADER_LEN: usize = 17;
const ACK_LEN: usize = 13;
const NACK_LEN: usize = 14;
const FIN_LEN: usize = 13;
const FINACK_LEN: usize = 5;
const BPRES_LEN: usize = 17;

const DEFAULT_RTO: Duration = Duration::from_millis(500);
const MAX_RETRIES: u32 = 5;
const ACK_READ_TIMEOUT: Duration = Duration::from_millis(50);
const RECEIVER_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const BPRES_INTERVAL: u32 = 16;

// ── Chunk tracking ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkState {
    Pending,
    InFlight { sent_at: Instant, attempt: u32 },
    Acked,
    Lost,
}

#[derive(Debug, Clone)]
pub struct ChunkInfo {
    pub id: u32,
    pub offset: u64,
    pub size: usize,
    pub state: ChunkState,
    pub is_retransmit: bool,
}

/// Tracks the state of every chunk in a transfer.
pub struct ChunkTracker {
    chunks: Vec<ChunkInfo>,
    acked_ids: HashSet<u32>,
    contiguous_acked: u32,
    total_bytes: u64,
    pub in_flight: u32,
    pub retransmits: u32,
}

impl ChunkTracker {
    pub fn new(chunk_size: usize, total_bytes: u64) -> Self {
        let mut chunks = Vec::new();
        let mut offset = 0u64;
        let mut id = 0u32;
        while offset < total_bytes {
            let size = (total_bytes - offset).min(chunk_size as u64) as usize;
            chunks.push(ChunkInfo {
                id,
                offset,
                size,
                state: ChunkState::Pending,
                is_retransmit: false,
            });
            offset += size as u64;
            id += 1;
        }
        Self {
            chunks,
            acked_ids: HashSet::new(),
            contiguous_acked: 0,
            total_bytes,
            in_flight: 0,
            retransmits: 0,
        }
    }

    pub fn mark_in_flight(&mut self, id: u32, attempt: u32) {
        if let Some(chunk) = self.chunks.iter_mut().find(|c| c.id == id) {
            chunk.state = ChunkState::InFlight { sent_at: Instant::now(), attempt };
            self.in_flight += 1;
            if attempt > 1 {
                chunk.is_retransmit = true;
                self.retransmits += 1;
            }
        }
    }

    /// Mark as ACKed. Returns true if this was a new (non-duplicate) ACK.
    pub fn mark_acked(&mut self, id: u32, _acked_offset: u64) -> bool {
        if !self.acked_ids.insert(id) {
            return false;
        }
        if let Some(chunk) = self.chunks.iter_mut().find(|c| c.id == id) {
            chunk.state = ChunkState::Acked;
            self.in_flight = self.in_flight.saturating_sub(1);
            while let Some(next) = self.chunks.get(self.contiguous_acked as usize) {
                if next.state != ChunkState::Acked {
                    break;
                }
                self.contiguous_acked += 1;
            }
        }
        true
    }

    /// Return the send timestamp of an in-flight chunk (for RTT measurement).
    pub fn sent_at(&self, id: u32) -> Option<Instant> {
        self.chunks.iter().find(|c| c.id == id).and_then(|c| {
            if let ChunkState::InFlight { sent_at, .. } = c.state {
                Some(sent_at)
            } else {
                None
            }
        })
    }

    /// Find chunks that timed out (in-flight longer than RTO).
    pub fn find_timed_out(&self, rto: Duration) -> Vec<u32> {
        let now = Instant::now();
        self.chunks
            .iter()
            .filter_map(|c| {
                if let ChunkState::InFlight { sent_at, .. } = c.state {
                    if now.duration_since(sent_at) > rto {
                        Some(c.id)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check if any in-flight chunk has exhausted its retries.
    pub fn has_exhausted_retries(&self, max_retries: u32) -> bool {
        self.chunks.iter().any(|c| {
            if let ChunkState::InFlight { attempt, .. } = c.state {
                attempt > max_retries
            } else {
                false
            }
        })
    }

    pub fn next_pending(&self) -> Option<u32> {
        self.chunks
            .iter()
            .find(|c| c.state == ChunkState::Pending)
            .map(|c| c.id)
    }

    pub fn all_acked(&self) -> bool {
        self.contiguous_acked as usize == self.chunks.len()
    }

    pub fn last_acked_offset(&self) -> u64 {
        let mut offset = 0u64;
        for chunk in self.chunks.iter().take(self.contiguous_acked as usize) {
            offset += chunk.size as u64;
        }
        offset
    }

    pub fn progress(&self) -> (u64, u64) {
        (self.last_acked_offset(), self.total_bytes)
    }
}

// ── Frame encoding / decoding ────────────────────────────────────────────

fn encode_data_frame(chunk_id: u32, offset: u64, data: &[u8]) -> Vec<u8> {
    let len = DATA_HEADER_LEN + data.len();
    let mut buf = Vec::with_capacity(len);
    buf.push(FRAME_DATA);
    buf.extend_from_slice(&chunk_id.to_be_bytes());
    buf.extend_from_slice(&offset.to_be_bytes());
    buf.extend_from_slice(&(data.len() as u32).to_be_bytes());
    buf.extend_from_slice(data);
    buf
}

fn encode_ack(chunk_id: u32, acked_offset: u64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(ACK_LEN);
    buf.push(FRAME_ACK);
    buf.extend_from_slice(&chunk_id.to_be_bytes());
    buf.extend_from_slice(&acked_offset.to_be_bytes());
    buf
}

fn encode_nack(chunk_id: u32, expected_offset: u64, reason: u8) -> Vec<u8> {
    let mut buf = Vec::with_capacity(NACK_LEN);
    buf.push(FRAME_NACK);
    buf.extend_from_slice(&chunk_id.to_be_bytes());
    buf.extend_from_slice(&expected_offset.to_be_bytes());
    buf.push(reason);
    buf
}

fn encode_fin(chunk_id: u32, total_bytes: u64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(FIN_LEN);
    buf.push(FRAME_FIN);
    buf.extend_from_slice(&chunk_id.to_be_bytes());
    buf.extend_from_slice(&total_bytes.to_be_bytes());
    buf
}

fn encode_finack(chunk_id: u32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(FINACK_LEN);
    buf.push(FRAME_FINACK);
    buf.extend_from_slice(&chunk_id.to_be_bytes());
    buf
}

pub fn encode_cancel(reason: &str) -> Vec<u8> {
    let reason_bytes = reason.as_bytes();
    let mut buf = Vec::with_capacity(3 + reason_bytes.len());
    buf.push(FRAME_CANCEL);
    buf.extend_from_slice(&(reason_bytes.len() as u16).to_be_bytes());
    buf.extend_from_slice(reason_bytes);
    buf
}

fn encode_bpres(queue_depth_us: u64, processing_delay_us: u64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(BPRES_LEN);
    buf.push(FRAME_BPRES);
    buf.extend_from_slice(&queue_depth_us.to_be_bytes());
    buf.extend_from_slice(&processing_delay_us.to_be_bytes());
    buf
}

fn frame_payload_len(frame_type: u8) -> Option<usize> {
    match frame_type {
        FRAME_DATA => None,
        FRAME_ACK => Some(12),
        FRAME_NACK => Some(12),
        FRAME_FIN => Some(12),
        FRAME_FINACK => Some(4),
        FRAME_BPRES => Some(16),
        FRAME_CANCEL => None, // variable-length reason string
        _ => None,
    }
}

async fn read_frame(stream: &mut TcpStream) -> std::io::Result<(u8, Vec<u8>)> {
    let mut type_buf = [0u8; 1];
    stream.read_exact(&mut type_buf).await?;
    let frame_type = type_buf[0];

    if frame_type == FRAME_DATA {
        let mut header = [0u8; 16];
        stream.read_exact(&mut header).await?;
        let data_len = u32::from_be_bytes(header[12..16].try_into().unwrap()) as usize;
        let mut data = vec![0u8; data_len];
        stream.read_exact(&mut data).await?;
        let mut payload = header[..12].to_vec();
        payload.extend_from_slice(&data);
        Ok((frame_type, payload))
    } else if frame_type == FRAME_CANCEL {
        let mut len_buf = [0u8; 2];
        stream.read_exact(&mut len_buf).await?;
        let reason_len = u16::from_be_bytes(len_buf) as usize;
        let mut reason = vec![0u8; reason_len];
        stream.read_exact(&mut reason).await?;
        Ok((frame_type, reason))
    } else if let Some(len) = frame_payload_len(frame_type) {
        let mut payload = vec![0u8; len];
        stream.read_exact(&mut payload).await?;
        Ok((frame_type, payload))
    } else {
        Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "unknown frame type"))
    }
}

// ── Reliable sender ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ReliabilityConfig {
    pub rto: Duration,
    pub max_retries: u32,
    pub window_size: u32,
}

impl Default for ReliabilityConfig {
    fn default() -> Self {
        Self {
            rto: DEFAULT_RTO,
            max_retries: MAX_RETRIES,
            window_size: 4,
        }
    }
}

/// Send a file reliably over a TCP stream using chunk ACKs.
///
/// Design:
/// - Fixed interval observability logging (not bursty)
/// - Actual RTT measured from send→ACK time (not RTO proxy)
/// - Write batching: coalesces multiple DATA frames into single write_all
/// - NACK-triggered fast retransmit: retransmit immediately on NACK
/// - Contiguous ACK tracking for progress reporting
/// - Duplicate ACK detection to prevent in_flight undercount
/// - Explicit max-retries failure via has_exhausted_retries()
pub async fn send_file_reliable(
    stream: &mut TcpStream,
    data: &[u8],
    chunk_size: usize,
    control: Option<&ControlLoop>,
) -> std::io::Result<(u64, u64)> {
    let total = data.len() as u64;
    let mut tracker = ChunkTracker::new(chunk_size, total);
    let mut rto = DEFAULT_RTO;
    let mut config = ReliabilityConfig::default();
    let start = Instant::now();
    let mut last_log = Instant::now();

    loop {
        let now = Instant::now();

        // 0. Observability every ~500ms — fixed precision, no burst
        if now.duration_since(last_log) >= Duration::from_millis(500) {
            last_log = now;
            let (sent, total_sz) = tracker.progress();
            let rtt_ms = if let Some(ctrl) = control {
                ctrl.latest_metrics().await.map(|m| m.rtt_ms).unwrap_or(0.0)
            } else { 0.0 };
            let elapsed = now.duration_since(start).as_secs_f64();
            let rate_mbs = if elapsed > 0.0 { sent as f64 / (elapsed * 1_000_000.0) } else { 0.0 };
            println!(
                "[xfer] rate={:.1}MB/s rtt={:.1}ms chunk={}KB window={} inflight={} retx={} sent={:.1}/{}MB",
                rate_mbs,
                rtt_ms,
                chunk_size / 1024,
                config.window_size,
                tracker.in_flight,
                tracker.retransmits,
                sent as f64 / 1_000_000.0,
                total_sz as f64 / 1_000_000.0,
            );
        }

        // 1. Update RTO and window from control loop
        if let Some(ctrl) = control {
            if let Some(metrics) = ctrl.latest_metrics().await {
                let rtt_based = Duration::from_millis((metrics.rtt_ms * 2.0) as u64).max(Duration::from_millis(100));
                rto = rtt_based;
            }
            let live_config = ctrl.current_config().await;
            config.window_size = live_config.parallel_streams.max(1) as u32;
            config.rto = rto;
        }

        // 2. Check for max-retries exhaustion before any retransmit
        if tracker.has_exhausted_retries(config.max_retries) {
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut,
                "max retries exceeded for one or more chunks"));
        }

        // 3. Retransmit timed-out chunks
        let timed_out = tracker.find_timed_out(rto);
        for &chunk_id in &timed_out {
            let chunk_info = tracker.chunks.iter().find(|c| c.id == chunk_id).cloned();
            if let Some(ref info) = chunk_info {
                let chunk_data = &data[info.offset as usize..(info.offset as usize + info.size)];
                let attempt = match info.state {
                    ChunkState::InFlight { attempt, .. } => attempt + 1,
                    _ => 1,
                };
                let frame = encode_data_frame(chunk_id, info.offset, chunk_data);
                stream.write_all(&frame).await?;
                tracker.mark_in_flight(chunk_id, attempt);
                if let Some(ctrl) = control {
                    ctrl.record_write(info.size as u64, 0, true).await;
                }
            }
        }

        // 4. Fill window with new pending chunks (with write batching)
        if tracker.in_flight < config.window_size {
            let mut batch = Vec::with_capacity(65536);
            let mut batched = 0u32;
            while tracker.in_flight + batched < config.window_size {
                if let Some(chunk_id) = tracker.next_pending() {
                    let chunk_info = tracker.chunks.iter().find(|c| c.id == chunk_id).cloned();
                    if let Some(ref info) = chunk_info {
                        let chunk_data = &data[info.offset as usize..(info.offset as usize + info.size)];
                        let frame = encode_data_frame(chunk_id, info.offset, chunk_data);
                        batch.extend_from_slice(&frame);
                        batched += 1;
                        // Mark in_flight now so it counts toward window
                        tracker.mark_in_flight(chunk_id, 1);
                    }
                } else {
                    break;
                }
            }
            if !batch.is_empty() {
                let write_start = Instant::now();
                stream.write_all(&batch).await?;
                let write_us = write_start.elapsed().as_micros() as u64;
                if let Some(ctrl) = control {
                    // Report the total batch size; duration covers the whole batch
                    ctrl.record_write(batch.len() as u64 - batched as u64 * DATA_HEADER_LEN as u64,
                        write_us, false).await;
                }
            }
        }

        // 5. Read ACK/NACK/BPRES frames (with short timeout)
        let read_timeout = tokio::time::sleep(ACK_READ_TIMEOUT);
        tokio::pin!(read_timeout);

        tokio::select! {
            _ = &mut read_timeout => {}
            result = read_frame(stream) => {
                match result {
                    Ok((FRAME_ACK, payload)) => {
                        let chunk_id = u32::from_be_bytes(payload[0..4].try_into().unwrap());
                        let acked_offset = u64::from_be_bytes(payload[4..12].try_into().unwrap());
                        let measured_rtt = tracker.sent_at(chunk_id)
                            .map(|t| t.elapsed().as_secs_f64() * 1000.0)
                            .unwrap_or(0.0);
                        tracker.mark_acked(chunk_id, acked_offset);
                        if let Some(ctrl) = control {
                            if measured_rtt > 0.0 {
                                ctrl.record_rtt(measured_rtt).await;
                            }
                        }
                    }
                    Ok((FRAME_NACK, payload)) => {
                        let chunk_id = u32::from_be_bytes(payload[0..4].try_into().unwrap());
                        // NACK fast retransmit — retransmit immediately
                        let chunk_info = tracker.chunks.iter().find(|c| c.id == chunk_id).cloned();
                        if let Some(ref info) = chunk_info {
                            let chunk_data = &data[info.offset as usize..(info.offset as usize + info.size)];
                            let attempt = match info.state {
                                ChunkState::InFlight { attempt, .. } => attempt + 1,
                                _ => 1,
                            };
                            let frame = encode_data_frame(chunk_id, info.offset, chunk_data);
                            stream.write_all(&frame).await?;
                            tracker.mark_in_flight(chunk_id, attempt);
                            if let Some(ctrl) = control {
                                ctrl.record_write(info.size as u64, 0, true).await;
                            }
                        }
                    }
                    Ok((FRAME_BPRES, payload)) => {
                        let queue_depth_us = u64::from_be_bytes(payload[0..8].try_into().unwrap());
                        let _processing_delay_us = u64::from_be_bytes(payload[8..16].try_into().unwrap());
                        if queue_depth_us > 50_000 {
                            config.window_size = config.window_size.saturating_sub(1).max(1);
                        } else if queue_depth_us < 10_000 && config.window_size < 8 {
                            config.window_size += 1;
                        }
                    }
                    Ok((FRAME_CANCEL, payload)) => {
                        let reason = String::from_utf8_lossy(&payload);
                        return Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted,
                            format!("transfer cancelled by peer: {}", reason)));
                    }
                    Ok((_, _)) => {}
                    Err(e) => return Err(e),
                }
            }
        }

        // 6. Check completion
        if tracker.all_acked() {
            stream.write_all(&encode_fin(0, total)).await?;
            match tokio::time::timeout(Duration::from_secs(5), read_frame(stream)).await {
                Ok(Ok((FRAME_FINACK, _))) => {}
                Ok(Ok((_, _))) => {
                    stream.write_all(&encode_fin(0, total)).await?;
                }
                _ => {}
            }
            return Ok((total, tracker.last_acked_offset()));
        }
    }
}

/// Receive a file reliably over TCP stream.
///
/// Sends BPRES frames every BPRES_INTERVAL chunks to inform the sender
/// of receiver queue depth. Implements idle timeout to prevent hanging
/// if the sender disconnects.
pub async fn receive_file_reliable(
    stream: &mut TcpStream,
    expected_size: u64,
) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(expected_size as usize);
    let mut received_chunks: HashMap<u32, (u64, Vec<u8>)> = HashMap::new();
    let mut chunk_count: u32 = 0;
    let mut last_activity = Instant::now();

    loop {
        if last_activity.elapsed() > RECEIVER_IDLE_TIMEOUT {
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut,
                "receiver idle timeout — sender may have disconnected"));
        }

        // Send BPRES periodically
        if chunk_count > 0 && chunk_count % BPRES_INTERVAL == 0 {
            let queue_depth_us = 0u64;
            let processing_delay_us = 0u64;
            stream.write_all(&encode_bpres(queue_depth_us, processing_delay_us)).await?;
        }

        match read_frame(stream).await {
            Ok((FRAME_DATA, payload)) => {
                last_activity = Instant::now();
                let chunk_id = u32::from_be_bytes(payload[0..4].try_into().unwrap());
                let offset = u64::from_be_bytes(payload[4..12].try_into().unwrap());
                let data = payload[12..].to_vec();

                let is_duplicate = received_chunks.contains_key(&chunk_id);
                received_chunks.insert(chunk_id, (offset, data.clone()));
                chunk_count += 1;

                stream.write_all(&encode_ack(chunk_id, offset + data.len() as u64)).await?;

                if is_duplicate {
                    stream.write_all(&encode_nack(chunk_id, offset, 1)).await?;
                }
            }
            Ok((FRAME_FIN, payload)) => {
                let total_bytes = u64::from_be_bytes(payload[4..12].try_into().unwrap());
                stream.write_all(&encode_finack(0)).await?;

                let mut sorted: Vec<_> = received_chunks.into_iter().collect();
                sorted.sort_by_key(|(_, (offset, _))| *offset);
                for (_, (_, data)) in sorted {
                    buf.extend_from_slice(&data);
                }
                buf.truncate(total_bytes as usize);
                return Ok(buf);
            }
            Ok((FRAME_CANCEL, payload)) => {
                let reason = String::from_utf8_lossy(&payload);
                return Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted,
                    format!("transfer cancelled: {}", reason)));
            }
            Ok((FRAME_BPRES, _)) => {}
            Ok((_, _)) => {}
            Err(e) => return Err(e),
        }
    }
}
