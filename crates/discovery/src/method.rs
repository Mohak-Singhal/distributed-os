//! Discovery trait and supporting types.

use dos_protocol::ids::NodeId;
use dos_core::{Capability, Platform};

use crate::DiscoveryError;

/// The mechanism through which a node was discovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryMethod {
    /// Found via mDNS on the local network.
    Mdns,
    /// Introduced through the relay server.
    Relay,
    /// User-entered pair code or QR scan.
    Manual,
}

/// A node that has been discovered but not yet fully trusted.
#[derive(Debug, Clone)]
pub struct DiscoveredNode {
    /// The discovered node's ID.
    pub node_id: NodeId,
    /// Human-readable name.
    pub name: String,
    /// Platform of the remote node.
    pub platform: Platform,
    /// Advertised capabilities.
    pub capabilities: Vec<Capability>,
    /// How this node was found.
    pub method: DiscoveryMethod,
    /// Network address (IP:port or relay address).
    pub address: String,
}

/// A discovery backend.
///
/// Implementations run continuously in the background, emitting [`DiscoveredNode`]
/// events via the provided callback. Each backend is responsible for its own
/// retry / deduplication logic.
#[async_trait::async_trait]
pub trait Discoverer: Send + Sync {
    /// Start the discovery process.
    ///
    /// The `on_discovered` callback is invoked for each newly found node.
    /// This method returns when the backend is shut down.
    ///
    /// # Errors
    /// Returns [`DiscoveryError`] if the backend cannot be initialised.
    async fn start(
        &self,
        on_discovered: Box<dyn Fn(DiscoveredNode) + Send + Sync>,
    ) -> Result<(), DiscoveryError>;

    /// Gracefully stop the discovery backend.
    async fn stop(&self);
}
