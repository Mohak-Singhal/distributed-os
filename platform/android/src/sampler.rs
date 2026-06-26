//! Android heartbeat sampler stub.

use chrono::Utc;
use dos_core::{NodeStatus, Platform};
use dos_heartbeat::{HeartbeatError, HeartbeatSampler};
use dos_protocol::message::HeartbeatPayload;

/// Android implementation of [`HeartbeatSampler`].
/// Battery level and real metrics come in Phase 7.
pub struct AndroidSampler { version: String }

impl AndroidSampler {
    /// Create a sampler for the given agent version string.
    pub fn new(version: impl Into<String>) -> Self {
        Self { version: version.into() }
    }
}

#[async_trait::async_trait]
impl HeartbeatSampler for AndroidSampler {
    async fn sample(&self) -> Result<HeartbeatPayload, HeartbeatError> {
        Ok(HeartbeatPayload {
            cpu_usage: 0.0,
            memory_usage: 0.0,
            battery_level: None,
            platform: Platform::Android,
            version: self.version.clone(),
            status: NodeStatus::Online,
            timestamp: Utc::now(),
        })
    }
}
