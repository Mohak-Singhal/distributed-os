use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransferCheckpoint {
    pub transfer_id: String,
    pub src_path: String,
    pub dst_path: String,
    pub file_size: u64,
    pub chunk_size: u64,
    pub completed_chunks: Vec<u64>,
    pub sha256_partial: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecoveryStore {
    pub checkpoints: HashMap<String, TransferCheckpoint>,
}

impl RecoveryStore {
    pub fn load() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let path = format!("{}/.pdos/transfer_checkpoints.json", home);
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(store) = serde_json::from_str(&data) {
                return store;
            }
        }
        Self { checkpoints: HashMap::new() }
    }

    pub fn save(&self) {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let dir = format!("{}/.pdos", home);
        std::fs::create_dir_all(&dir).ok();
        let path = format!("{}/transfer_checkpoints.json", dir);
        if let Ok(data) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, data);
        }
    }

    pub fn get_resume_offset(&self, transfer_id: &str) -> u64 {
        self.checkpoints.get(transfer_id)
            .map(|cp| cp.completed_chunks.len() as u64 * cp.chunk_size)
            .unwrap_or(0)
    }

    pub fn record_chunk(&mut self, transfer_id: &str, chunk_index: u64) {
        if let Some(cp) = self.checkpoints.get_mut(transfer_id) {
            if !cp.completed_chunks.contains(&chunk_index) {
                cp.completed_chunks.push(chunk_index);
            }
        }
    }

    pub fn remove(&mut self, transfer_id: &str) {
        self.checkpoints.remove(transfer_id);
    }
}
