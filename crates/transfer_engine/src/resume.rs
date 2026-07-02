//! Persistent resume support for interrupted transfers.
//!
//! Transfer state is stored in `.transfer_state/{file_id}.json`:
//! - SHA-256 hash of the file
//! - Last confirmed offset
//! - Chunk map (optional, for TCP reliable mode)
//! - Timestamp
//!
//! On resume, the sender verifies the file hash matches,
//! then skips to the last offset.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Persistent state for a file transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeState {
    /// Unique file identifier (derived from path + size + hash).
    pub file_id: String,
    /// Absolute path to the source file.
    pub source_path: String,
    /// SHA-256 hash of the complete file.
    pub file_hash: String,
    /// Total file size in bytes.
    pub total_bytes: u64,
    /// Last confirmed contiguous offset.
    pub last_offset: u64,
    /// Chunk-level ACK map (chunk_id → acked), optional.
    pub chunks: Option<HashMap<u32, bool>>,
    /// Transport mode used.
    pub transport: String,
    /// Timestamp of last update.
    pub updated_at: u64,
    /// Whether this transfer completed.
    pub completed: bool,
}

impl ResumeState {
    pub fn new(file_id: String, source_path: &Path, file_hash: String, total_bytes: u64, transport: &str) -> Self {
        Self {
            file_id,
            source_path: source_path.to_string_lossy().to_string(),
            file_hash,
            total_bytes,
            last_offset: 0,
            chunks: None,
            transport: transport.to_string(),
            updated_at: now_secs(),
            completed: false,
        }
    }
}

/// Manager for persistent transfer state on disk.
pub struct ResumeManager {
    /// Directory where state files are stored.
    state_dir: PathBuf,
}

impl ResumeManager {
    /// Create a new resume manager.
    ///
    /// `state_dir` defaults to `.transfer_state/` in the current directory.
    pub fn new(state_dir: Option<PathBuf>) -> Self {
        let dir = state_dir.unwrap_or_else(|| PathBuf::from(".transfer_state"));
        let _ = std::fs::create_dir_all(&dir);
        Self { state_dir: dir }
    }

    /// Path to the state file for a given file_id.
    fn state_path(&self, file_id: &str) -> PathBuf {
        self.state_dir.join(format!("{}.json", sanitize_id(file_id)))
    }

    /// Load transfer state from disk.
    pub fn load(&self, file_id: &str) -> Option<ResumeState> {
        let path = self.state_path(file_id);
        if !path.exists() {
            return None;
        }
        let data = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Save transfer state to disk.
    pub fn save(&self, state: &ResumeState) -> anyhow::Result<()> {
        let path = self.state_path(&state.file_id);
        let data = serde_json::to_string_pretty(state)?;
        std::fs::write(&path, data)?;
        Ok(())
    }

    /// Update offset and save.
    pub fn update_offset(&self, file_id: &str, offset: u64) -> anyhow::Result<()> {
        if let Some(mut state) = self.load(file_id) {
            state.last_offset = offset;
            state.updated_at = now_secs();
            self.save(&state)
        } else {
            Ok(())
        }
    }

    /// Mark transfer as completed and archive the state.
    pub fn mark_completed(&self, file_id: &str) -> anyhow::Result<()> {
        if let Some(mut state) = self.load(file_id) {
            state.completed = true;
            state.updated_at = now_secs();
            self.save(&state)
        } else {
            Ok(())
        }
    }

    /// Remove state file (for cleanup).
    pub fn remove(&self, file_id: &str) {
        let path = self.state_path(file_id);
        let _ = std::fs::remove_file(&path);
    }

    /// List all incomplete transfers.
    pub fn list_incomplete(&self) -> Vec<ResumeState> {
        let mut result = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.state_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(data) = std::fs::read_to_string(&path) {
                        if let Ok(state) = serde_json::from_str::<ResumeState>(&data) {
                            if !state.completed {
                                result.push(state);
                            }
                        }
                    }
                }
            }
        }
        result
    }

    /// Check if a file is eligible for resume.
    pub fn can_resume(&self, file_id: &str, file_hash: &str) -> Option<u64> {
        self.load(file_id).and_then(|state| {
            if state.completed {
                return None;
            }
            if state.file_hash != file_hash {
                return None;
            }
            if state.last_offset >= state.total_bytes {
                return None;
            }
            Some(state.last_offset)
        })
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}
