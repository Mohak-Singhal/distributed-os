//! Platform metric sampler trait.

use dos_protocol::message::HeartbeatPayload;

use crate::HeartbeatError;

/// Collects current system metrics for inclusion in a heartbeat.
///
/// Platform crates (`platform/mac`, `platform/linux`, …) implement this trait
/// using OS-specific APIs. The heartbeat service calls `sample()` before each
/// send without knowing which platform it's on.
#[async_trait::async_trait]
pub trait HeartbeatSampler: Send + Sync {
    /// Collect and return the current metrics payload.
    ///
    /// # Errors
    /// Returns [`HeartbeatError::SamplingFailed`] if any metric cannot be read.
    async fn sample(&self) -> Result<HeartbeatPayload, HeartbeatError>;
}
