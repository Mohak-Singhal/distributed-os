use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn, error};

/// Default port for the relay server.
pub const DEFAULT_RELAY_PORT: u16 = 3479;

/// Maximum time to wait for a second peer to join a session.
pub const PEER_TIMEOUT: Duration = Duration::from_secs(30);

/// A connected peer awaiting or participating in a relay session.
#[allow(dead_code)]
struct RelayPeer {
    session_id: String,
    stream: TcpStream,
    addr: SocketAddr,
    connected_at: std::time::Instant,
}

/// Relay server — accepts TCP connections and pairs them by session ID.
///
/// Two peers connecting with the same `session_id` get their byte streams
/// forwarded to each other. The first peer waits up to `PEER_TIMEOUT` for
/// the second peer to arrive.
///
/// This is used as a fallback transport when both peers are behind
/// symmetric NATs and hole punching fails.
pub struct RelayServer {
    sessions: Arc<RwLock<HashMap<String, Arc<Mutex<Option<RelayPeer>>>>>>,
    bind_addr: String,
}

impl RelayServer {
    /// Create a new relay server bound to the given address.
    pub fn new(bind_addr: String) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            bind_addr,
        }
    }

    /// Start the relay server. Runs forever — spawn in a background task.
    pub async fn run(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(&self.bind_addr).await?;
        info!(addr = %self.bind_addr, "relay server started");

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    self.handle_peer(stream, peer_addr).await;
                }
                Err(e) => {
                    error!(error = %e, "relay accept failed");
                }
            }
        }
    }

    /// Handle an incoming peer connection.
    async fn handle_peer(&self, mut stream: TcpStream, peer_addr: SocketAddr) {
        // Read the session ID line
        let mut len_buf = [0u8; 4];
        if stream.read_exact(&mut len_buf).await.is_err() {
            warn!(peer = %peer_addr, "failed to read session ID length");
            return;
        }
        let id_len = u32::from_be_bytes(len_buf) as usize;
        if id_len == 0 || id_len > 256 {
            warn!(peer = %peer_addr, "invalid session ID length: {}", id_len);
            return;
        }
        let mut session_id_buf = vec![0u8; id_len];
        if stream.read_exact(&mut session_id_buf).await.is_err() {
            warn!(peer = %peer_addr, "failed to read session ID");
            return;
        }
        let session_id = String::from_utf8_lossy(&session_id_buf).to_string();

        info!(peer = %peer_addr, %session_id, "peer joining relay session");

        // Look up or create the session slot
        let slot = {
            let mut sessions = self.sessions.write().await;
            sessions
                .entry(session_id.clone())
                .or_insert_with(|| Arc::new(Mutex::new(None)))
                .clone()
        };

        let mut guard = slot.lock().await;
        if guard.is_some() {
            // Second peer: pair them together
            let mut first_peer = guard.take().unwrap();
            info!(%session_id, "pairing two peers");

            // Signal to both that pairing succeeded
            let ok_msg = [0u8; 1]; // single zero byte = OK
            let _ = stream.write_all(&ok_msg).await;
            let _ = first_peer.stream.write_all(&ok_msg).await;

            // Spawn bidirectional forwarding
            let (a_read, a_write) = stream.into_split();
            let (b_read, b_write) = first_peer.stream.into_split();
            tokio::spawn(forward(a_read, b_write, session_id.clone()));
            tokio::spawn(forward(b_read, a_write, session_id));
        } else {
            // First peer: wait for the second
            let peer = RelayPeer {
                session_id: session_id.clone(),
                stream,
                addr: peer_addr,
                connected_at: std::time::Instant::now(),
            };
            *guard = Some(peer);
            drop(guard);

            // Wait for the second peer with a timeout
            tokio::time::sleep(PEER_TIMEOUT).await;

            // After timeout, check if we were paired (slot is now None)
            let guard2 = slot.lock().await;
            if guard2.is_some() {
                warn!(%session_id, "relay session timed out waiting for second peer");
            }
        }
    }
}

/// Forward bytes from `reader` to `writer`, logging the session ID.
async fn forward(
    mut reader: tokio::net::tcp::OwnedReadHalf,
    mut writer: tokio::net::tcp::OwnedWriteHalf,
    session_id: String,
) {
    let mut buf = vec![0u8; 65536];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                if let Err(e) = writer.write_all(&buf[..n]).await {
                    warn!(%session_id, error = %e, "relay forward write failed");
                    break;
                }
            }
            Err(e) => {
                warn!(%session_id, error = %e, "relay forward read failed");
                break;
            }
        }
    }
}

// ── Relay Client ──────────────────────────────────────────────────────────

/// Connect to a relay server to reach a peer by session ID.
///
/// Returns a connected TCP stream that is part of a relay session.
/// The relay pairs this connection with another peer using the same
/// `session_id`.
///
/// Timeouts:
/// - Connect: 10s
/// - Session pairing: 30s (matching server PEER_TIMEOUT)
pub async fn connect_to_relay(
    relay_addr: &str,
    session_id: &str,
) -> anyhow::Result<TcpStream> {
    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        TcpStream::connect(relay_addr),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Relay connect timed out after 10s"))?
    .map_err(|e| anyhow::anyhow!("Relay connect failed: {}", e))?;

    stream.set_nodelay(true)?;

    // Send session ID (length-prefixed) with timeout
    let id_bytes = session_id.as_bytes();
    let len = id_bytes.len() as u32;
    let mut s = stream;
    tokio::time::timeout(Duration::from_secs(5), async {
        s.write_all(&len.to_be_bytes()).await?;
        s.write_all(id_bytes).await?;
        let mut ok = [0u8; 1];
        s.read_exact(&mut ok).await?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|_| anyhow::anyhow!("Relay session pairing timed out after 5s"))?
    .map_err(|e| anyhow::anyhow!("Relay session failed: {}", e))?;

    Ok(s)
}
