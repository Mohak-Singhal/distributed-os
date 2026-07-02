use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use anyhow::{anyhow, Result};

// Protocol Frame Types
pub const FRAME_HANDSHAKE_REQ: u8 = 1;
pub const FRAME_HANDSHAKE_RESP: u8 = 2;
pub const FRAME_DATA: u8 = 3;
pub const FRAME_ACK: u8 = 4;
pub const FRAME_FIN: u8 = 5;
pub const FRAME_FIN_ACK: u8 = 6;

pub const UDP_CHUNK_SIZE: usize = 8192;

pub struct UdpMetrics {
    pub rtt_ms: f64,
    pub rtt_var_ms: f64,
    pub packet_loss_pct: f64,
    pub cwnd: usize,
    pub retransmits: u64,
}

pub async fn send_file_udp(
    file_path: &Path,
    remote_addr: SocketAddr,
    progress_cb: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
) -> Result<UdpMetrics> {
    let file_data = tokio::fs::read(file_path).await?;
    let total_size = file_data.len() as u64;
    let num_chunks = ((total_size + UDP_CHUNK_SIZE as u64 - 1) / UDP_CHUNK_SIZE as u64) as usize;

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(remote_addr).await?;

    // 1. Handshake Phase
    let mut handshake_success = false;
    let mut handshake_attempts = 0;
    let mut buf = [0u8; 1024];

    while handshake_attempts < 5 {
        let mut req = vec![FRAME_HANDSHAKE_REQ];
        req.extend_from_slice(&total_size.to_be_bytes());
        socket.send(&req).await?;

        socket.writable().await?;
        tokio::select! {
            res = socket.recv(&mut buf) => {
                if let Ok(n) = res {
                    if n > 0 && buf[0] == FRAME_HANDSHAKE_RESP {
                        handshake_success = true;
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(300)) => {
                handshake_attempts += 1;
            }
        }
    }

    if !handshake_success {
        return Err(anyhow!("UDP handshake failed — remote device did not respond"));
    }

    // 2. Data Transmission Phase with Sliding Window & Congestion Control
    let mut next_seq = 0usize;
    let mut acked_seqs = HashSet::new();
    let mut sent_times = HashMap::new();
    let mut last_sent_time = HashMap::new();
    
    // Congestion Window (packets)
    let mut cwnd = 10usize;
    let mut rtt_ms = 50.0;
    let mut rtt_var_ms = 10.0;
    let mut retransmits = 0u64;
    let mut total_sent = 0u64;

    let (ack_tx, mut ack_rx) = mpsc::channel::<usize>(1000);
    let socket_arc = Arc::new(socket);
    let socket_read = socket_arc.clone();

    // Start background receiver for ACKs
    tokio::spawn(async move {
        let mut read_buf = [0u8; 64];
        loop {
            if let Ok(n) = socket_read.recv(&mut read_buf).await {
                if n >= 9 && read_buf[0] == FRAME_ACK {
                    let mut seq_bytes = [0u8; 8];
                    seq_bytes.copy_from_slice(&read_buf[1..9]);
                    let seq = usize::from_be_bytes(seq_bytes);
                    let _ = ack_tx.send(seq).await;
                } else if n > 0 && read_buf[0] == FRAME_FIN_ACK {
                    let _ = ack_tx.send(99999999).await; // Signal FinAck
                    break;
                }
            } else {
                break;
            }
        }
    });

    let start_time = Instant::now();
    let timeout_duration = Duration::from_millis(150);

    while acked_seqs.len() < num_chunks {
        // Send packets within Congestion Window
        while next_seq < num_chunks && (next_seq - acked_seqs.len()) < cwnd {
            let offset = next_seq * UDP_CHUNK_SIZE;
            let end = (offset + UDP_CHUNK_SIZE).min(file_data.len());
            let chunk = &file_data[offset..end];

            let mut packet = vec![FRAME_DATA];
            packet.extend_from_slice(&next_seq.to_be_bytes());
            packet.extend_from_slice(&(chunk.len() as u16).to_be_bytes());
            packet.extend_from_slice(chunk);

            socket_arc.send(&packet).await?;
            let now = Instant::now();
            sent_times.insert(next_seq, now);
            last_sent_time.insert(next_seq, now);
            total_sent += 1;

            // Packet pacing (micro-delay based on tuning config)
            let active_cfg = crate::adaptive::get_active_config();
            if active_cfg.chunk_size_bytes > 0 {
                // Pace dynamically: e.g. 50-250 microseconds delay
                tokio::time::sleep(Duration::from_micros(100)).await;
            }

            next_seq += 1;
        }

        // Wait for ACK or Timeout
        tokio::select! {
            maybe_seq = ack_rx.recv() => {
                if let Some(seq) = maybe_seq {
                    if seq == 99999999 {
                        break;
                    }
                    if acked_seqs.insert(seq) {
                        // Calculate RTT
                        if let Some(sent) = sent_times.get(&seq) {
                            let sample = sent.elapsed().as_secs_f64() * 1000.0;
                            let diff = (sample - rtt_ms).abs();
                            rtt_ms = rtt_ms * 0.875 + sample * 0.125;
                            rtt_var_ms = rtt_var_ms * 0.75 + diff * 0.25;
                        }
                        // Congestion window: Additive Increase
                        cwnd += 1;
                        
                        if let Some(ref cb) = progress_cb {
                            cb((acked_seqs.len() * UDP_CHUNK_SIZE).min(file_data.len()) as u64, total_size);
                        }
                    }
                }
            }
            _ = tokio::time::sleep(timeout_duration) => {
                // Check outstanding packets for retransmission
                let now = Instant::now();
                for seq in acked_seqs.len()..next_seq {
                    if !acked_seqs.contains(&seq) {
                        let should_retransmit = last_sent_time.get(&seq)
                            .map(|&t| now.duration_since(t) >= timeout_duration)
                            .unwrap_or(true);
                        
                        if should_retransmit {
                            let offset = seq * UDP_CHUNK_SIZE;
                            let end = (offset + UDP_CHUNK_SIZE).min(file_data.len());
                            let chunk = &file_data[offset..end];

                            let mut packet = vec![FRAME_DATA];
                            packet.extend_from_slice(&seq.to_be_bytes());
                            packet.extend_from_slice(&(chunk.len() as u16).to_be_bytes());
                            packet.extend_from_slice(chunk);

                            socket_arc.send(&packet).await?;
                            last_sent_time.insert(seq, now);
                            retransmits += 1;
                            total_sent += 1;
                        }
                    }
                }
                // Multiplicative Decrease
                cwnd = (cwnd / 2).max(4);
            }
        }
    }

    // 3. Fin Phase
    let mut fin_attempts = 0;
    while fin_attempts < 5 {
        let fin_packet = vec![FRAME_FIN];
        socket_arc.send(&fin_packet).await?;
        
        tokio::select! {
            maybe_seq = ack_rx.recv() => {
                if let Some(99999999) = maybe_seq {
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {
                fin_attempts += 1;
            }
        }
    }

    let loss_pct = if total_sent > 0 {
        (retransmits as f64 / total_sent as f64) * 100.0
    } else {
        0.0
    };

    Ok(UdpMetrics {
        rtt_ms,
        rtt_var_ms,
        packet_loss_pct: loss_pct,
        cwnd,
        retransmits,
    })
}

pub async fn receive_file_udp(
    save_path: &Path,
    listen_port: u16,
    expected_size: u64,
    progress_cb: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
) -> Result<()> {
    let socket = UdpSocket::bind(format!("0.0.0.0:{}", listen_port)).await?;
    let mut buf = vec![0u8; UDP_CHUNK_SIZE + 32];
    
    let mut file_buf = vec![0u8; expected_size as usize];
    let num_chunks = ((expected_size + UDP_CHUNK_SIZE as u64 - 1) / UDP_CHUNK_SIZE as u64) as usize;
    let mut received_chunks = HashSet::new();
    let mut client_addr = None;

    loop {
        let (n, addr) = socket.recv_from(&mut buf).await?;
        if n == 0 { continue; }
        
        client_addr = Some(addr);
        let frame_type = buf[0];

        match frame_type {
            FRAME_HANDSHAKE_REQ => {
                let resp = vec![FRAME_HANDSHAKE_RESP];
                socket.send_to(&resp, addr).await?;
            }
            FRAME_DATA => {
                if n < 11 { continue; }
                let mut seq_bytes = [0u8; 8];
                seq_bytes.copy_from_slice(&buf[1..9]);
                let seq = usize::from_be_bytes(seq_bytes);

                let mut len_bytes = [0u8; 2];
                len_bytes.copy_from_slice(&buf[9..11]);
                let len = u16::from_be_bytes(len_bytes) as usize;

                if n >= 11 + len {
                    let offset = seq * UDP_CHUNK_SIZE;
                    if offset + len <= file_buf.len() {
                        file_buf[offset..offset+len].copy_from_slice(&buf[11..11+len]);
                        if received_chunks.insert(seq) {
                            if let Some(ref cb) = progress_cb {
                                cb(received_chunks.len() as u64 * UDP_CHUNK_SIZE as u64, expected_size);
                            }
                        }
                    }
                    // Send ACK
                    let mut ack = vec![FRAME_ACK];
                    ack.extend_from_slice(&seq.to_be_bytes());
                    socket.send_to(&ack, addr).await?;
                }
            }
            FRAME_FIN => {
                let resp = vec![FRAME_FIN_ACK];
                socket.send_to(&resp, addr).await?;
                if received_chunks.len() >= num_chunks {
                    break;
                }
            }
            _ => {}
        }
    }

    tokio::fs::write(save_path, &file_buf).await?;
    Ok(())
}

pub async fn receive_file_udp_with_socket(
    socket: UdpSocket,
    save_path: &Path,
    expected_size: u64,
    progress_cb: Option<Box<dyn Fn(u64, u64) + Send + Sync>>,
) -> Result<()> {
    let mut buf = vec![0u8; UDP_CHUNK_SIZE + 32];
    
    let mut file_buf = vec![0u8; expected_size as usize];
    let num_chunks = ((expected_size + UDP_CHUNK_SIZE as u64 - 1) / UDP_CHUNK_SIZE as u64) as usize;
    let mut received_chunks = HashSet::new();

    loop {
        let (n, addr) = socket.recv_from(&mut buf).await?;
        if n == 0 { continue; }
        
        let frame_type = buf[0];

        match frame_type {
            FRAME_HANDSHAKE_REQ => {
                let resp = vec![FRAME_HANDSHAKE_RESP];
                socket.send_to(&resp, addr).await?;
            }
            FRAME_DATA => {
                if n < 11 { continue; }
                let mut seq_bytes = [0u8; 8];
                seq_bytes.copy_from_slice(&buf[1..9]);
                let seq = usize::from_be_bytes(seq_bytes);

                let mut len_bytes = [0u8; 2];
                len_bytes.copy_from_slice(&buf[9..11]);
                let len = u16::from_be_bytes(len_bytes) as usize;

                if n >= 11 + len {
                    let offset = seq * UDP_CHUNK_SIZE;
                    if offset + len <= file_buf.len() {
                        file_buf[offset..offset+len].copy_from_slice(&buf[11..11+len]);
                        if received_chunks.insert(seq) {
                            if let Some(ref cb) = progress_cb {
                                cb(received_chunks.len() as u64 * UDP_CHUNK_SIZE as u64, expected_size);
                            }
                        }
                    }
                    // Send ACK
                    let mut ack = vec![FRAME_ACK];
                    ack.extend_from_slice(&seq.to_be_bytes());
                    socket.send_to(&ack, addr).await?;
                }
            }
            FRAME_FIN => {
                let resp = vec![FRAME_FIN_ACK];
                socket.send_to(&resp, addr).await?;
                if received_chunks.len() >= num_chunks {
                    break;
                }
            }
            _ => {}
        }
    }

    tokio::fs::write(save_path, &file_buf).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[tokio::test]
    async fn test_udp_transport_end_to_end() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let src_path = temp_dir.path().join("src.bin");
        let dst_path = temp_dir.path().join("dst.bin");

        let mock_data: Vec<u8> = (0..50 * 1024).map(|i| (i % 256) as u8).collect();
        tokio::fs::write(&src_path, &mock_data).await?;

        let rx_socket = UdpSocket::bind("127.0.0.1:0").await?;
        let rx_addr = rx_socket.local_addr()?;

        let dst_path_clone = dst_path.clone();
        let bytes_received = Arc::new(AtomicU64::new(0));
        let bytes_received_cb = bytes_received.clone();
        
        let recv_handle = tokio::spawn(async move {
            let cb = move |recv: u64, _total: u64| {
                bytes_received_cb.store(recv, Ordering::Relaxed);
            };
            receive_file_udp_with_socket(rx_socket, &dst_path_clone, 50 * 1024, Some(Box::new(cb))).await
        });

        let progress_cb = Arc::new(|_sent: u64, _total: u64| {});
        let metrics = send_file_udp(&src_path, rx_addr, Some(progress_cb)).await?;

        recv_handle.await??;

        let received_data = tokio::fs::read(&dst_path).await?;
        assert_eq!(mock_data, received_data);

        assert!(metrics.rtt_ms >= 0.0);
        assert!(metrics.packet_loss_pct >= 0.0);
        assert!(metrics.cwnd > 0);

        Ok(())
    }
}
