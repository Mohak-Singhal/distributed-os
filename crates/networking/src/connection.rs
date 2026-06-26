//! The [`Connection`] trait — an abstract bidirectional message channel.
//!
//! Concrete implementations (WebSocket, in-process pipe for tests) live in
//! separate modules and implement this trait. Higher-level code only ever
//! sees a `dyn Connection`, keeping it transport-agnostic.

use dos_protocol::Message;

use crate::NetworkError;

/// A bidirectional, async message channel.
///
/// Implementors handle framing, reconnection, and backpressure. Callers use
/// only `send` and `recv`.
#[async_trait::async_trait]
pub trait Connection: Send + Sync {
    /// Send a [`Message`] to the remote end.
    ///
    /// # Errors
    /// Returns [`NetworkError`] if the connection is closed or the message
    /// could not be serialised.
    async fn send(&self, message: &Message) -> Result<(), NetworkError>;

    /// Receive the next [`Message`] from the remote end.
    ///
    /// Returns `None` if the connection has been cleanly closed.
    ///
    /// # Errors
    /// Returns [`NetworkError`] on transport or deserialisation failure.
    async fn recv(&self) -> Result<Option<Message>, NetworkError>;

    /// Close the connection gracefully.
    ///
    /// # Errors
    /// Returns [`NetworkError`] if the underlying socket cannot be shut down.
    async fn close(&self) -> Result<(), NetworkError>;

    /// Returns `true` if the connection is currently open.
    fn is_connected(&self) -> bool;
}
