//! The central [`Node`] model and [`Platform`] enum.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Capability, NodeStatus};

/// The platform (OS / runtime) a node runs on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Platform {
    /// macOS desktop or laptop.
    Mac,
    /// Microsoft Windows.
    Windows,
    /// Linux (bare metal or VM).
    Linux,
    /// Android mobile device.
    Android,
    /// Docker container.
    Docker,
    /// Generic virtual machine.
    Vm,
    /// Browser-based agent.
    Browser,
    /// Network-attached storage appliance.
    Nas,
    /// Raspberry Pi or similar SBC.
    RaspberryPi,
    /// Cloud VM (AWS / GCP / Azure …).
    Cloud,
    /// WSL2 environment.
    Wsl,
    /// A platform not yet recognised by this protocol version.
    Unknown(String),
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Mac => "mac",
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Android => "android",
            Self::Docker => "docker",
            Self::Vm => "vm",
            Self::Browser => "browser",
            Self::Nas => "nas",
            Self::RaspberryPi => "raspberry_pi",
            Self::Cloud => "cloud",
            Self::Wsl => "wsl",
            Self::Unknown(s) => s.as_str(),
        };
        write!(f, "{s}")
    }
}

/// Every endpoint participating in the distributed OS.
///
/// A `Node` record is stored locally and synchronised through the relay.
/// The `id` is derived from the node's ed25519 public key at first boot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// Globally unique identifier derived from the node's public key.
    pub id: Uuid,
    /// Human-readable display name (e.g. "Mohak's MacBook").
    pub name: String,
    /// Underlying OS / runtime.
    pub platform: Platform,
    /// Services this node can provide.
    pub capabilities: Vec<Capability>,
    /// Current presence state.
    pub status: NodeStatus,
    /// Timestamp of the last received heartbeat.
    pub last_seen: Option<DateTime<Utc>>,
    /// Hex-encoded ed25519 public key.
    pub public_key: String,
    /// Protocol version string (semver).
    pub version: String,
}

impl Node {
    /// Construct a new [`Node`].
    ///
    /// `status` defaults to [`NodeStatus::Offline`]; `last_seen` to `None`.
    pub fn new(
        id: Uuid,
        name: impl Into<String>,
        platform: Platform,
        capabilities: Vec<Capability>,
        public_key: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            platform,
            capabilities,
            status: NodeStatus::default(),
            last_seen: None,
            public_key: public_key.into(),
            version: version.into(),
        }
    }

    /// Returns `true` if this node advertises the given capability.
    pub fn has_capability(&self, cap: &Capability) -> bool {
        self.capabilities.contains(cap)
    }

    /// Update `last_seen` and set status to [`NodeStatus::Online`].
    pub fn mark_seen(&mut self, at: DateTime<Utc>) {
        self.last_seen = Some(at);
        self.status = NodeStatus::Online;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn node_has_capability() {
        let node = Node::new(
            Uuid::new_v4(),
            "Test Node",
            Platform::Linux,
            vec![Capability::Compute, Capability::Docker],
            "pubkey",
            "0.1.0",
        );
        assert!(node.has_capability(&Capability::Compute));
        assert!(!node.has_capability(&Capability::Camera));
    }

    #[test]
    fn node_mark_seen_sets_online() {
        let mut node = Node::new(
            Uuid::new_v4(),
            "Test Node",
            Platform::Mac,
            vec![],
            "pubkey",
            "0.1.0",
        );
        assert_eq!(node.status, NodeStatus::Offline);
        node.mark_seen(Utc::now());
        assert_eq!(node.status, NodeStatus::Online);
        assert!(node.last_seen.is_some());
    }
}
