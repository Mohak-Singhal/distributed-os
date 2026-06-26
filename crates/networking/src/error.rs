//! Error types for the `dos-networking` crate.

use thiserror::Error;

/// Errors arising from network transport operations.
#[derive(Debug, Error)]
pub enum NetworkError {
    /// The WebSocket connection was closed unexpectedly.
    #[error("connection closed")]
    ConnectionClosed,

    /// Failed to connect to the remote endpoint.
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    /// A message could not be serialised for sending.
    #[error("serialisation error: {0}")]
    Serialisation(#[from] serde_json::Error),

    /// A received frame could not be deserialised.
    #[error("deserialisation error: {0}")]
    Deserialisation(String),

    /// The underlying WebSocket produced an error.
    #[error("websocket error: {0}")]
    WebSocket(String),

    /// A send or receive timed out.
    #[error("operation timed out")]
    Timeout,
}
