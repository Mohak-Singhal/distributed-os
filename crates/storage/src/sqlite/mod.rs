//! SQLite-backed repository implementations and type conversion helpers.
/// Helpers for converting core types to/from SQLite strings.

use dos_core::{Capability, NodeStatus, Platform};

pub(crate) fn platform_from_str(s: &str) -> Platform {
    match s {
        "mac" => Platform::Mac,
        "windows" => Platform::Windows,
        "linux" => Platform::Linux,
        "android" => Platform::Android,
        "docker" => Platform::Docker,
        "vm" => Platform::Vm,
        "browser" => Platform::Browser,
        "nas" => Platform::Nas,
        "raspberry_pi" => Platform::RaspberryPi,
        "cloud" => Platform::Cloud,
        "wsl" => Platform::Wsl,
        other => Platform::Unknown(other.to_string()),
    }
}

pub(crate) fn status_from_str(s: &str) -> NodeStatus {
    match s {
        "online" => NodeStatus::Online,
        "busy" => NodeStatus::Busy,
        "maintenance" => NodeStatus::Maintenance,
        _ => NodeStatus::Offline,
    }
}

/// Capabilities stored as a JSON array of snake_case strings.
pub(crate) fn capabilities_to_json(caps: &[Capability]) -> String {
    let strings: Vec<String> = caps.iter().map(|c| c.to_string()).collect();
    serde_json::to_string(&strings).unwrap_or_else(|_| "[]".into())
}

pub(crate) fn capabilities_from_json(json: &str) -> Vec<Capability> {
    let strings: Vec<String> = serde_json::from_str(json).unwrap_or_default();
    strings
        .into_iter()
        .map(|s| match s.as_str() {
            "compute" => Capability::Compute,
            "file_storage" => Capability::FileStorage,
            "search" => Capability::Search,
            "docker" => Capability::Docker,
            "ai_model" => Capability::AiModel,
            "browser" => Capability::Browser,
            "notifications" => Capability::Notifications,
            "camera" => Capability::Camera,
            "microphone" => Capability::Microphone,
            "terminal" => Capability::Terminal,
            "remote_execution" => Capability::RemoteExecution,
            other => Capability::Unknown(other.to_string()),
        })
        .collect()
}

pub mod node_repo;
pub mod settings_repo;
pub mod task_repo;
