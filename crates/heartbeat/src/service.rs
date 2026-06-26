//! Heartbeat send loop.

use std::sync::Arc;
use std::time::Duration;

use tracing::{error, info};

use crate::{HeartbeatError, HeartbeatSampler, HEARTBEAT_INTERVAL_SECS};

/// Configuration for the heartbeat service.
#[derive(Debug, Clone)]
pub struct HeartbeatConfig {
    /// How often to send a heartbeat. Defaults to 15 seconds.
    pub interval: Duration,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self { interval: Duration::from_secs(HEARTBEAT_INTERVAL_SECS) }
    }
}

/// Drives the periodic heartbeat send loop.
///
/// Call [`HeartbeatService::run`] inside a dedicated tokio task. It will
/// sample metrics and invoke the provided send callback until the task is
/// cancelled.
pub struct HeartbeatService {
    sampler: Arc<dyn HeartbeatSampler>,
    config: HeartbeatConfig,
}

impl HeartbeatService {
    /// Create a new service with the given sampler and config.
    pub fn new(sampler: Arc<dyn HeartbeatSampler>, config: HeartbeatConfig) -> Self {
        Self { sampler, config }
    }

    /// Run the heartbeat loop, calling `send_fn` with each sampled payload.
    ///
    /// This method loops indefinitely until the task is cancelled (e.g. via a
    /// `CancellationToken`). Sampling errors are logged but do not stop the loop.
    pub async fn run<F, Fut>(&self, send_fn: F)
    where
        F: Fn(dos_protocol::message::HeartbeatPayload) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<(), HeartbeatError>> + Send,
    {
        let mut interval = tokio::time::interval(self.config.interval);
        loop {
            interval.tick().await;
            match self.sampler.sample().await {
                Ok(payload) => {
                    if let Err(e) = send_fn(payload).await {
                        error!(error = %e, "heartbeat send failed");
                    } else {
                        info!("heartbeat sent");
                    }
                }
                Err(e) => {
                    error!(error = %e, "heartbeat sampling failed");
                }
            }
        }
    }
}
