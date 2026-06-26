//! macOS heartbeat sampler stub.
//!
//! Full implementation (using `sysinfo` or IOKit) comes in Phase 7.

use chrono::Utc;
use dos_core::{NodeStatus, Platform};
use dos_heartbeat::{HeartbeatError, HeartbeatSampler};
use dos_protocol::message::HeartbeatPayload;

/// macOS implementation of [`HeartbeatSampler`].
///
/// Phase 1: returns placeholder zeroed metrics.
/// Phase 7: replaces these with real sysinfo calls.
pub struct MacSampler {
    version: String,
}

impl MacSampler {
    /// Create a sampler for the given agent version string.
    pub fn new(version: impl Into<String>) -> Self {
        Self { version: version.into() }
    }
}

#[async_trait::async_trait]
impl HeartbeatSampler for MacSampler {
    async fn sample(&self) -> Result<HeartbeatPayload, HeartbeatError> {
        // TODO(phase-7): replace with sysinfo / IOKit reads
        Ok(HeartbeatPayload {
            cpu_usage: 0.0,
            memory_usage: 0.0,
            battery_level: None,
            platform: Platform::Mac,
            version: self.version.clone(),
            status: NodeStatus::Online,
            timestamp: Utc::now(),
        })
    }
}
