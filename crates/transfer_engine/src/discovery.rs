//! LAN device discovery via UDP broadcast.
//!
//! Two modes:
//! - **Advertising**: periodically broadcasts our presence on the LAN.
//! - **Scanning**: listens for broadcasts from other devices.
//!
//! The wire format is a compact binary protocol:
//!
//! ```text
//! Offset  Size  Field
//! 0       4     magic: b"XYNC"
//! 4       1     version (0x01)
//! 5       1     msg_type (0x01=Announce)
//! 6       16    device_id (UUID raw bytes)
//! 22      2     name_len (big-endian u16)
//! 24      N     name (UTF-8)
//! 24+N    1     transport_flags (bit0=TCP, bit1=TLS, bit2=QUIC, bit3=Relay)
//! 25+N    2     transfer_port (big-endian u16)
//! 27+N    1     platform_len
//! 28+N    P     platform (UTF-8)
//! 28+N+P  1     addr_count
//! 29+N+P  V     addresses: each [1 byte type(4|6), 4|16 bytes addr]
//! ```

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use tracing::{info, warn, error};
use uuid::Uuid;

use crate::peer::PeerManager;
use crate::{DeviceInfo, TrustStatus};

// ── Constants ─────────────────────────────────────────────────────────

pub const DISCOVERY_PORT: u16 = 18251;
pub const ADVERTISE_INTERVAL: Duration = Duration::from_secs(5);
pub const SCAN_INTERVAL: Duration = Duration::from_secs(3);

/// Maximum size of a discovery message (fits in one UDP packet).
const MAX_MSG_SIZE: usize = 1024;

// ── Wire Protocol ────────────────────────────────────────────────────

const MAGIC: [u8; 4] = *b"XYNC";
const PROTOCOL_VERSION: u8 = 0x01;

/// Discovery message types.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgType {
    Announce = 0x01,
    Query = 0x02,
    Response = 0x03,
}

impl MsgType {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::Announce),
            0x02 => Some(Self::Query),
            0x03 => Some(Self::Response),
            _ => None,
        }
    }
}

/// Transport protocol support flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportFlags(u8);

impl TransportFlags {
    pub const TCP: u8 = 0b0001;
    pub const TLS: u8 = 0b0010;
    pub const QUIC: u8 = 0b0100;
    pub const RELAY: u8 = 0b1000;

    pub fn new(flags: u8) -> Self {
        Self(flags)
    }

    pub fn supports_tcp(&self) -> bool { self.0 & Self::TCP != 0 }
    pub fn supports_tls(&self) -> bool { self.0 & Self::TLS != 0 }
    pub fn supports_quic(&self) -> bool { self.0 & Self::QUIC != 0 }
    pub fn supports_relay(&self) -> bool { self.0 & Self::RELAY != 0 }

    pub fn as_u8(&self) -> u8 { self.0 }

    pub fn names(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.supports_tcp() { v.push("tcp"); }
        if self.supports_tls() { v.push("tls"); }
        if self.supports_quic() { v.push("quic"); }
        if self.supports_relay() { v.push("relay"); }
        v
    }
}

/// A structured discovery query message.
#[derive(Debug, Clone)]
pub struct DiscoveryQuery {
    pub device_id: String,
    pub device_name: String,
}

/// A structured discovery announce or response message.
#[derive(Debug, Clone)]
pub struct DiscoveryAnnounce {
    pub msg_type: MsgType,
    pub device_id: String,
    pub device_name: String,
    pub transport_flags: TransportFlags,
    pub transfer_port: u16,
    pub platform: String,
    pub addresses: Vec<Vec<u8>>,
}

/// Encode a discovery query message.
pub fn encode_query(device_id: &str, device_name: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(&MAGIC);
    buf.push(PROTOCOL_VERSION);
    buf.push(MsgType::Query as u8);
    if let Ok(u) = Uuid::parse_str(device_id) {
        buf.extend_from_slice(u.as_bytes());
    } else {
        buf.extend_from_slice(&[0u8; 16]);
    }
    let name_bytes = device_name.as_bytes();
    let name_len = name_bytes.len().min(u16::MAX as usize) as u16;
    buf.extend_from_slice(&name_len.to_be_bytes());
    buf.extend_from_slice(&name_bytes[..name_len as usize]);
    buf
}

/// Decode a discovery query message.
pub fn decode_query(buf: &[u8]) -> Option<DiscoveryQuery> {
    if buf.len() < 24 {
        return None;
    }
    if buf[0..4] != MAGIC || buf[4] != PROTOCOL_VERSION || buf[5] != MsgType::Query as u8 {
        return None;
    }
    let uuid_bytes = &buf[6..22];
    let device_id = Uuid::from_slice(uuid_bytes)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| uuid_bytes.iter().map(|b| format!("{:02x}", b)).collect());
    let name_len = u16::from_be_bytes([buf[22], buf[23]]) as usize;
    if buf.len() < 24 + name_len {
        return None;
    }
    let device_name = String::from_utf8_lossy(&buf[24..24 + name_len]).to_string();
    Some(DiscoveryQuery { device_id, device_name })
}

/// Encode a discovery announce message into a binary buffer.
pub fn encode_announce(msg: &DiscoveryAnnounce) -> Vec<u8> {
    let mut buf = Vec::with_capacity(MAX_MSG_SIZE);

    // Magic + version + msg_type
    buf.extend_from_slice(&MAGIC);
    buf.push(PROTOCOL_VERSION);
    buf.push(msg.msg_type as u8);

    // Device ID (UUID bytes)
    if let Ok(u) = Uuid::parse_str(&msg.device_id) {
        buf.extend_from_slice(u.as_bytes());
    } else {
        // Pad/short UUID: use all zeros
        buf.extend_from_slice(&[0u8; 16]);
    }

    // Name
    let name_bytes = msg.device_name.as_bytes();
    let name_len = name_bytes.len().min(u16::MAX as usize) as u16;
    buf.extend_from_slice(&name_len.to_be_bytes());
    buf.extend_from_slice(&name_bytes[..name_len as usize]);

    // Transport flags + port
    buf.push(msg.transport_flags.as_u8());
    buf.extend_from_slice(&msg.transfer_port.to_be_bytes());

    // Platform
    let plat_bytes = msg.platform.as_bytes();
    let plat_len = plat_bytes.len().min(u8::MAX as usize) as u8;
    buf.push(plat_len);
    buf.extend_from_slice(&plat_bytes[..plat_len as usize]);

    // Addresses
    let addr_count = msg.addresses.len().min(u8::MAX as usize) as u8;
    buf.push(addr_count);
    for addr in &msg.addresses {
        // addr should already be raw IPv4 (4 bytes) or IPv6 (16 bytes)
        buf.push(addr.len() as u8);
        buf.extend_from_slice(addr);
    }

    buf
}

/// Try to parse a discovery message from a binary buffer.
/// Returns `None` if the buffer is invalid or from an unsupported version.
pub fn decode_discovery(buf: &[u8]) -> Option<DiscoveryAnnounce> {
    if buf.len() < 6 {
        return None;
    }
    if buf[0..4] != MAGIC {
        return None;
    }
    if buf[4] != PROTOCOL_VERSION {
        return None;
    }
    let msg_type = MsgType::from_u8(buf[5])?;
    if msg_type != MsgType::Announce && msg_type != MsgType::Response {
        return None;
    }

    let mut offset = 6usize;

    // Device ID (16 bytes)
    if buf.len() < offset + 16 {
        return None;
    }
    let uuid_bytes = &buf[offset..offset + 16];
    let device_id = Uuid::from_slice(uuid_bytes)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| uuid_bytes.iter().map(|b| format!("{:02x}", b)).collect());
    offset += 16;

    // Name
    if buf.len() < offset + 2 {
        return None;
    }
    let name_len = u16::from_be_bytes([buf[offset], buf[offset + 1]]) as usize;
    offset += 2;
    if buf.len() < offset + name_len {
        return None;
    }
    let device_name = String::from_utf8_lossy(&buf[offset..offset + name_len]).to_string();
    offset += name_len;

    // Transport flags
    if buf.len() < offset + 1 {
        return None;
    }
    let transport_flags = TransportFlags::new(buf[offset]);
    offset += 1;

    // Port
    if buf.len() < offset + 2 {
        return None;
    }
    let transfer_port = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
    offset += 2;

    // Platform
    if buf.len() < offset + 1 {
        return None;
    }
    let plat_len = buf[offset] as usize;
    offset += 1;
    if buf.len() < offset + plat_len {
        return None;
    }
    let platform = String::from_utf8_lossy(&buf[offset..offset + plat_len]).to_string();
    offset += plat_len;

    // Addresses
    if buf.len() < offset + 1 {
        return None;
    }
    let addr_count = buf[offset] as usize;
    offset += 1;
    let mut addresses = Vec::with_capacity(addr_count);
    for _ in 0..addr_count {
        if buf.len() < offset + 1 {
            return None;
        }
        let addr_len = buf[offset] as usize;
        offset += 1;
        if buf.len() < offset + addr_len {
            return None;
        }
        addresses.push(buf[offset..offset + addr_len].to_vec());
        offset += addr_len;
    }

    Some(DiscoveryAnnounce {
        msg_type,
        device_id,
        device_name,
        transport_flags,
        transfer_port,
        platform,
        addresses,
    })
}

// ── Advertise / Scan ─────────────────────────────────────────────────

/// Start advertising this device on the LAN.
///
/// Spawns a background task that periodically broadcasts our identity.
/// The task stops when the returned `AdvertHandle` is dropped.
pub fn start_advertising(
    _peer_mgr: Arc<PeerManager>,
    device_id: String,
    device_name: String,
    transport_flags: TransportFlags,
    transfer_port: u16,
    platform: String,
    local_addrs: Vec<Vec<u8>>,
) -> AdvertHandle {
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
    let handle = AdvertHandle { _stop_tx: stop_tx };

    let announce = DiscoveryAnnounce {
        msg_type: MsgType::Announce,
        device_id,
        device_name,
        transport_flags,
        transfer_port,
        platform,
        addresses: local_addrs,
    };
    let payload = encode_announce(&announce);

    tokio::spawn(async move {
        let socket = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => {
                error!(error = %e, "failed to bind discovery advertisement socket");
                return;
            }
        };
        if let Err(e) = socket.set_broadcast(true) {
            warn!(error = %e, "failed to set broadcast");
        }

        let broadcast_addr: SocketAddr = SocketAddr::from(([255, 255, 255, 255], DISCOVERY_PORT));
        let broadcast6: SocketAddr = SocketAddr::from(([0xff02, 0, 0, 0, 0, 0, 0, 1], DISCOVERY_PORT));
        let loopback_addr: SocketAddr = SocketAddr::from(([127, 0, 0, 1], DISCOVERY_PORT));

        // Send a Query packet and an Announce packet on startup to request immediate responses and announce our presence
        let query_payload = encode_query(&announce.device_id, &announce.device_name);
        let _ = socket.send_to(&query_payload, broadcast_addr).await;
        let _ = socket.send_to(&query_payload, broadcast6).await;
        let _ = socket.send_to(&query_payload, loopback_addr).await;

        let _ = socket.send_to(&payload, broadcast_addr).await;
        let _ = socket.send_to(&payload, broadcast6).await;
        let _ = socket.send_to(&payload, loopback_addr).await;

        info!(
            name = %announce.device_name,
            port = %transfer_port,
            transports = ?announce.transport_flags.names(),
            "starting LAN advertisement"
        );

        loop {
            tokio::select! {
                _ = &mut stop_rx => break,
                _ = tokio::time::sleep(ADVERTISE_INTERVAL) => {
                    let _ = socket.send_to(&payload, broadcast_addr).await;
                    let _ = socket.send_to(&payload, broadcast6).await;
                    let _ = socket.send_to(&payload, loopback_addr).await;
                }
            }
        }
    });

    handle
}

/// Start scanning the LAN for peer discovery broadcasts.
///
/// Discovered peers are fed into the `PeerManager`. Also supports
/// a callback for notification of new/disappeared peers.
/// Runs until the returned `ScanHandle` is dropped.
pub fn start_scanning(
    peer_mgr: Arc<PeerManager>,
    our_announce: Option<DiscoveryAnnounce>,
    on_device_discovered: Option<Arc<dyn Fn(DeviceInfo) + Send + Sync>>,
) -> ScanHandle {
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
    let handle = ScanHandle { _stop_tx: stop_tx };

    tokio::spawn(async move {
        let socket = match UdpSocket::bind(format!("0.0.0.0:{}", DISCOVERY_PORT)).await {
            Ok(s) => s,
            Err(e) => {
                error!(
                    error = %e,
                    "failed to bind discovery scan socket on port {}",
                    DISCOVERY_PORT,
                );
                return;
            }
        };

        // Also listen on IPv6 if available
        let _ = socket.set_broadcast(true);

        info!("listening for LAN peers on port {}", DISCOVERY_PORT);

        let mut buf = [0u8; MAX_MSG_SIZE];
        let mut reap_interval = tokio::time::interval(Duration::from_secs(10));
        // Avoid immediate tick on startup
        reap_interval.tick().await;

        loop {
            tokio::select! {
                _ = &mut stop_rx => break,
                _ = reap_interval.tick() => {
                    peer_mgr.reap_expired().await;
                }
                result = socket.recv_from(&mut buf) => {
                    match result {
                        Ok((len, peer_addr)) => {
                            // Check for Query message
                            if let Some(query) = decode_query(&buf[..len]) {
                                if let Some(ref our) = our_announce {
                                    if query.device_id != our.device_id {
                                        // Unicast our response directly back to the querying peer
                                        let response = DiscoveryAnnounce {
                                            msg_type: MsgType::Response,
                                            device_id: our.device_id.clone(),
                                            device_name: our.device_name.clone(),
                                            transport_flags: our.transport_flags.clone(),
                                            transfer_port: our.transfer_port,
                                            platform: our.platform.clone(),
                                            addresses: our.addresses.clone(),
                                        };
                                        let reply = encode_announce(&response);
                                        let _ = socket.send_to(&reply, peer_addr).await;
                                    }
                                }
                                continue;
                            }

                            if let Some(announce) = decode_discovery(&buf[..len]) {
                                // Skip our own announcements
                                if let Some(peer_id) = peer_mgr.get(&announce.device_id).await {
                                    if peer_id.address == peer_addr.to_string() {
                                        continue;
                                    }
                                }

                                let device_info = DeviceInfo {
                                    device_id: announce.device_id.clone(),
                                    name: announce.device_name,
                                    device_type: announce.platform,
                                    trust_status: TrustStatus::Unknown,
                                    last_seen: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs(),
                                    addresses: vec![
                                        format!("{}:{}", peer_addr.ip(), announce.transfer_port)
                                    ],
                                    transport_flags: announce.transport_flags.as_u8(),
                                    protocol_version: "1.0.0".to_string(),
                                };

                                // Register in peer manager
                                let mut peer_entry = crate::peer::PeerInfo::new(
                                    announce.device_id.clone(),
                                    device_info.name.clone(),
                                    peer_addr.to_string(),
                                );
                                peer_entry.method = "lan".to_string();
                                peer_entry.platform = device_info.device_type.clone();
                                peer_mgr.register(peer_entry).await;

                                // Notify callback if present
                                if let Some(ref cb) = on_device_discovered {
                                    cb(device_info);
                                }
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "discovery recv error");
                        }
                    }
                }
            }
        }
    });

    handle
}

// ── Handles ──────────────────────────────────────────────────────────

/// Stops LAN advertisement when dropped.
pub struct AdvertHandle {
    _stop_tx: tokio::sync::oneshot::Sender<()>,
}

/// Stops LAN scanning when dropped.
pub struct ScanHandle {
    _stop_tx: tokio::sync::oneshot::Sender<()>,
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let msg = DiscoveryAnnounce {
            msg_type: MsgType::Announce,
            device_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            device_name: "Alice's Phone".to_string(),
            transport_flags: TransportFlags::new(TransportFlags::TCP | TransportFlags::TLS),
            transfer_port: 18250,
            platform: "macos".to_string(),
            addresses: vec![
                vec![192, 168, 1, 10],
                vec![10, 0, 0, 5],
            ],
        };

        let encoded = encode_announce(&msg);
        assert!(encoded.len() < MAX_MSG_SIZE);

        let decoded = decode_discovery(&encoded).expect("should decode");
        assert_eq!(decoded.device_id, msg.device_id);
        assert_eq!(decoded.device_name, msg.device_name);
        assert!(decoded.transport_flags.supports_tcp());
        assert!(decoded.transport_flags.supports_tls());
        assert!(!decoded.transport_flags.supports_quic());
        assert_eq!(decoded.transfer_port, 18250);
        assert_eq!(decoded.platform, "macos");
        assert_eq!(decoded.addresses.len(), 2);
    }

    #[test]
    fn test_decode_invalid_magic() {
        let buf = b"XXXX...".to_vec();
        assert!(decode_discovery(&buf).is_none());
    }

    #[test]
    fn test_decode_short_buffer() {
        let buf = vec![0u8; 3];
        assert!(decode_discovery(&buf).is_none());
    }

    #[test]
    fn test_transport_flag_names() {
        let flags = TransportFlags::new(0b1011);
        let names = flags.names();
        assert!(names.contains(&"tcp"));
        assert!(names.contains(&"tls"));
        assert!(names.contains(&"relay"));
        assert!(!names.contains(&"quic"));
    }

    #[test]
    fn test_decode_non_announce_type() {
        let mut buf = encode_announce(&DiscoveryAnnounce {
            msg_type: MsgType::Announce,
            device_id: Uuid::new_v4().to_string(),
            device_name: "test".into(),
            transport_flags: TransportFlags::new(0),
            transfer_port: 0,
            platform: "".into(),
            addresses: vec![],
        });
        // Change msg_type to an invalid value (0x99)
        buf[5] = 0x99;
        assert!(decode_discovery(&buf).is_none());
    }

    #[test]
    fn test_query_encode_decode_roundtrip() {
        let device_id = Uuid::new_v4().to_string();
        let device_name = "Querying Device".to_string();
        let encoded = encode_query(&device_id, &device_name);
        assert!(encoded.len() < MAX_MSG_SIZE);

        let decoded = decode_query(&encoded).expect("should decode query");
        assert_eq!(decoded.device_id, device_id);
        assert_eq!(decoded.device_name, device_name);
    }

    #[tokio::test]
    async fn test_scanning_and_advertising_lan() {
        let peer_mgr = Arc::new(PeerManager::new(Duration::from_secs(2)));
        let device_id = Uuid::new_v4().to_string();
        let device_name = "Ad-Tester".to_string();
        let transport_flags = TransportFlags::new(TransportFlags::TCP);
        let transfer_port = 12345;
        let platform = "test-os".to_string();
        let local_addrs = vec![vec![127, 0, 0, 1]];

        let our_announce = DiscoveryAnnounce {
            msg_type: MsgType::Announce,
            device_id: device_id.clone(),
            device_name: device_name.clone(),
            transport_flags: transport_flags.clone(),
            transfer_port,
            platform: platform.clone(),
            addresses: local_addrs.clone(),
        };

        // We will start scanning with on_device_discovered callback
        let (discovered_tx, mut discovered_rx) = tokio::sync::mpsc::channel(1);
        let cb = Arc::new(move |device: DeviceInfo| {
            let _ = discovered_tx.try_send(device);
        });

        let _scan_handle = start_scanning(peer_mgr.clone(), Some(our_announce), Some(cb));

        // Start advertising with a distinct ID so scanner registers it
        let remote_id = Uuid::new_v4().to_string();
        let _advert_handle = start_advertising(
            peer_mgr.clone(),
            remote_id.clone(),
            "Remote-Peer".to_string(),
            transport_flags,
            transfer_port,
            platform,
            local_addrs,
        );

        // Await discovery
        let discovered = tokio::time::timeout(Duration::from_secs(5), discovered_rx.recv())
            .await
            .expect("timeout waiting for discovery")
            .expect("should receive discovered device");

        assert_eq!(discovered.device_id, remote_id);
        assert_eq!(discovered.name, "Remote-Peer");

        // Verify the peer is registered in peer manager
        let peer = peer_mgr.get(&remote_id).await;
        assert!(peer.is_some());

        // Wait for TTL (2 seconds) + some padding to let reaping happen
        tokio::time::sleep(Duration::from_millis(2500)).await;
        peer_mgr.reap_expired().await;
        let peer_after_expiry = peer_mgr.get(&remote_id).await;
        assert!(peer_after_expiry.is_none(), "peer should be reaped after TTL expiry");
    }
}
