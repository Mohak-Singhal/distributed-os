//! Node and system status enumerations.

use serde::{Deserialize, Serialize};

/// The connectivity/presence status of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    /// Reachable and sending heartbeats.
    Online,
    /// No heartbeat received within the expected window.
    #[default]
    Offline,
    /// Reachable but not accepting new tasks (e.g. high load).
    Busy,
    /// Undergoing maintenance or an upgrade.
    Maintenance,
}

impl std::fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Busy => "busy",
            Self::Maintenance => "maintenance",
        };
        write!(f, "{s}")
    }
}
