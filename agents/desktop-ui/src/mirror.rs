use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Clone)]
pub struct MirrorStats {
    pub connected: bool,
    pub frames_received: u64,
    pub fps: f64,
    pub bitrate_kbps: f64,
    pub width: u32,
    pub height: u32,
}

pub async fn receive_mirror_stream(
    ip: &str,
    port: u16,
    stats: Arc<Mutex<MirrorStats>>,
    output_path: Option<String>,
) -> anyhow::Result<()> {
    let addr = format!("{}:{}", ip, port);
    info!("Connecting to screen mirror at {addr}");
    let mut stream = TcpStream::connect(&addr).await?;

    {
        let mut s = stats.lock().await;
        s.connected = true;
        s.frames_received = 0;
    }

    let mut writer: Option<tokio::io::BufWriter<tokio::fs::File>> = if let Some(path) = output_path
    {
        let file = tokio::fs::File::create(&path).await?;
        Some(tokio::io::BufWriter::new(file))
    } else {
        None
    };

    let mut frame_buf = vec![0u8; 4 * 1024 * 1024]; // 4MB max frame
    let mut frame_count = 0u64;
    let start = Instant::now();
    let mut last_log = Instant::now();
    let mut bytes_received = 0u64;

    loop {
        let mut size_buf = [0u8; 4];
        match stream.read_exact(&mut size_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                info!("Mirror stream ended");
                break;
            }
            Err(e) => {
                warn!("Mirror read error: {e}");
                break;
            }
        }

        let frame_size = u32::from_be_bytes(size_buf) as usize;

        if frame_buf.len() < frame_size {
            frame_buf.resize(frame_size, 0);
        }

        stream.read_exact(&mut frame_buf[..frame_size]).await?;
        bytes_received += frame_size as u64;
        frame_count += 1;

        // Write Annex-B to file if saving
        if let Some(ref mut w) = writer {
            use tokio::io::AsyncWriteExt;
            w.write_all(&[0x00, 0x00, 0x00, 0x01]).await?;
            w.write_all(&frame_buf[..frame_size]).await?;
        }

        // Update stats every 30 frames
        if frame_count % 30 == 0 {
            let elapsed = start.elapsed().as_secs_f64();
            let mut s = stats.lock().await;
            s.frames_received = frame_count;
            s.fps = frame_count as f64 / elapsed.max(0.001);
            s.bitrate_kbps = (bytes_received as f64 * 8.0 / 1000.0) / elapsed.max(0.001);
        }

        // Log every 5 seconds
        if last_log.elapsed() > Duration::from_secs(5) {
            let elapsed = start.elapsed().as_secs_f64();
            info!(
                "Mirror: {frame_count} frames, {:.1} fps, {:.1} kbps",
                frame_count as f64 / elapsed.max(0.001),
                (bytes_received as f64 * 8.0 / 1000.0) / elapsed.max(0.001)
            );
            last_log = Instant::now();
        }
    }

    {
        let mut s = stats.lock().await;
        s.connected = false;
    }

    info!("Mirror session ended: {frame_count} frames received");
    Ok(())
}
