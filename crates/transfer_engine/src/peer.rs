//! Peer management for discovered and known devices.
//!
//! `PeerInfo` stores the known state of a remote peer.
//! `PeerManager` provides deduplication, expiry, and query operations.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

/// A known peer on the network.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Unique node identifier.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Network address (IP:port or relay URL).
    pub address: String,
    /// Platform string.
    pub platform: String,
    /// How the peer was discovered (mdns, relay, manual).
    pub method: String,
    /// Timestamp of last discovery or heartbeat.
    pub last_seen: Instant,
    /// Whether this peer has been successfully handshaked.
    pub trusted: bool,
    /// Protocol version for compatibility checks.
    pub version: String,
}

impl PeerInfo {
    /// Create a new peer entry.
    pub fn new(id: String, name: String, address: String) -> Self {
        Self {
            id,
            name,
            address,
            platform: String::new(),
            method: String::new(),
            last_seen: Instant::now(),
            trusted: false,
            version: String::new(),
        }
    }
}

/// Thread-safe peer registry with automatic expiry.
#[derive(Clone)]
pub struct PeerManager {
    peers: Arc<RwLock<HashMap<String, PeerInfo>>>,
    /// Peers not seen within this duration are considered expired.
    expiry: Duration,
}

impl PeerManager {
    /// Create a new peer manager.
    ///
    /// `expiry` controls how long a peer is kept after its last heartbeat.
    pub fn new(expiry: Duration) -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            expiry,
        }
    }

    /// Register or update a peer. Deduplicates by node ID.
    pub async fn register(&self, peer: PeerInfo) {
        let mut map = self.peers.write().await;
        map.insert(peer.id.clone(), peer);
    }

    /// Remove a peer from the registry.
    pub async fn remove(&self, id: &str) {
        let mut map = self.peers.write().await;
        map.remove(id);
    }

    /// Get a single peer by ID.
    pub async fn get(&self, id: &str) -> Option<PeerInfo> {
        let map = self.peers.read().await;
        map.get(id).cloned()
    }

    /// Return all non-expired peers.
    pub async fn list_alive(&self) -> Vec<PeerInfo> {
        let map = self.peers.read().await;
        let now = Instant::now();
        map.values()
            .filter(|p| now.duration_since(p.last_seen) < self.expiry)
            .cloned()
            .collect()
    }

    /// Return all peers (including expired).
    pub async fn list_all(&self) -> Vec<PeerInfo> {
        let map = self.peers.read().await;
        map.values().cloned().collect()
    }

    /// Remove all peers that have expired.
    pub async fn reap_expired(&self) -> usize {
        let mut map = self.peers.write().await;
        let now = Instant::now();
        let before = map.len();
        map.retain(|_, p| now.duration_since(p.last_seen) < self.expiry);
        before - map.len()
    }

    /// Mark a peer as seen (update last_seen).
    pub async fn touch(&self, id: &str) {
        let mut map = self.peers.write().await;
        if let Some(peer) = map.get_mut(id) {
            peer.last_seen = Instant::now();
        }
    }

    /// Mark a peer as trusted (handshake completed).
    pub async fn set_trusted(&self, id: &str, trusted: bool) {
        let mut map = self.peers.write().await;
        if let Some(peer) = map.get_mut(id) {
            peer.trusted = trusted;
        }
    }

    /// Return the number of known peers.
    pub async fn count(&self) -> usize {
        let map = self.peers.read().await;
        map.len()
    }
}
