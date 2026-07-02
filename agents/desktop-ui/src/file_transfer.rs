use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Clone)]
pub struct TransferStats {
    pub active: bool,
    pub speed_mbps: f64,
    pub progress: f64,
    pub bytes_sent: u64,
    pub total_bytes: u64,
    pub elapsed: Duration,
}

pub async fn send_file(
    ip: &str,
    port: u16,
    data: Bytes,
    remote_path: &str,
    stats: Arc<Mutex<TransferStats>>,
) -> anyhow::Result<()> {
    let addr = format!("{}:{}", ip, port + 10); // file transfer on control_port+10
    info!("Connecting to {addr} for file transfer");
    let mut stream = TcpStream::connect(&addr).await?;

    // Send header: remote_path length + remote_path + data length + data
    let path_bytes = remote_path.as_bytes();
    let total = (4 + path_bytes.len() + 8 + data.len()) as u64;

    {
        let mut s = stats.lock().await;
        s.active = true;
        s.total_bytes = total;
        s.bytes_sent = 0;
    }

    // Write path length + path
    stream.write_u32(path_bytes.len() as u32).await?;
    stream.write_all(path_bytes).await?;

    // Write data length + data
    stream.write_u64(data.len() as u64).await?;

    let start = Instant::now();
    let chunk_size = 256 * 1024; // 256KB chunks
    let mut offset = 0;

    while offset < data.len() {
        let end = (offset + chunk_size).min(data.len());
        let chunk = &data[offset..end];
        stream.write_all(chunk).await?;

        offset = end;
        let elapsed = start.elapsed();

        let mut s = stats.lock().await;
        s.bytes_sent = offset as u64;
        s.elapsed = elapsed;
        s.speed_mbps = if elapsed.as_secs_f64() > 0.0 {
            (offset as f64 / elapsed.as_secs_f64()) / (1024.0 * 1024.0)
        } else {
            0.0
        };
    }

    // Wait for ack
    let ack = stream.read_u8().await?;
    if ack != 0xAC {
        warn!("Server returned non-ACK: {ack}");
    }

    let mut s = stats.lock().await;
    s.active = false;
    s.progress = 100.0;
    info!(
        "Sent {} bytes in {:?} ({:.1} MB/s)",
        offset,
        start.elapsed(),
        s.speed_mbps
    );

    Ok(())
}

pub async fn receive_file(
    ip: &str,
    port: u16,
    remote_path: &str,
    local_path: &str,
    stats: Arc<Mutex<TransferStats>>,
) -> anyhow::Result<()> {
    let addr = format!("{}:{}", ip, port + 10);
    let mut stream = TcpStream::connect(&addr).await?;

    // Send request: 0x01 (read) + remote_path
    stream.write_u8(0x01).await?;
    stream.write_u32(remote_path.len() as u32).await?;
    stream.write_all(remote_path.as_bytes()).await?;

    // Read file size
    let file_size = stream.read_u64().await?;

    {
        let mut s = stats.lock().await;
        s.active = true;
        s.total_bytes = file_size;
        s.bytes_sent = 0;
    }

    let start = Instant::now();
    let mut received = 0u64;
    let mut file = tokio::fs::File::create(local_path).await?;
    let mut buf = vec![0u8; 256 * 1024];

    while received < file_size {
        let to_read = (file_size - received).min(buf.len() as u64) as usize;
        let n = stream.read(&mut buf[..to_read]).await?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).await?;
        received += n as u64;

        let elapsed = start.elapsed();
        let mut s = stats.lock().await;
        s.bytes_sent = received;
        s.elapsed = elapsed;
        s.speed_mbps = if elapsed.as_secs_f64() > 0.0 {
            (received as f64 / elapsed.as_secs_f64()) / (1024.0 * 1024.0)
        } else {
            0.0
        };
    }

    let mut s = stats.lock().await;
    s.active = false;
    s.progress = 100.0;
    info!(
        "Received {} bytes in {:?} ({:.1} MB/s)",
        received,
        start.elapsed(),
        s.speed_mbps
    );

    Ok(())
}
