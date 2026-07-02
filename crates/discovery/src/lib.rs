//! Device discovery abstractions.
//!
//! Three discovery mechanisms are supported in v0.1:
//! - **Local**: mDNS/Bonjour (same LAN)
//! - **Remote**: relay server (different networks)
//! - **Manual**: pair code / QR code
//!
//! All three implement the [`Discoverer`] trait so the agent can compose them.

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

pub mod error;
pub mod method;
pub mod mdns;
pub mod udp;

pub use error::DiscoveryError;
pub use method::{DiscoveredNode, Discoverer, DiscoveryMethod};

