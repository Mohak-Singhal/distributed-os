//! Error types for the `dos-protocol` crate.

use thiserror::Error;

/// Errors arising from protocol encoding/decoding and validation.
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// JSON serialisation or deserialisation failed.
    #[error("serialisation error: {0}")]
    Serialisation(#[from] serde_json::Error),

    /// The sender is running an incompatible protocol version.
    #[error("protocol version mismatch: expected {expected}, received {received}")]
    VersionMismatch {
        /// The version this node speaks.
        expected: u16,
        /// The version the remote sent.
        received: u16,
    },

    /// An inbound message failed structural validation.
    #[error("validation failed: {0}")]
    ValidationFailed(String),

    /// A message referenced an unknown or unsupported message type.
    #[error("unknown message type: {0}")]
    UnknownMessageType(String),

    /// A required field was absent in the incoming message.
    #[error("missing field: {0}")]
    MissingField(&'static str),

    /// A pair code was invalid or expired.
    #[error("invalid pair code: {0}")]
    InvalidPairCode(String),
}
