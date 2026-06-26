//! Windows-specific platform implementations.
//!
//! Compiled only on `target_os = "windows"`.

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

#[cfg(target_os = "windows")]
pub mod sampler;

#[cfg(target_os = "windows")]
pub use sampler::WindowsSampler;
