//! Heartbeat scheduling and payload construction.
//!
//! Every node sends a [`HeartbeatPayload`] every 15 seconds. This crate
//! provides the [`HeartbeatSampler`] trait for platform-specific metric
//! collection and the [`HeartbeatService`] for driving the send loop.

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

pub mod error;
pub mod sampler;
pub mod service;

pub use error::HeartbeatError;
pub use sampler::HeartbeatSampler;
pub use service::{HeartbeatConfig, HeartbeatService};

/// Default heartbeat interval in seconds.
pub const HEARTBEAT_INTERVAL_SECS: u64 = 15;
