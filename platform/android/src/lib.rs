//! Android-specific platform implementations.
//!
//! JNI bridge and Android system API calls live here.
//! Full implementation deferred to Phase 7/8.

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

pub mod sampler;

pub use sampler::AndroidSampler;
