//! WebSocket transport abstractions.

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

pub mod connection;
pub mod error;
pub mod ws_connection;

pub use connection::Connection;
pub use error::NetworkError;
pub use ws_connection::WsConnection;

/// Connect to a WebSocket server and return a [`WsConnection`].
///
/// # Errors
/// Returns [`NetworkError::ConnectionFailed`] if the handshake fails.
pub async fn connect(url: &str) -> Result<WsConnection, NetworkError> {
    let (ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
    Ok(WsConnection::from_client(ws))
}
