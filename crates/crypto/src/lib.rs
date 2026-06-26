//! ed25519 identity and cryptographic primitives.
//!
//! Each node has a stable [`NodeIdentity`] — an ed25519 signing key pair plus
//! a derived node ID. The identity is persisted locally and used to
//! authenticate all inter-node communication.

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

pub mod error;
pub mod identity;

pub use error::CryptoError;
pub use identity::NodeIdentity;
