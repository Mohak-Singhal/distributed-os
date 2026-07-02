//! Trust On First Use (TOFU) for P2P TLS.
//!
//! On first connection, the peer's certificate fingerprint is stored.
//! On subsequent connections, the fingerprint must match.
//! This prevents MITM after the first exchange.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use sha2::Digest;

/// A stored trust entry for a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustEntry {
    /// Peer identifier (device_id from handshake).
    pub peer_id: String,
    /// SHA-256 fingerprint of the peer's DER certificate.
    pub fingerprint: String,
    /// Human-readable peer name (from handshake).
    pub peer_name: String,
    /// Timestamp of first trust.
    pub trusted_since: u64,
}

/// TOFU trust store — persists trusted peer certificates.
#[derive(Debug)]
pub struct TofuStore {
    entries: HashMap<String, TrustEntry>,
    path: PathBuf,
    last_mtime: Option<std::time::SystemTime>,
}

impl TofuStore {
    /// Load the TOFU store from disk. Creates file if not present.
    pub fn load(state_dir: Option<PathBuf>) -> Self {
        let dir = state_dir.unwrap_or_else(|| PathBuf::from(".transfer_state"));
        let path = dir.join("tofu.json");
        let _ = std::fs::create_dir_all(&dir);

        let (entries, last_mtime) = if path.exists() {
            let mtime = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok());
            let entries = std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<HashMap<String, TrustEntry>>(&s).ok())
                .unwrap_or_default();
            (entries, mtime)
        } else {
            (HashMap::new(), None)
        };

        Self { entries, path, last_mtime }
    }

    /// Reload the store from disk if the file has changed since last load.
    /// Returns `true` if the store was reloaded (i.e. file changed).
    pub fn reload_if_changed(&mut self) -> bool {
        let mtime = std::fs::metadata(&self.path)
            .ok()
            .and_then(|m| m.modified().ok());
        if mtime != self.last_mtime {
            if let Ok(data) = std::fs::read_to_string(&self.path) {
                if let Ok(entries) = serde_json::from_str::<HashMap<String, TrustEntry>>(&data) {
                    self.entries = entries;
                    self.last_mtime = mtime;
                    return true;
                }
            }
        }
        false
    }

    /// Compute SHA-256 fingerprint from a DER certificate.
    pub fn fingerprint(der: &[u8]) -> String {
        let hash = sha2::Sha256::digest(der);
        hash.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// Check if a peer's fingerprint is trusted.
    pub fn is_trusted(&self, peer_id: &str, fingerprint: &str) -> bool {
        match self.entries.get(peer_id) {
            Some(entry) => entry.fingerprint == fingerprint,
            None => false,
        }
    }

    /// Record trust for a peer (first use).
    pub fn trust(&mut self, peer_id: &str, fingerprint: &str, peer_name: &str) {
        let entry = TrustEntry {
            peer_id: peer_id.to_string(),
            fingerprint: fingerprint.to_string(),
            peer_name: peer_name.to_string(),
            trusted_since: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        self.entries.insert(peer_id.to_string(), entry);
        self.save();
    }

    /// Get the stored fingerprint for a peer, if any.
    pub fn fingerprint_for(&self, peer_id: &str) -> Option<&str> {
        self.entries.get(peer_id).map(|e| e.fingerprint.as_str())
    }

    /// Persist the store to disk.
    fn save(&mut self) {
        if let Ok(data) = serde_json::to_string_pretty(&self.entries) {
            let _ = std::fs::write(&self.path, data);
            self.last_mtime = std::fs::metadata(&self.path).ok().and_then(|m| m.modified().ok());
        }
    }
}
