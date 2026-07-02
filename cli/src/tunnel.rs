use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, RwLock};
use tracing::{info, error, warn};

/// A phone connected via reverse tunnel (phone→Mac TCP)
struct TunnelEntry {
    stream: TcpStream,
    phone_ip: String,
}

lazy_static::lazy_static! {
    static ref TUNNELS: RwLock<HashMap<String, Arc<Mutex<TunnelEntry>>>> = RwLock::new(HashMap::new());
}

/// Frame format: [4-byte filename_len][filename][8-byte file_size][file_data]
const HEADER_SIZE: usize = 12; // 4 + 8

/// Start the reverse tunnel server on `listen_port`.
/// Phones (outbound from hotspot) connect here for reverse file delivery (Mac → Android).
pub async fn start_tunnel_server(listen_port: u16) {
    let addr = format!("0.0.0.0:{}", listen_port);
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => {
            info!(port = listen_port, "reverse tunnel server started");
            l
        }
        Err(e) => {
            error!(error = %e, "failed to start reverse tunnel server");
            return;
        }
    };

    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                let ip = peer_addr.ip().to_string();
                info!(ip = %ip, "reverse tunnel connection from phone");
                let entry = Arc::new(Mutex::new(TunnelEntry {
                    stream,
                    phone_ip: ip.clone(),
                }));
                TUNNELS.write().await.insert(ip.clone(), entry.clone());

                // Spawn a keepalive monitor — if connection drops, remove from registry
                let ip_clone = ip.clone();
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(Duration::from_secs(15));
                    loop {
                        interval.tick().await;
                        let mut guard = entry.lock().await;
                        if let Err(e) = guard.stream.try_write(&[0u8; 0]) {
                            warn!(ip = %ip_clone, error = %e, "tunnel keepalive failed, removing");
                            drop(guard);
                            TUNNELS.write().await.remove(&ip_clone);
                            break;
                        }
                    }
                });
            }
            Err(e) => {
                error!(error = %e, "tunnel accept error");
            }
        }
    }
}

/// Receive server on `listen_port` (default 7896) for files pushed from Android → Mac.
/// Uses the same frame format as the tunnel: [4-byte name_len][name][8-byte size][data]
pub async fn start_tunnel_receive_server(listen_port: u16) {
    let addr = format!("0.0.0.0:{}", listen_port);
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => {
            info!(port = listen_port, "tunnel receive server started (Android → Mac)");
            l
        }
        Err(e) => {
            error!(error = %e, "failed to start tunnel receive server");
            return;
        }
    };

    loop {
        match listener.accept().await {
            Ok((mut stream, peer_addr)) => {
                let ip = peer_addr.ip().to_string();
                info!(ip = %ip, "incoming file push from phone");
                tokio::spawn(async move {
                    handle_receive_frame(&mut stream, &ip).await;
                });
            }
            Err(e) => {
                error!(error = %e, "tunnel receive server accept error");
            }
        }
    }
}

async fn handle_receive_frame(stream: &mut TcpStream, ip: &str) {
    use std::io::SeekFrom;
    use tokio::io::AsyncSeekExt;

    loop {
        // Read 4-byte filename length
        let mut name_len_buf = [0u8; 4];
        if let Err(e) = stream.read_exact(&mut name_len_buf).await {
            info!(ip = %ip, error = %e, "tunnel receive: connection closed");
            return;
        }
        let name_len = u32::from_be_bytes(name_len_buf) as usize;
        if name_len > 1024 {
            warn!(ip = %ip, "invalid filename length: {}", name_len);
            return;
        }

        // Read filename
        let mut name_buf = vec![0u8; name_len];
        if let Err(e) = stream.read_exact(&mut name_buf).await {
            warn!(ip = %ip, error = %e, "failed to read filename");
            return;
        }
        let filename = String::from_utf8_lossy(&name_buf).to_string();

        // Read 8-byte file size
        let mut size_buf = [0u8; 8];
        if let Err(e) = stream.read_exact(&mut size_buf).await {
            warn!(ip = %ip, error = %e, "failed to read file size");
            return;
        }
        let file_size = u64::from_be_bytes(size_buf);
        if file_size > 10 * 1024 * 1024 * 1024 {
            warn!(ip = %ip, "invalid file size: {}", file_size);
            return;
        }

        info!(ip = %ip, filename = %filename, size = file_size, "receiving file push");

        // Save to ~/Downloads/PDOS/
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let downloads_dir = format!("{}/Downloads/PDOS", home);
        if let Err(e) = tokio::fs::create_dir_all(&downloads_dir).await {
            error!(error = %e, "failed to create downloads dir");
            return;
        }
        let output_path = format!("{}/{}", downloads_dir, filename);
        let mut file = match tokio::fs::File::create(&output_path).await {
            Ok(f) => f,
            Err(e) => {
                error!(error = %e, "failed to create output file");
                return;
            }
        };

        let mut remaining = file_size;
        let mut data_buf = vec![0u8; 65536];
        let mut total_written = 0u64;
        while remaining > 0 {
            let to_read = data_buf.len().min(remaining as usize);
            if let Err(e) = stream.read_exact(&mut data_buf[..to_read]).await {
                warn!(ip = %ip, error = %e, "failed to read file data");
                return;
            }
            if let Err(e) = file.write_all(&data_buf[..to_read]).await {
                error!(error = %e, "failed to write file");
                return;
            }
            remaining -= to_read as u64;
            total_written += to_read as u64;
        }
        let _ = file.flush().await;
        info!(ip = %ip, filename = %filename, bytes = total_written, "file push received");

        // Show macOS notification
        let script = format!(
            "display notification \"File received from Android: {}\" with title \"PDOS: File Received\" sound name \"default\"",
            filename
        );
        let _ = std::process::Command::new("osascript").arg("-e").arg(&script).output();
    }
}

/// Send a file through the reverse tunnel to `phone_ip`.
/// Falls back to `http_upload` if no tunnel exists or tunnel write fails.
pub async fn send_through_tunnel(
    phone_ip: &str,
    file_path: &str,
    remote_filename: &str,
) -> Result<(), TunnelError> {
    // Try exact IP match first, then fall back to any available tunnel
    let tunnel = {
        let tunnels = TUNNELS.read().await;
        tunnels.get(phone_ip).cloned().or_else(|| {
            // IP didn't match — try any registered tunnel (phone may have different IPs)
            if let Some((ip, entry)) = tunnels.iter().next() {
                info!(expected = %phone_ip, actual = %ip, "tunnel IP mismatch, using available tunnel");
                Some(entry.clone())
            } else {
                None
            }
        })
    };

    let entry = tunnel.ok_or(TunnelError::NoTunnel)?;
    let mut guard = entry.lock().await;

    let data = tokio::fs::read(file_path).await.map_err(|e| {
        error!(error = %e, path = %file_path, "failed to read file for tunnel");
        TunnelError::IoError(e)
    })?;

    let name_bytes = remote_filename.as_bytes();
    let name_len = name_bytes.len() as u32;
    let file_size = data.len() as u64;

    let mut header = Vec::with_capacity(HEADER_SIZE + name_len as usize + file_size as usize);
    header.extend_from_slice(&name_len.to_be_bytes());
    header.extend_from_slice(name_bytes);
    header.extend_from_slice(&file_size.to_be_bytes());
    header.extend_from_slice(&data);

    if let Err(e) = guard.stream.write_all(&header).await {
        warn!(ip = %phone_ip, error = %e, "tunnel write failed, removing tunnel");
        drop(guard);
        TUNNELS.write().await.remove(phone_ip);
        return Err(TunnelError::WriteError(e));
    }

    info!(filename = %remote_filename, size = file_size, ip = %phone_ip, "sent file through reverse tunnel");
    Ok(())
}

/// Check if a reverse tunnel exists for `phone_ip`.
pub async fn has_tunnel(phone_ip: &str) -> bool {
    TUNNELS.read().await.contains_key(phone_ip)
}

#[derive(Debug)]
pub enum TunnelError {
    NoTunnel,
    IoError(std::io::Error),
    WriteError(std::io::Error),
}

impl std::fmt::Display for TunnelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TunnelError::NoTunnel => write!(f, "no reverse tunnel for this phone (hotspot blocks direct TCP; run 'dos serve' on Mac and ensure phone has connected to it)"),
            TunnelError::IoError(e) => write!(f, "file read error: {}", e),
            TunnelError::WriteError(e) => write!(f, "tunnel write error: {}", e),
        }
    }
}
