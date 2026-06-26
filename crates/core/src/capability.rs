//! Node capability definitions.

use serde::{Deserialize, Serialize};

/// A capability that a node can advertise to the distributed OS.
///
/// Capabilities drive routing decisions: when a task requires `Capability::Compute`,
/// the task manager dispatches it only to nodes advertising that capability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Capability {
    /// General-purpose computation.
    Compute,
    /// File storage and retrieval.
    FileStorage,
    /// Full-text and structured search.
    Search,
    /// Docker container execution.
    Docker,
    /// AI model inference.
    AiModel,
    /// Web browser automation.
    Browser,
    /// Push notification delivery.
    Notifications,
    /// Camera capture.
    Camera,
    /// Microphone capture.
    Microphone,
    /// Interactive terminal.
    Terminal,
    /// Remote command execution.
    RemoteExecution,
    /// A capability not yet modelled by this protocol version.
    Unknown(String),
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Compute => "compute",
            Self::FileStorage => "file_storage",
            Self::Search => "search",
            Self::Docker => "docker",
            Self::AiModel => "ai_model",
            Self::Browser => "browser",
            Self::Notifications => "notifications",
            Self::Camera => "camera",
            Self::Microphone => "microphone",
            Self::Terminal => "terminal",
            Self::RemoteExecution => "remote_execution",
            Self::Unknown(s) => s.as_str(),
        };
        write!(f, "{s}")
    }
}
