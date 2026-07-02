//! Session management for peer-to-peer file transfers.
//!
//! A `TransferSession` tracks the full lifecycle of a transfer with a peer:
//! discovery → handshake → negotiating → active → completed / failed.
//!
//! `SessionManager` provides thread-safe CRUD and query operations.
//!
//! # Session Protocol
//!
//! Session negotiation happens over a brief TCP exchange before data transfer:
//!
//! ```text
//! Initiator                              Receiver
//!    |------- SessionRequest ------------>|
//!    |<------ SessionAccept / Reject -----|
//!    |------- TransferStart ------------->|
//!    |<======= file data ================>|
//! ```

use crate::{TransferDirection, TransferOptions, TransferProgress};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

// ── Per-File Metadata ────────────────────────────────────────────────

/// Metadata for a single file within a transfer session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    /// Unique file identifier within this session.
    pub file_id: String,
    /// Original filename (not full path).
    pub name: String,
    /// Full source path on the sending device.
    pub source_path: PathBuf,
    /// File size in bytes.
    pub size: u64,
    /// Optional SHA-256 hash for integrity verification.
    pub hash: Option<String>,
    /// Transfer direction for this file.
    pub direction: TransferDirection,
    /// Bytes transferred so far.
    pub bytes_sent: u64,
    /// Whether this file has been fully transferred.
    pub completed: bool,
    /// Error message if this file failed.
    pub error: Option<String>,
}

impl FileMeta {
    pub fn new(name: String, source_path: PathBuf, size: u64, direction: TransferDirection) -> Self {
        Self {
            file_id: format!("{}-{}", name, std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()),
            name,
            source_path,
            size,
            hash: None,
            direction,
            bytes_sent: 0,
            completed: false,
            error: None,
        }
    }

    /// Progress as a float in [0.0, 1.0].
    pub fn progress_fraction(&self) -> f64 {
        if self.size == 0 {
            return 1.0;
        }
        (self.bytes_sent as f64 / self.size as f64).clamp(0.0, 1.0)
    }
}

// ── Aggregated Session Progress ──────────────────────────────────────

/// Aggregated progress across all files in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionProgress {
    /// Session ID this progress belongs to.
    pub session_id: String,
    /// Total bytes across all files.
    pub total_bytes: u64,
    /// Total bytes transferred so far.
    pub bytes_transferred: u64,
    /// Number of files completed.
    pub files_completed: usize,
    /// Total number of files.
    pub files_total: usize,
    /// Throughput in Mbps.
    pub speed_mbps: f64,
    /// Per-file progress details.
    pub files: Vec<FileMeta>,
    /// Estimated time remaining in seconds.
    pub eta_secs: Option<f64>,
}

impl SessionProgress {
    pub fn new(session_id: String, files: Vec<FileMeta>) -> Self {
        let total_bytes: u64 = files.iter().map(|f| f.size).sum();
        let files_total = files.len();
        Self {
            session_id,
            total_bytes,
            bytes_transferred: 0,
            files_completed: 0,
            files_total,
            speed_mbps: 0.0,
            files,
            eta_secs: None,
        }
    }

    /// Recompute aggregated progress from per-file data.
    pub fn recompute(&mut self) {
        self.total_bytes = self.files.iter().map(|f| f.size).sum();
        self.bytes_transferred = self.files.iter().map(|f| f.bytes_sent).sum();
        self.files_completed = self.files.iter().filter(|f| f.completed).count();
        self.files_total = self.files.len();
    }

    /// Overall progress as a float in [0.0, 1.0].
    pub fn overall_fraction(&self) -> f64 {
        if self.total_bytes == 0 {
            return if self.files_total == 0 { 0.0 } else { 1.0 };
        }
        (self.bytes_transferred as f64 / self.total_bytes as f64).clamp(0.0, 1.0)
    }

    /// Update progress for a single file, then recompute aggregate.
    pub fn update_file(&mut self, file_id: &str, bytes_sent: u64, completed: bool) {
        if let Some(file) = self.files.iter_mut().find(|f| f.file_id == file_id) {
            file.bytes_sent = bytes_sent;
            file.completed = completed;
        }
        self.recompute();
    }
}

// ── Session Wire Protocol ────────────────────────────────────────────

/// Magic bytes for session negotiation messages.
pub const SESSION_MAGIC: [u8; 4] = *b"XYNS";

/// Negotiation message types.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMsgType {
    /// Initiator → Receiver: propose a session with file list.
    Request = 0x10,
    /// Receiver → Initiator: accept the session (with resume offsets).
    Accept = 0x11,
    /// Receiver → Initiator: reject the session.
    Reject = 0x12,
    /// Both directions: transfer progress update.
    Progress = 0x13,
    /// Either side: transfer complete.
    Complete = 0x14,
    /// Either side: transfer failed.
    Failed = 0x15,
}

impl SessionMsgType {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x10 => Some(Self::Request),
            0x11 => Some(Self::Accept),
            0x12 => Some(Self::Reject),
            0x13 => Some(Self::Progress),
            0x14 => Some(Self::Complete),
            0x15 => Some(Self::Failed),
            _ => None,
        }
    }
}

/// Wire format header for session negotiation messages.
/// Payload (JSON) follows immediately after these 10 bytes.
#[derive(Debug, Clone)]
pub struct SessionMsgHeader {
    pub msg_type: SessionMsgType,
    pub payload_len: u32,
}

/// Encode a session negotiation message: 10-byte header + JSON payload.
pub fn encode_session_msg(msg_type: SessionMsgType, payload: &[u8]) -> Vec<u8> {
    let payload_len = payload.len() as u32;
    let mut buf = Vec::with_capacity(10 + payload_len as usize);
    buf.extend_from_slice(&SESSION_MAGIC);
    buf.push(0x01); // version
    buf.push(msg_type as u8);
    buf.extend_from_slice(&payload_len.to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}

/// Try to decode a session message header from a buffer.
/// Returns `None` if the buffer is too short or has invalid magic.
pub fn decode_session_header(buf: &[u8]) -> Option<SessionMsgHeader> {
    if buf.len() < 10 {
        return None;
    }
    if buf[0..4] != SESSION_MAGIC {
        return None;
    }
    let msg_type = SessionMsgType::from_u8(buf[5])?;
    let payload_len = u32::from_be_bytes([buf[6], buf[7], buf[8], buf[9]]);
    Some(SessionMsgHeader { msg_type, payload_len })
}

/// Write a framed session negotiation message to a writer.
pub async fn write_session_msg<W>(
    writer: &mut W,
    msg_type: SessionMsgType,
    payload: &[u8],
) -> std::io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin + ?Sized,
{
    use tokio::io::AsyncWriteExt;
    let buf = encode_session_msg(msg_type, payload);
    writer.write_all(&buf).await
}

/// Read a framed session negotiation message from a reader.
pub async fn read_session_msg<R>(
    reader: &mut R,
) -> std::io::Result<(SessionMsgHeader, Vec<u8>)>
where
    R: tokio::io::AsyncRead + Unpin + ?Sized,
{
    use tokio::io::AsyncReadExt;
    let mut header_buf = [0u8; 10];
    reader.read_exact(&mut header_buf).await?;
    let header = decode_session_header(&header_buf)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid session header"))?;
    let mut payload = vec![0u8; header.payload_len as usize];
    reader.read_exact(&mut payload).await?;
    Ok((header, payload))
}

/// Payload for a session request (initiator → receiver).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRequestPayload {
    pub session_id: String,
    pub source_device_id: String,
    pub source_device_name: String,
    pub files: Vec<FileMeta>,
    pub protocol_version: String,
}

/// Payload for a session accept (receiver → initiator).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAcceptPayload {
    pub session_id: String,
    pub target_device_id: String,
    pub target_device_name: String,
    pub accepted_file_ids: Vec<String>,
    pub resume_offsets: HashMap<String, u64>,
}

/// Payload for a session reject (receiver → initiator).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRejectPayload {
    pub session_id: String,
    pub reason: String,
}

/// Each session proceeds through these states exactly once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Initial handshake in progress
    Handshaking,
    /// Capability negotiation (chunk size, parallelism, etc.)
    Negotiating,
    /// Data is actively being transferred
    Active,
    /// Transfer completed successfully
    Completed,
    /// Transfer failed with a terminal error
    Failed,
    /// Cancelled by the user
    Cancelled,
}

impl SessionState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, SessionState::Completed | SessionState::Failed | SessionState::Cancelled)
    }

    pub fn is_active(&self) -> bool {
        matches!(self, SessionState::Handshaking | SessionState::Negotiating | SessionState::Active)
    }
}

/// A single transfer session with a peer.
#[derive(Debug, Clone)]
pub struct TransferSession {
    /// Unique session identifier.
    pub id: String,
    /// The remote peer's node ID.
    pub peer_id: String,
    /// Human-readable peer name.
    pub peer_name: String,
    /// Current lifecycle state.
    pub state: SessionState,
    /// Upload or download.
    pub direction: TransferDirection,
    /// Transfer progress (legacy flat progress).
    pub progress: TransferProgress,
    /// Options negotiated with remote peer.
    pub negotiated_options: TransferOptions,
    /// Remote peer's capability exchange (populated after handshake).
    pub remote_caps_json: Option<String>,
    /// Wall clock creation time.
    pub created_at: Instant,
    /// Last state transition time.
    pub updated_at: Instant,
    /// Human-readable error if state == Failed.
    pub error: Option<String>,
    /// Files being transferred in this session.
    pub files: Vec<FileMeta>,
    /// Aggregated session progress across all files.
    pub session_progress: Option<SessionProgress>,
}

impl TransferSession {
    pub fn new(
        id: String,
        peer_id: String,
        peer_name: String,
        direction: TransferDirection,
        options: TransferOptions,
    ) -> Self {
        Self {
            id,
            peer_id,
            peer_name,
            state: SessionState::Handshaking,
            direction,
            progress: TransferProgress::default(),
            negotiated_options: options,
            remote_caps_json: None,
            created_at: Instant::now(),
            updated_at: Instant::now(),
            error: None,
            files: Vec::new(),
            session_progress: None,
        }
    }

    /// Attach files to this session and initialize aggregated progress.
    pub fn with_files(mut self, files: Vec<FileMeta>) -> Self {
        let total_bytes: u64 = files.iter().map(|f| f.size).sum();
        self.files = files;
        self.progress = TransferProgress {
            bytes_sent: 0,
            bytes_total: total_bytes,
            ..Default::default()
        };
        self
    }

    /// Update progress for a specific file and recompute aggregate.
    pub fn update_file_progress(&mut self, file_id: &str, bytes_sent: u64, completed: bool) {
        if let Some(file) = self.files.iter_mut().find(|f| f.file_id == file_id) {
            file.bytes_sent = bytes_sent;
            file.completed = completed;
        }
        // Recompute aggregate
        let total_bytes: u64 = self.files.iter().map(|f| f.size).sum();
        let bytes_transferred: u64 = self.files.iter().map(|f| f.bytes_sent).sum();
        self.progress = TransferProgress {
            bytes_sent: bytes_transferred,
            bytes_total: total_bytes,
            ..self.progress.clone()
        };

        // Update session_progress if initialized
        if let Some(ref mut sp) = self.session_progress {
            sp.update_file(file_id, bytes_sent, completed);
        }
    }

    /// Returns the overall session progress as a fraction [0.0, 1.0].
    pub fn overall_fraction(&self) -> f64 {
        if self.progress.bytes_total == 0 {
            return 1.0;
        }
        (self.progress.bytes_sent as f64 / self.progress.bytes_total as f64).clamp(0.0, 1.0)
    }

    /// Set a file transfer as failed.
    pub fn fail_file(&mut self, file_id: &str, error: String) {
        if let Some(file) = self.files.iter_mut().find(|f| f.file_id == file_id) {
            file.error = Some(error);
        }
    }

    /// Returns the names of all files in this session.
    pub fn file_names(&self) -> Vec<&str> {
        self.files.iter().map(|f| f.name.as_str()).collect()
    }
}

/// Thread-safe session manager.
///
/// Maintains a map of active (non-terminal) sessions plus a bounded
/// history of completed ones for observability.
pub struct SessionManager {
    active: Arc<RwLock<HashMap<String, TransferSession>>>,
    history: Arc<RwLock<Vec<TransferSession>>>,
    max_history: usize,
}

impl SessionManager {
    pub fn new(max_history: usize) -> Self {
        Self {
            active: Arc::new(RwLock::new(HashMap::new())),
            history: Arc::new(RwLock::new(Vec::with_capacity(max_history))),
            max_history,
        }
    }

    /// Create a new session and insert it into the active map.
    pub async fn create(&self, session: TransferSession) {
        let mut map = self.active.write().await;
        map.insert(session.id.clone(), session);
    }

    /// Get a session by ID (checks active map first, then history).
    pub async fn get(&self, id: &str) -> Option<TransferSession> {
        {
            let map = self.active.read().await;
            if let Some(s) = map.get(id) {
                return Some(s.clone());
            }
        }
        let hist = self.history.read().await;
        hist.iter().find(|s| s.id == id).cloned()
    }

    /// Valid state transitions. Returns `false` if transition is illegal.
    fn valid_transition(from: SessionState, to: SessionState) -> bool {
        match (from, to) {
            (SessionState::Handshaking, SessionState::Negotiating) => true,
            (SessionState::Negotiating, SessionState::Active) => true,
            (SessionState::Active, SessionState::Completed) => true,
            (SessionState::Active, SessionState::Failed) => true,
            (SessionState::Active, SessionState::Cancelled) => true,
            (SessionState::Handshaking, SessionState::Failed) => true,
            (SessionState::Negotiating, SessionState::Failed) => true,
            (SessionState::Handshaking, SessionState::Cancelled) => true,
            (SessionState::Negotiating, SessionState::Cancelled) => true,
            _ => false,
        }
    }

    /// Update session state. If terminal, moves session to history.
    /// Returns `false` if the transition was invalid.
    pub async fn update_state(&self, id: &str, state: SessionState, error: Option<String>) -> bool {
        let mut map = self.active.write().await;
        if let Some(session) = map.get_mut(id) {
            if !Self::valid_transition(session.state, state) {
                tracing::warn!(
                    "Invalid session state transition: {:?} → {:?} for session {}",
                    session.state, state, id,
                );
                return false;
            }
            session.state = state;
            session.updated_at = Instant::now();
            session.error = error;
            if state.is_terminal() {
                let session = session.clone();
                map.remove(id);
                let mut hist = self.history.write().await;
                hist.push(session);
                while hist.len() > self.max_history {
                    hist.remove(0);
                }
            }
            true
        } else {
            false
        }
    }

    /// Update transfer progress for an active session.
    pub async fn update_progress(&self, id: &str, progress: TransferProgress) {
        let mut map = self.active.write().await;
        if let Some(session) = map.get_mut(id) {
            session.progress = progress;
            session.updated_at = Instant::now();
        }
    }

    /// Store remote capabilities after handshake.
    pub async fn set_remote_caps(&self, id: &str, caps_json: String) {
        let mut map = self.active.write().await;
        if let Some(session) = map.get_mut(id) {
            session.remote_caps_json = Some(caps_json);
            session.updated_at = Instant::now();
        }
    }

    /// Return all active (non-terminal) sessions.
    pub async fn list_active(&self) -> Vec<TransferSession> {
        let map = self.active.read().await;
        map.values().cloned().collect()
    }

    /// Return recent completed/failed sessions.
    pub async fn list_history(&self) -> Vec<TransferSession> {
        let hist = self.history.read().await;
        hist.clone()
    }

    /// Return sessions involving a specific peer.
    pub async fn for_peer(&self, peer_id: &str) -> Vec<TransferSession> {
        let mut result = Vec::new();
        {
            let map = self.active.read().await;
            for s in map.values() {
                if s.peer_id == peer_id {
                    result.push(s.clone());
                }
            }
        }
        let hist = self.history.read().await;
        for s in hist.iter() {
            if s.peer_id == peer_id {
                result.push(s.clone());
            }
        }
        result
    }

    /// Update progress for a specific file within a session.
    pub async fn update_file_progress(
        &self,
        session_id: &str,
        file_id: &str,
        bytes_sent: u64,
        completed: bool,
    ) {
        let mut map = self.active.write().await;
        if let Some(session) = map.get_mut(session_id) {
            session.update_file_progress(file_id, bytes_sent, completed);
            session.updated_at = Instant::now();
        }
    }

    /// Attach files to an existing session (e.g., after negotiation).
    pub async fn set_files(&self, session_id: &str, files: Vec<FileMeta>) {
        let mut map = self.active.write().await;
        if let Some(session) = map.get_mut(session_id) {
            let total_bytes: u64 = files.iter().map(|f| f.size).sum();
            session.files = files;
            session.progress = TransferProgress {
                bytes_sent: 0,
                bytes_total: total_bytes,
                ..Default::default()
            };
            session.updated_at = Instant::now();
        }
    }

    /// Get aggregated session progress for a session.
    pub async fn session_progress(&self, id: &str) -> Option<SessionProgress> {
        let map = self.active.read().await;
        map.get(id).map(|s| {
            let mut sp = SessionProgress::new(s.id.clone(), s.files.clone());
            sp.recompute();
            sp
        })
    }
}
