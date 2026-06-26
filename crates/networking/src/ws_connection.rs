//! Concrete WebSocket [`Connection`] implementation using `tokio-tungstenite`.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message as WsMsg;

use dos_protocol::Message;

use crate::{Connection, NetworkError};

type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    WsMsg,
>;

type WsStream = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
>;

/// A WebSocket [`Connection`] wrapping `tokio-tungstenite`.
///
/// Both halves are held behind `Arc<Mutex<_>>` so the connection can be
/// cloned and shared across tasks (e.g. separate send and heartbeat tasks).
pub struct WsConnection {
    sink: Arc<Mutex<WsSink>>,
    stream: Arc<Mutex<WsStream>>,
    connected: Arc<AtomicBool>,
}

impl WsConnection {
    /// Wrap a client-side WebSocket stream.
    pub fn from_client(
        ws: tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> Self {
        let (sink, stream) = ws.split();
        Self {
            sink: Arc::new(Mutex::new(sink)),
            stream: Arc::new(Mutex::new(stream)),
            connected: Arc::new(AtomicBool::new(true)),
        }
    }
}

#[async_trait]
impl Connection for WsConnection {
    async fn send(&self, message: &Message) -> Result<(), NetworkError> {
        let json = serde_json::to_string(message).map_err(NetworkError::Serialisation)?;
        let mut sink = self.sink.lock().await;
        sink.send(WsMsg::Text(json))
            .await
            .map_err(|e| NetworkError::WebSocket(e.to_string()))
    }

    async fn recv(&self) -> Result<Option<Message>, NetworkError> {
        let mut stream = self.stream.lock().await;
        match stream.next().await {
            Some(Ok(WsMsg::Text(text))) => {
                let msg = serde_json::from_str(&text)
                    .map_err(|e| NetworkError::Deserialisation(e.to_string()))?;
                Ok(Some(msg))
            }
            Some(Ok(WsMsg::Close(_))) | None => {
                self.connected.store(false, Ordering::Relaxed);
                Ok(None)
            }
            Some(Ok(_)) => Ok(None), // binary/ping frames — ignore
            Some(Err(e)) => Err(NetworkError::WebSocket(e.to_string())),
        }
    }

    async fn close(&self) -> Result<(), NetworkError> {
        self.connected.store(false, Ordering::Relaxed);
        let mut sink = self.sink.lock().await;
        sink.close()
            .await
            .map_err(|e| NetworkError::WebSocket(e.to_string()))
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}
