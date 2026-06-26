//! Linux-specific platform implementations.

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

#[cfg(target_os = "linux")]
pub mod sampler;

#[cfg(target_os = "linux")]
pub use sampler::LinuxSampler;
