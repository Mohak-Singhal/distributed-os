//! Error types for the `dos-discovery` crate.

use thiserror::Error;

/// Errors arising from device discovery.
#[derive(Debug, Error)]
pub enum DiscoveryError {
    /// mDNS socket could not be bound.
    #[error("mDNS initialisation failed: {0}")]
    MdnsInit(String),

    /// The relay connection was refused or dropped.
    #[error("relay connection error: {0}")]
    RelayConnection(String),

    /// A pair code was malformed or expired.
    #[error("invalid pair code: {0}")]
    InvalidPairCode(String),

    /// The discovery backend was already running.
    #[error("discovery already running")]
    AlreadyRunning,
}
