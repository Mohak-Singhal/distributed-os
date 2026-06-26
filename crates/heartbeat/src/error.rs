//! Error types for the `dos-heartbeat` crate.

use thiserror::Error;

/// Errors from heartbeat operations.
#[derive(Debug, Error)]
pub enum HeartbeatError {
    /// The platform sampler failed to collect one or more metrics.
    #[error("metric sampling failed: {0}")]
    SamplingFailed(String),

    /// The heartbeat could not be sent over the network.
    #[error("send failed: {0}")]
    SendFailed(String),
}
