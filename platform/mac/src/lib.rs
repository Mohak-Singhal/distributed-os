//! macOS-specific platform implementations.
//!
//! This crate implements [`dos_heartbeat::HeartbeatSampler`] using
//! macOS system APIs. It is only compiled on `target_os = "macos"`.

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

#[cfg(target_os = "macos")]
pub mod sampler;

#[cfg(target_os = "macos")]
pub use sampler::MacSampler;
