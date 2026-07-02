//! UDP Broadcast discovery mechanism for locating the macOS relay on local networks.

use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;
use tracing::{info, warn, error};

/// Default UDP port for discovery.
pub const DISCOVERY_PORT: u16 = 18250;
/// Broadcast message sent by clients to search for a relay.
pub const DISCOVER_MSG: &[u8] = b"PDOS_DISCOVER";
/// Prefix returned by the relay in response to discovery queries.
pub const RELAY_PREFIX: &str = "PDOS_RELAY:";

/// An advertiser that runs on the Relay and responds to discovery packets.
pub struct RelayAdvertiser {
    ws_port: u16,
}

impl RelayAdvertiser {
    /// Create a new `RelayAdvertiser` advertising the specified WebSocket port.
    pub fn new(ws_port: u16) -> Self {
        Self { ws_port }
    }

    /// Run the advertiser loop.
    pub async fn run(&self) -> std::io::Result<()> {
        let addr = SocketAddr::from(([0, 0, 0, 0], DISCOVERY_PORT));
        let socket = UdpSocket::bind(addr).await?;
        info!(port = DISCOVERY_PORT, "UDP discovery advertiser listening");

        let mut buf = [0u8; 1024];
        loop {
            match socket.recv_from(&mut buf).await {
                Ok((len, peer)) => {
                    let msg = &buf[..len];
                    if msg == DISCOVER_MSG {
                        let local_ip = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
                        let reply = format!("{}{}:{}", RELAY_PREFIX, local_ip, self.ws_port);
                        info!(peer = %peer, reply = %reply, "received UDP discovery request, responding");
                        if let Err(e) = socket.send_to(reply.as_bytes(), peer).await {
                            warn!(error = %e, "failed to send UDP response to client");
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "UDP recv error");
                }
            }
        }
    }
}

/// A helper function to determine the local IP by querying the OS routing table.
pub fn get_local_ip() -> Option<String> {
    // Connect to a public IP to determine which local interface is used
    use std::net::UdpSocket as StdUdpSocket;
    if let Ok(socket) = StdUdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(local_addr) = socket.local_addr() {
                return Some(local_addr.ip().to_string());
            }
        }
    }
    None
}

/// Run a client-side scan to discover the relay IP.
/// Broadcasts a discovery request and waits for a response.
pub async fn discover_relay(timeout: Duration) -> Option<String> {
    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(e) => {
            error!(error = %e, "failed to bind UDP socket for discovery");
            return None;
        }
    };
    if let Err(e) = socket.set_broadcast(true) {
        warn!(error = %e, "failed to set broadcast on UDP socket");
    }

    let broadcast_addr: SocketAddr = SocketAddr::from(([255, 255, 255, 255], DISCOVERY_PORT));
    let mut buf = [0u8; 1024];

    let start_time = tokio::time::Instant::now();
    let mut interval = tokio::time::interval(Duration::from_millis(1500));

    loop {
        let elapsed = start_time.elapsed();
        if elapsed >= timeout {
            break;
        }

        tokio::select! {
            _ = interval.tick() => {
                info!("broadcasting UDP discovery request...");
                if let Err(e) = socket.send_to(DISCOVER_MSG, broadcast_addr).await {
                    warn!(error = %e, "failed to send discovery broadcast");
                }
            }
            res = socket.recv_from(&mut buf) => {
                match res {
                    Ok((len, _peer)) => {
                        let resp = String::from_utf8_lossy(&buf[..len]);
                        if resp.starts_with(RELAY_PREFIX) {
                            let url = resp.trim_start_matches(RELAY_PREFIX).to_string();
                            info!(discovered_relay = %url, "discovered relay via UDP!");
                            return Some(url);
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "recv error during discovery");
                    }
                }
            }
        }
    }

    None
}
