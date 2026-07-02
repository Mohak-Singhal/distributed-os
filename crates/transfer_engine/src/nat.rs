//! NAT traversal for P2P connectivity.
//!
//! Phase 1: STUN client — detect public IP and NAT type.
//! Phase 2: UDP hole punching — exchange public endpoints and
//!          send simultaneous UDP packets to open NAT mappings.
//! Fallback: Relay server (simple TCP forwarder).

use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use tokio::net::TcpStream;
use tokio::io::AsyncBufReadExt;

use crate::CancelToken;

/// Result of a STUN query.
#[derive(Debug, Clone)]
pub struct StunResult {
    /// Public IP:port as seen by the STUN server.
    pub public_addr: SocketAddr,
    /// Inferred NAT type.
    pub nat_type: NatType,
    /// Round-trip time to the STUN server.
    pub rtt_ms: f64,
}

/// Inferred NAT type based on STUN behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatType {
    /// No NAT — direct public IP.
    None,
    /// Full-cone NAT: any external host can send to the mapped address.
    FullCone,
    /// Restricted cone: only hosts we've sent to can send back.
    RestrictedCone,
    /// Port-restricted cone: only the IP:port we sent to can send back.
    PortRestrictedCone,
    /// Symmetric NAT: each destination gets a different mapped port.
    Symmetric,
    /// Unknown / unable to determine.
    Unknown,
}

/// Default STUN servers.
pub const STUN_SERVERS: &[&str] = &[
    "stun.l.google.com:19302",
    "stun1.l.google.com:19302",
    "stun.ekiga.net:3478",
];

/// Query a STUN server to discover our public IP and port.
///
/// Implements the STUN binding request (RFC 5389) over UDP.
/// Returns the mapped address from the STUN response.
pub fn stun_query(server: &str, timeout: Duration) -> anyhow::Result<StunResult> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(timeout))?;
    socket.set_write_timeout(Some(timeout))?;

    let remote: SocketAddr = server.parse()?;
    let start = std::time::Instant::now();

    // Build a STUN binding request
    // RFC 5389: Binding request is a 20-byte header (no attributes)
    // Transaction ID must be random (12 bytes)
    let mut tx_id = [0u8; 12];
    getrandom::getrandom(&mut tx_id).map_err(|e| anyhow::anyhow!("getrandom failed: {:?}", e))?;

    let request = build_stun_request(&tx_id);
    socket.send_to(&request, remote)?;

    // Read response
    let mut buf = [0u8; 512];
    let (n, _) = socket.recv_from(&mut buf)?;
    let rtt = start.elapsed().as_secs_f64() * 1000.0;

    let public_addr = parse_stun_response(&buf[..n], &tx_id)?;
    let nat_type = detect_nat_type(&socket, remote, public_addr, timeout)?;

    Ok(StunResult {
        public_addr,
        nat_type,
        rtt_ms: rtt,
    })
}

/// Build a STUN binding request (RFC 5389).
pub fn build_stun_request(tx_id: &[u8; 12]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(20);
    // Type: Binding Request (0x0001)
    buf.extend_from_slice(&0x0001u16.to_be_bytes());
    // Message Length: 0 (no attributes)
    buf.extend_from_slice(&0x0000u16.to_be_bytes());
    // Magic Cookie: 0x2112A442
    buf.extend_from_slice(&0x2112A442u32.to_be_bytes());
    // Transaction ID (12 bytes)
    buf.extend_from_slice(tx_id);
    buf
}

/// Parse a STUN response to extract the mapped address.
pub fn parse_stun_response(data: &[u8], tx_id: &[u8; 12]) -> anyhow::Result<SocketAddr> {
    if data.len() < 20 {
        anyhow::bail!("STUN response too short: {} bytes", data.len());
    }

    // Verify magic cookie
    let magic = u32::from_be_bytes(data[4..8].try_into()?);
    if magic != 0x2112A442 {
        anyhow::bail!("invalid STUN magic cookie");
    }

    // Verify transaction ID
    if &data[8..20] != tx_id.as_slice() {
        anyhow::bail!("STUN transaction ID mismatch");
    }

    // Parse attributes (start at offset 20)
    let mut offset = 20;
    while offset + 4 <= data.len() {
        let attr_type = u16::from_be_bytes(data[offset..offset + 2].try_into()?);
        let attr_len = u16::from_be_bytes(data[offset + 2..offset + 4].try_into()?) as usize;
        offset += 4;

        if offset + attr_len > data.len() {
            break;
        }

        // XOR-MAPPED-ADDRESS (0x0020)
        if attr_type == 0x0020 && attr_len >= 8 {
            let family = data[offset + 1];
            let port = u16::from_be_bytes(data[offset + 2..offset + 4].try_into()?) ^ 0x2112;
            match family {
                0x01 => {
                    // IPv4
                    let ip_bytes = &data[offset + 4..offset + 8];
                    let ip = std::net::Ipv4Addr::new(
                        ip_bytes[0] ^ 0x21,
                        ip_bytes[1] ^ 0x12,
                        ip_bytes[2] ^ 0xA4,
                        ip_bytes[3] ^ 0x42,
                    );
                    return Ok(SocketAddr::new(std::net::IpAddr::V4(ip), port));
                }
                0x02 => {
                    // IPv6
                    let ip_bytes = &data[offset + 4..offset + 20];
                    let mut xor_ip = [0u8; 16];
                    let magic_bytes = 0x2112A442u32.to_be_bytes();
                    for i in 0..16 {
                        xor_ip[i] = if i < 4 {
                            ip_bytes[i] ^ magic_bytes[i]
                        } else {
                            ip_bytes[i] ^ tx_id[i - 4]
                        };
                    }
                    let ip = std::net::Ipv6Addr::from(xor_ip);
                    return Ok(SocketAddr::new(std::net::IpAddr::V6(ip), port));
                }
                _ => {}
            }
        }

        offset += attr_len;
        // Align to 4 bytes
        if attr_len % 4 != 0 {
            offset += 4 - (attr_len % 4);
        }
    }

    anyhow::bail!("no XOR-MAPPED-ADDRESS in STUN response")
}

/// Detect NAT type by performing additional STUN queries.
///
/// Uses a second socket to test if the NAT is truly symmetric or just
/// address-restricted. This is a simplified detection — a full RFC 3489
/// test requires multiple sockets and servers.
fn detect_nat_type(
    _socket: &UdpSocket,
    _server: SocketAddr,
    _public_addr: SocketAddr,
    timeout: Duration,
) -> anyhow::Result<NatType> {
    // Try connecting to a second STUN server on a different port
    // If the mapped address is the same, it's cone NAT
    // If different, it's symmetric

    let second_server: SocketAddr = STUN_SERVERS.get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(_server);

    // Send a binding request from a different source port
    let second_socket = UdpSocket::bind("0.0.0.0:0")?;
    second_socket.set_read_timeout(Some(timeout))?;

    let mut tx_id2 = [0u8; 12];
    getrandom::getrandom(&mut tx_id2).map_err(|e| anyhow::anyhow!("getrandom failed: {:?}", e))?;
    let req2 = build_stun_request(&tx_id2);
    second_socket.send_to(&req2, second_server)?;

    let mut buf2 = [0u8; 512];
    match second_socket.recv_from(&mut buf2) {
        Ok((n, _)) => {
            if let Ok(addr2) = parse_stun_response(&buf2[..n], &tx_id2) {
                // If the mapped port changed, it's symmetric
                if addr2.port() != _public_addr.port() {
                    return Ok(NatType::Symmetric);
                }
                if addr2.ip() != _public_addr.ip() {
                    return Ok(NatType::Symmetric);
                }
            }
        }
        Err(_) => {}
    }

    Ok(NatType::Unknown)
}

/// Result of NAT traversal attempt.
#[derive(Debug, Clone)]
pub struct NatTraversalResult {
    /// Whether traversal succeeded.
    pub success: bool,
    /// The address to use for the peer connection.
    pub mapped_addr: Option<SocketAddr>,
    /// NAT type detected.
    pub nat_type: NatType,
    /// Recommended transport mode.
    pub recommended_transport: &'static str,
}

/// Exchange public endpoints with a peer via a TCP signaling connection.
///
/// Both peers connect to each other's signaling port. Each sends their
/// STUN-discovered public endpoint. Returns the peer's public endpoint.
pub async fn exchange_endpoints(
    peer_addr: &str,
    my_public: SocketAddr,
    timeout: Duration,
) -> anyhow::Result<SocketAddr> {
    let stream = tokio::time::timeout(
        timeout,
        TcpStream::connect(&peer_addr.parse::<SocketAddr>()?),
    )
    .await??;
    let our_info = format!("{}:{}\n", my_public.ip(), my_public.port());
    stream.writable().await?;
    stream.try_write(our_info.as_bytes())?;

    let mut buf = String::new();
    let mut reader = tokio::io::BufReader::new(stream);
    tokio::time::timeout(timeout, reader.read_line(&mut buf)).await??;
    let peer_public: SocketAddr = buf.trim().parse()
        .map_err(|e| anyhow::anyhow!("invalid peer endpoint: {}", e))?;
    Ok(peer_public)
}

/// Attempt UDP hole punching between two peers.
///
/// Both peers call this function. Each sends UDP packets to the other's
/// public endpoint simultaneously. This opens a bi-directional NAT mapping.
///
/// `try_quic` — if true, also attempts a QUIC handshake over the punched path.
pub struct PunchResult {
    /// Whether the punch established connectivity.
    pub success: bool,
    /// Address data was received from (the peer's public endpoint).
    pub peer_public: Option<SocketAddr>,
    /// RTT after punch.
    pub rtt_ms: f64,
    /// Whether QUIC handshake succeeded over the punched path.
    pub quic_success: bool,
}

pub async fn punch_hole(
    my_public: SocketAddr,
    peer_public: SocketAddr,
    try_quic: bool,
    cancel: Option<&CancelToken>,
) -> anyhow::Result<PunchResult> {
    let socket = tokio::net::UdpSocket::bind(format!("0.0.0.0:{}", my_public.port())).await?;

    let start = Instant::now();
    let punch_msg = format!("PUNCH {}:{}", my_public.ip(), my_public.port());
    let mut sent_to_peer = false;
    let mut received_from_peer = false;
    let deadline = start + Duration::from_secs(5);
    let mut retry_delay = Duration::from_millis(50);

    while Instant::now() < deadline {
        if let Some(ref c) = cancel {
            if c.is_cancelled() {
                break;
            }
        }

        // Send a packet to the peer's public endpoint
        if socket.send_to(punch_msg.as_bytes(), peer_public).await.is_ok() {
            sent_to_peer = true;
        }

        // Try to receive with short timeout via tokio::time::timeout
        let mut buf = [0u8; 512];
        match tokio::time::timeout(Duration::from_millis(100), socket.recv_from(&mut buf)).await {
            Ok(Ok((n, addr))) => {
                let _msg = String::from_utf8_lossy(&buf[..n]);
                received_from_peer = true;
                if addr == peer_public {
                    break;
                }
            }
            _ => {
                // No packet received yet — back off exponentially
                tokio::time::sleep(retry_delay).await;
                retry_delay = (retry_delay * 2).min(Duration::from_secs(1));
            }
        }
    }

    let rtt = start.elapsed().as_secs_f64() * 1000.0;
    let quic_success = if try_quic && sent_to_peer && received_from_peer {
        attempt_quic_over_punched_path(peer_public).await
    } else {
        false
    };

    Ok(PunchResult {
        success: sent_to_peer && received_from_peer,
        peer_public: Some(peer_public),
        rtt_ms: rtt,
        quic_success,
    })
}

/// Attempt a QUIC handshake over the just-punched UDP path.
async fn attempt_quic_over_punched_path(peer: SocketAddr) -> bool {
    #[cfg(feature = "quic")]
    {
        match crate::transport::quic::try_connect_quick(peer).await {
            Ok(_) => true,
            Err(_) => false,
        }
    }
    #[cfg(not(feature = "quic"))]
    {
        let _ = peer;
        false
    }
}

/// Determine the best approach for reaching a peer based on NAT analysis.
pub fn analyze_connectivity(local_nat: &StunResult, peer_nat: &StunResult) -> NatTraversalResult {
    let both_symmetric = matches!(local_nat.nat_type, NatType::Symmetric)
        && matches!(peer_nat.nat_type, NatType::Symmetric);

    if both_symmetric {
        NatTraversalResult {
            success: false,
            mapped_addr: None,
            nat_type: local_nat.nat_type,
            recommended_transport: "relay",
        }
    } else if local_nat.nat_type == NatType::Symmetric || peer_nat.nat_type == NatType::Symmetric {
        NatTraversalResult {
            success: true,
            mapped_addr: Some(local_nat.public_addr),
            nat_type: local_nat.nat_type,
            recommended_transport: "tcp",
        }
    } else {
        NatTraversalResult {
            success: true,
            mapped_addr: Some(local_nat.public_addr),
            nat_type: local_nat.nat_type,
            recommended_transport: "quic",
        }
    }
}
