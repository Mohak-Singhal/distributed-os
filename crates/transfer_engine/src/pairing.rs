//! Secure pairing and trust management for peer-to-peer transfers.
//!
//! Uses a Trust On First Use (TOFU) model:
//! - First connection stores the peer's certificate fingerprint.
//! - Subsequent connections verify the fingerprint matches.
//! - A pairing request/confirmation flow enables explicit trust establishment.
//!
//! # Flow
//! 1. [`PairingManager::request_pairing`] — initiates pairing with a peer
//! 2. Remote receives [`IncomingPairing`] via event channel
//! 3. User confirms via [`PairingManager::confirm_pairing`] or rejects
//! 4. On confirm: both sides persist each other's fingerprint in TOFU store
//! 5. Future connections verify fingerprint automatically

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::transport::tofu::TofuStore;

/// How long a pending pairing request stays valid before auto-expiry.
const PAIRING_REQUEST_TTL: Duration = Duration::from_secs(120);

/// Pairing status between this device and a remote peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingStatus {
    /// No relationship exists yet.
    Unknown,
    /// Pairing has been requested but not yet confirmed.
    Pending {
        request_id: String,
        is_incoming: bool,
    },
    /// Device is trusted (fingerprint verified).
    Trusted,
    /// Device has been explicitly blocked.
    Blocked,
}

/// An incoming or outgoing pairing request.
#[derive(Debug, Clone)]
pub struct PairingRequest {
    /// Unique request identifier.
    pub request_id: String,
    /// Remote peer's device ID.
    pub device_id: String,
    /// Remote peer's display name.
    pub device_name: String,
    /// Remote peer's certificate fingerprint (SHA-256 of DER).
    pub fingerprint: String,
    /// Whether this request was received from the remote (true) or initiated locally.
    pub is_incoming: bool,
    /// When this request expires.
    pub expires_at: Instant,
    /// Wall clock creation timestamp (epoch seconds).
    pub created_at: u64,
}

impl PairingRequest {
    /// Returns `true` if the pairing request has expired.
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }
}

/// Wire format for sending a pairing request to a remote peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRequestPayload {
    pub request_id: String,
    pub device_id: String,
    pub device_name: String,
    pub fingerprint: String,
    pub created_at: u64,
}

/// Wire format for a pairing confirmation response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingConfirmPayload {
    pub request_id: String,
    pub device_id: String,
    pub device_name: String,
    pub fingerprint: String,
    pub accepted: bool,
}

/// Manages TOFU pairing and trust for discovered peers.
///
/// Thread-safe and designed to be shared via `Arc`.
pub struct PairingManager {
    /// Underlying TOFU store for persistent trust entries.
    tofu_store: Arc<RwLock<TofuStore>>,
    /// Pending (unconfirmed) pairing requests.
    pending: Arc<RwLock<HashMap<String, PairingRequest>>>,
    /// Peers that have been explicitly blocked.
    blocked: Arc<RwLock<Vec<String>>>,
}

impl PairingManager {
    /// Create a new pairing manager backed by the given TOFU store.
    pub fn new(tofu_store: TofuStore) -> Self {
        Self {
            tofu_store: Arc::new(RwLock::new(tofu_store)),
            pending: Arc::new(RwLock::new(HashMap::new())),
            blocked: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create a pairing request to send to a remote peer.
    ///
    /// Returns the [`PairingRequestPayload`] to send over the wire,
    /// and registers the pending request locally.
    pub async fn request_pairing(
        &self,
        device_id: &str,
        device_name: &str,
        our_fingerprint: &str,
    ) -> PairingRequestPayload {
        let request_id = Uuid::new_v4().to_string();
        let now = Instant::now();
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let request = PairingRequest {
            request_id: request_id.clone(),
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            fingerprint: our_fingerprint.to_string(),
            is_incoming: false,
            expires_at: now + PAIRING_REQUEST_TTL,
            created_at,
        };

        self.pending
            .write()
            .await
            .insert(request_id.clone(), request);

        PairingRequestPayload {
            request_id,
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            fingerprint: our_fingerprint.to_string(),
            created_at,
        }
    }

    /// Register an incoming pairing request received from a remote peer.
    ///
    /// Returns the registered [`PairingRequest`] that the user can confirm/reject.
    pub async fn incoming_request(
        &self,
        payload: PairingRequestPayload,
    ) -> PairingRequest {
        let request = PairingRequest {
            request_id: payload.request_id.clone(),
            device_id: payload.device_id,
            device_name: payload.device_name,
            fingerprint: payload.fingerprint,
            is_incoming: true,
            expires_at: Instant::now() + PAIRING_REQUEST_TTL,
            created_at: payload.created_at,
        };

        self.pending
            .write()
            .await
            .insert(request.request_id.clone(), request.clone());

        request
    }

    /// Confirm an incoming pairing request.
    ///
    /// Stores the remote peer's fingerprint in the TOFU store,
    /// removes the pending request, and returns `true` on success.
    /// Returns `false` if the request is expired or not found.
    pub async fn confirm_pairing(&self, request_id: &str) -> bool {
        let mut pending = self.pending.write().await;
        if let Some(request) = pending.remove(request_id) {
            if request.is_expired() {
                return false;
            }
            let mut store = self.tofu_store.write().await;
            store.trust(&request.device_id, &request.fingerprint, &request.device_name);
            true
        } else {
            false
        }
    }

    /// Reject an incoming pairing request (removes it without storing trust).
    pub async fn reject_pairing(&self, request_id: &str) -> bool {
        self.pending.write().await.remove(request_id).is_some()
    }

    /// Process a confirmation received from the remote side.
    ///
    /// Stores their fingerprint (they already trust us, so we reciprocate).
    pub async fn accept_remote_confirmation(&self, payload: PairingConfirmPayload) {
        if payload.accepted {
            let mut store = self.tofu_store.write().await;
            store.trust(&payload.device_id, &payload.fingerprint, &payload.device_name);
        }
        self.pending.write().await.remove(&payload.request_id);
    }

    /// Check the trust status of a peer.
    pub async fn trust_status(&self, device_id: &str) -> PairingStatus {
        // Check blocked list first
        if self.blocked.read().await.iter().any(|id| id == device_id) {
            return PairingStatus::Blocked;
        }

        // Check TOFU store
        {
            let store = self.tofu_store.read().await;
            if store.fingerprint_for(device_id).is_some() {
                return PairingStatus::Trusted;
            }
        }

        // Check pending requests
        {
            let pending = self.pending.read().await;
            for request in pending.values() {
                if request.device_id == device_id {
                    return PairingStatus::Pending {
                        request_id: request.request_id.clone(),
                        is_incoming: request.is_incoming,
                    };
                }
            }
        }

        PairingStatus::Unknown
    }

    /// Returns `true` if the peer is trusted (fingerprint stored).
    pub async fn is_trusted(&self, device_id: &str) -> bool {
        let store = self.tofu_store.read().await;
        store.fingerprint_for(device_id).is_some()
    }

    /// Verify a peer's certificate fingerprint matches our stored trust entry.
    ///
    /// This is the core TOUF check: on reconnect, the fingerprint must match
    /// what we stored on first connection.
    pub async fn verify_fingerprint(&self, device_id: &str, fingerprint: &str) -> bool {
        let store = self.tofu_store.read().await;
        store.is_trusted(device_id, fingerprint)
    }

    /// Get the stored fingerprint for a trusted peer, if any.
    pub async fn fingerprint_for(&self, device_id: &str) -> Option<String> {
        let store = self.tofu_store.read().await;
        store.fingerprint_for(device_id).map(|s| s.to_string())
    }

    /// Explicitly block a peer.
    pub async fn block_peer(&self, device_id: &str) {
        self.blocked.write().await.push(device_id.to_string());
    }

    /// Remove a peer from the blocked list.
    pub async fn unblock_peer(&self, device_id: &str) {
        let mut blocked = self.blocked.write().await;
        blocked.retain(|id| id != device_id);
    }

    /// Remove all expired pending requests. Returns the number removed.
    pub async fn reap_expired(&self) -> usize {
        let mut pending = self.pending.write().await;
        let before = pending.len();
        pending.retain(|_, req| !req.is_expired());
        before - pending.len()
    }

    /// Returns all pending (unconfirmed) pairing requests.
    pub async fn list_pending(&self) -> Vec<PairingRequest> {
        let pending = self.pending.read().await;
        pending.values().cloned().collect()
    }

    /// Access the underlying TOFU store (for persistence operations).
    pub fn tofu_store(&self) -> Arc<RwLock<TofuStore>> {
        self.tofu_store.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> TofuStore {
        let dir = std::env::temp_dir().join(format!("pairing_test_{}", Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        TofuStore::load(Some(dir))
    }

    #[tokio::test]
    async fn test_pairing_request_confirm() {
        let store = test_store();
        let mgr = PairingManager::new(store);

        let payload = mgr.request_pairing("peer-a", "Alice", "abc123").await;
        assert_eq!(payload.device_id, "peer-a");

        // Simulate receiving it on the other side
        let incoming = mgr.incoming_request(payload).await;
        assert_eq!(incoming.device_id, "peer-a");
        assert!(incoming.is_incoming);

        // Confirm pairing
        let confirmed = mgr.confirm_pairing(&incoming.request_id).await;
        assert!(confirmed, "should confirm successfully");

        // Verify trust is established
        assert!(mgr.is_trusted("peer-a").await);
        assert_eq!(mgr.trust_status("peer-a").await, PairingStatus::Trusted);
    }

    #[tokio::test]
    async fn test_pairing_reject() {
        let store = test_store();
        let mgr = PairingManager::new(store);

        let payload = mgr.request_pairing("peer-b", "Bob", "def456").await;
        let incoming = mgr.incoming_request(payload).await;

        let rejected = mgr.reject_pairing(&incoming.request_id).await;
        assert!(rejected, "should reject successfully");

        assert_eq!(mgr.trust_status("peer-b").await, PairingStatus::Unknown);
    }

    #[tokio::test]
    async fn test_pairing_block() {
        let store = test_store();
        let mgr = PairingManager::new(store);

        mgr.block_peer("mallory").await;
        assert_eq!(mgr.trust_status("mallory").await, PairingStatus::Blocked);

        mgr.unblock_peer("mallory").await;
        assert_eq!(mgr.trust_status("mallory").await, PairingStatus::Unknown);
    }

    #[tokio::test]
    async fn test_fingerprint_verification() {
        let store = test_store();
        let mgr = PairingManager::new(store);

        // First trust
        {
            let mut s = mgr.tofu_store.write().await;
            s.trust("peer-c", "fingerprint-x", "Carol");
        }

        // Verify correct fingerprint
        assert!(mgr.verify_fingerprint("peer-c", "fingerprint-x").await);

        // Wrong fingerprint should fail
        assert!(!mgr.verify_fingerprint("peer-c", "fingerprint-y").await);

        // Unknown peer should fail
        assert!(!mgr.verify_fingerprint("unknown", "anything").await);
    }

    #[tokio::test]
    async fn test_incoming_request_passthrough() {
        let store = test_store();
        let mgr = PairingManager::new(store);

        let payload = PairingRequestPayload {
            request_id: "req-1".into(),
            device_id: "peer-d".into(),
            device_name: "Dave".into(),
            fingerprint: "ff1234".into(),
            created_at: 1000,
        };

        let request = mgr.incoming_request(payload).await;
        assert_eq!(request.device_name, "Dave");
        assert!(request.is_incoming);

        // Confirm on the receiving side
        let ok = mgr.confirm_pairing("req-1").await;
        assert!(ok);
        assert!(mgr.is_trusted("peer-d").await);
    }

    #[test]
    fn test_pairing_request_expiry() {
        let request = PairingRequest {
            request_id: "req-x".into(),
            device_id: "peer-x".into(),
            device_name: "Xavier".into(),
            fingerprint: "xx".into(),
            is_incoming: true,
            expires_at: Instant::now() - Duration::from_secs(1),
            created_at: 0,
        };
        assert!(request.is_expired());
    }
}
