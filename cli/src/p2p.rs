use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::{info, error, warn};

/// A discovered P2P node (Android agent via mDNS).
#[derive(Debug, Clone)]
pub struct P2pNode {
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub node_name: String,
    pub platform: String,
}

/// Discover `_xync._tcp` services via mDNS for a given timeout.
pub async fn discover_xync_nodes(timeout_secs: u64) -> anyhow::Result<Vec<P2pNode>> {
    let mdns = ServiceDaemon::new()?;
    let service_type = "_xync._tcp.local.";
    let receiver = mdns.browse(service_type)?;

    let discovered = Arc::new(Mutex::new(Vec::new()));
    let discovered_clone = discovered.clone();

    let listen_handle = tokio::spawn(async move {
        loop {
            match receiver.recv_async().await {
                Ok(ServiceEvent::ServiceResolved(svc)) => {
                    let ip = svc.get_addresses_v4().iter()
                        .next()
                        .map(|a| a.to_string())
                        .unwrap_or_default();
                    let node = P2pNode {
                        name: svc.get_fullname().to_string(),
                        ip,
                        port: svc.get_port(),
                        node_name: svc.get_property_val_str("node_name")
                            .unwrap_or("unknown").to_string(),
                        platform: svc.get_property_val_str("platform")
                            .unwrap_or("unknown").to_string(),
                    };
                    info!(name = %node.name, ip = %node.ip, port = %node.port, "discovered P2P node via mDNS");
                    let mut list = discovered_clone.lock().await;
                    if !list.iter().any(|n: &P2pNode| n.name == node.name) {
                        list.push(node);
                    }
                }
                Ok(ServiceEvent::ServiceFound(_, _)) => {}
                Ok(ServiceEvent::ServiceRemoved(_, _)) => {}
                Err(e) => {
                    error!(error = %e, "mDNS recv error");
                    break;
                }
                _ => {}
            }
        }
    });

    tokio::time::sleep(Duration::from_secs(timeout_secs)).await;
    listen_handle.abort();

    let result = discovered.lock().await.clone();
    mdns.shutdown()?;
    Ok(result)
}

/// Connect to a P2P node's control WebSocket (port 7891).
pub async fn connect_to_node(node: &P2pNode) -> anyhow::Result<dos_networking::WsConnection> {
    let url = format!("ws://{}:{}", node.ip, node.port);
    info!(url = %url, "connecting to P2P node");
    let conn = dos_networking::connect(&url).await?;
    Ok(conn)
}

/// Connect to a video stream (screen mirror or camera) and receive H.264 frames.
/// Writes raw H.264 Annex-B to stdout by default.
/// Use `output` arg to redirect to a file.
pub async fn receive_video_stream(ip: &str, port: u16) -> anyhow::Result<()> {
    let addr = format!("{ip}:{port}");
    info!(addr = %addr, "connecting to video stream");
    let mut stream = TcpStream::connect(&addr).await
        .map_err(|e| anyhow::anyhow!("Failed to connect to {addr}: {e}"))?;

    info!("Connected to video stream, receiving H.264...");

    // Write raw H.264 to stdout (pipe to file or player)
    let stdout = tokio::io::stdout();
    let mut writer = tokio::io::BufWriter::new(stdout);

    // Write AVCC -> Annex-B conversion header
    // We'll just write raw NALs with start codes
    let mut buf = vec![0u8; 8192];
    let mut frame_count = 0u64;

    loop {
        // Read 4-byte frame size prefix (big-endian)
        let mut size_buf = [0u8; 4];
        match stream.read_exact(&mut size_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                info!("Video stream ended");
                break;
            }
            Err(e) => {
                warn!(error = %e, "read error");
                break;
            }
        }

        let frame_size = u32::from_be_bytes(size_buf) as usize;

        // Ensure buffer is large enough
        if buf.len() < frame_size {
            buf.resize(frame_size, 0);
        }

        // Read frame data
        stream.read_exact(&mut buf[..frame_size]).await?;

        // Write Annex-B start code + NAL
        writer.write_all(&[0x00, 0x00, 0x00, 0x01]).await?;
        writer.write_all(&buf[..frame_size]).await?;

        frame_count += 1;
        if frame_count % 30 == 0 {
            info!("Received {frame_count} frames");
        }
    }

    writer.flush().await?;
    info!("Video stream finished, received {frame_count} total frames");
    Ok(())
}

/// Advertise this Mac node on `_xync._tcp` so Android can discover us.
/// Returns the `ServiceDaemon` — keep it alive for the duration of advertising.
pub fn advertise_xync(port: u16, node_name: &str) -> anyhow::Result<ServiceDaemon> {
    let mdns = ServiceDaemon::new()?;
    let hostname = format!(
        "pdos-mac-{}",
        uuid::Uuid::new_v4().to_string().chars().take(4).collect::<String>()
    );
    let service_type = "_xync._tcp.local.";

    let mut properties = HashMap::new();
    properties.insert("platform".to_string(), "mac".to_string());
    properties.insert("node_name".to_string(), node_name.to_string());
    properties.insert("version".to_string(), env!("CARGO_PKG_VERSION").to_string());

    let service_info = ServiceInfo::new(
        service_type,
        node_name,
        &format!("{}.local.", hostname),
        "0.0.0.0",
        port,
        properties,
    )?;

    mdns.register(service_info)?;
    info!(name = %node_name, port = %port, "advertised on _xync._tcp");
    Ok(mdns)
}
