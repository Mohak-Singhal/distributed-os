//! Windows heartbeat sampler stub.

use chrono::Utc;
use dos_core::{NodeStatus, Platform};
use dos_heartbeat::{HeartbeatError, HeartbeatSampler};
use dos_protocol::message::HeartbeatPayload;

/// Windows implementation of [`HeartbeatSampler`]. Full metrics in Phase 7.
pub struct WindowsSampler { version: String }

impl WindowsSampler {
    /// Create a sampler for the given agent version string.
    pub fn new(version: impl Into<String>) -> Self {
        Self { version: version.into() }
    }
}

#[async_trait::async_trait]
impl HeartbeatSampler for WindowsSampler {
    async fn sample(&self) -> Result<HeartbeatPayload, HeartbeatError> {
        Ok(HeartbeatPayload {
            cpu_usage: 0.0,
            memory_usage: 0.0,
            battery_level: None,
            platform: Platform::Windows,
            version: self.version.clone(),
            status: NodeStatus::Online,
            timestamp: Utc::now(),
        })
    }
}
