//! Stable device identity management.
//!
//! Each device gets a persistent UUID and a user-visible name.
//! The identity is stored in a config file and loaded on startup.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Device identity shared across the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIdentity {
    /// Stable, unique device ID (UUID v4, never changes).
    pub device_id: String,
    /// User-visible device name (can be changed).
    pub device_name: String,
    /// Human-readable device model (e.g. "MacBook Pro M4").
    pub device_model: String,
    /// Software version.
    pub software_version: String,
}

impl DeviceIdentity {
    /// Generate a dummy certificate fingerprint for this identity.
    /// In real usage, this would be derived from a TLS certificate.
    pub fn fingerprint(&self) -> String {
        use sha2::Digest;
        let hash = sha2::Sha256::digest(self.device_id.as_bytes());
        hash.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

impl Default for DeviceIdentity {
    fn default() -> Self {
        Self {
            device_id: uuid::Uuid::new_v4().to_string(),
            device_name: whoami::fallible::hostname().unwrap_or_else(|_| "unknown".into()),
            device_model: whoami::platform().to_string(),
            software_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Manager for device identity persistence.
pub struct IdentityManager {
    inner: Arc<RwLock<DeviceIdentity>>,
    config_path: PathBuf,
}

impl IdentityManager {
    /// Create or load the device identity.
    ///
    /// If config_path doesn't exist, generates a new identity and saves it.
    pub fn new(config_dir: Option<PathBuf>) -> Self {
        let dir = config_dir.unwrap_or_else(|| {
            let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
            p.push("xync");
            p
        });
        let config_path = dir.join("identity.json");
        let _ = std::fs::create_dir_all(&dir);

        let identity = if config_path.exists() {
            std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            let id = DeviceIdentity::default();
            if let Ok(data) = serde_json::to_string_pretty(&id) {
                let _ = std::fs::write(&config_path, data);
            }
            id
        };

        Self {
            inner: Arc::new(RwLock::new(identity)),
            config_path,
        }
    }

    /// Get the current device identity.
    pub async fn get(&self) -> DeviceIdentity {
        self.inner.read().await.clone()
    }

    /// Update the device name.
    pub async fn set_name(&self, name: &str) {
        let mut id = self.inner.write().await;
        id.device_name = name.to_string();
        self.persist(&id).await;
    }

    /// Persist identity to disk.
    async fn persist(&self, identity: &DeviceIdentity) {
        if let Ok(data) = serde_json::to_string_pretty(identity) {
            let _ = tokio::fs::write(&self.config_path, data).await;
        }
    }
}
