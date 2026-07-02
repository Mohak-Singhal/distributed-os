//! Core Transport trait and TCP implementation.

use std::path::Path;
use std::time::Instant;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::control::ControlLoop;
use crate::keepalive::KeepaliveStream;
use crate::streaming::TransferSessionResult;
use crate::TransferOptions;

// ── Transport trait ─────────────────────────────────────────────────────

/// A transport protocol implementation.
///
/// Each transport encapsulates:
/// - Connection establishment
/// - Data send/receive semantics
/// - Built-in reliability (or lack thereof)
/// - Connection teardown
#[async_trait]
pub trait Transport: Send + Sync {
    /// Human-readable transport name (e.g. "tcp", "quic").
    fn name(&self) -> &'static str;

    /// Whether the transport provides its own reliability (retransmission,
    /// ordering). If `true`, the caller should skip custom ChunkTracker/ACK.
    fn is_reliable(&self) -> bool;

    /// Whether the transport supports multiplexed streams over one connection.
    fn supports_multiplexing(&self) -> bool;

    /// Connect to a remote peer.
    async fn connect(&self, addr: &str) -> anyhow::Result<Box<dyn TransportConnection>>;

    /// Bind and listen for incoming connections.
    async fn listen(&self, addr: &str) -> anyhow::Result<Box<dyn TransportListener>>;

    /// Upload a file using this transport.
    async fn upload(
        &self,
        addr: &str,
        path: &Path,
        display_name: &str,
        options: &TransferOptions,
        control: Option<&ControlLoop>,
    ) -> anyhow::Result<TransferSessionResult>;
}

/// An established transport connection (send + recv over one logical stream).
#[async_trait]
pub trait TransportConnection: Send {
    /// Send data. For reliable transports this is fire-and-forget;
    /// for unreliable transports the caller must handle retransmission.
    async fn send(&mut self, data: &[u8]) -> anyhow::Result<()>;

    /// Receive data into a buffer. Returns the number of bytes read.
    /// Returns `Ok(0)` on clean EOF.
    async fn recv(&mut self, buf: &mut [u8]) -> anyhow::Result<usize>;

    /// Receive into a growing vector (reads until EOF or error).
    async fn recv_all(&mut self, buf: &mut Vec<u8>) -> anyhow::Result<usize> {
        let mut tmp = [0u8; 65536];
        loop {
            let n = self.recv(&mut tmp).await?;
            if n == 0 {
                return Ok(buf.len());
            }
            buf.extend_from_slice(&tmp[..n]);
        }
    }

    /// Close the connection gracefully.
    async fn close(&mut self) -> anyhow::Result<()>;
}

/// A listener that accepts incoming transport connections.
#[async_trait]
pub trait TransportListener: Send {
    /// Accept the next incoming connection. Blocks until one arrives.
    async fn accept(&mut self) -> anyhow::Result<Box<dyn TransportConnection>>;

    /// Local socket address this listener is bound to.
    fn local_addr(&self) -> anyhow::Result<String>;
}

// ── TCP Transport ───────────────────────────────────────────────────────

/// TCP transport using the existing reliable layer.
pub struct TcpTransport;

#[async_trait]
impl Transport for TcpTransport {
    fn name(&self) -> &'static str {
        "tcp"
    }

    fn is_reliable(&self) -> bool {
        true
    }

    fn supports_multiplexing(&self) -> bool {
        false
    }

    async fn connect(&self, addr: &str) -> anyhow::Result<Box<dyn TransportConnection>> {
        let (host, port) = parse_addr(addr)?;
        let stream = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            connect_optimized(&host, port),
        )
        .await
        .map_err(|_| anyhow::anyhow!("TCP connect timed out after 10s"))??;
        Ok(Box::new(TcpConnection { stream }))
    }

    async fn listen(&self, addr: &str) -> anyhow::Result<Box<dyn TransportListener>> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let local = listener.local_addr()?;
        Ok(Box::new(TcpListener {
            inner: listener,
            local: local.to_string(),
        }))
    }

    async fn upload(
        &self,
        addr: &str,
        path: &Path,
        display_name: &str,
        options: &TransferOptions,
        control: Option<&ControlLoop>,
    ) -> anyhow::Result<TransferSessionResult> {
        tcp_upload(addr, path, display_name, options, control).await
    }
}

// ── TCP connection wrappers ──────────────────────────────────────────────

pub struct TcpConnection {
    stream: TcpStream,
}

#[async_trait]
impl TransportConnection for TcpConnection {
    async fn send(&mut self, data: &[u8]) -> anyhow::Result<()> {
        self.stream.write_all(data).await?;
        Ok(())
    }

    async fn recv(&mut self, buf: &mut [u8]) -> anyhow::Result<usize> {
        let n = self.stream.read(buf).await?;
        Ok(n)
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        self.stream.shutdown().await?;
        Ok(())
    }
}

pub struct TcpListener {
    inner: tokio::net::TcpListener,
    local: String,
}

#[async_trait]
impl TransportListener for TcpListener {
    async fn accept(&mut self) -> anyhow::Result<Box<dyn TransportConnection>> {
        let (stream, _) = self.inner.accept().await?;
        Ok(Box::new(TcpConnection { stream }))
    }

    fn local_addr(&self) -> anyhow::Result<String> {
        Ok(self.local.clone())
    }
}

// ── TCP upload logic (reuses HTTP streaming infrastructure) ────────────

use crate::http::{connect_optimized, urlencode};

async fn tcp_upload(
    addr: &str,
    path: &Path,
    display_name: &str,
    options: &TransferOptions,
    control: Option<&ControlLoop>,
) -> anyhow::Result<TransferSessionResult> {
    let size = tokio::fs::metadata(path).await?.len();
    let start = Instant::now();
    let resume_offset = options.resume_offset;

    let (host, port) = parse_addr(addr)?;
    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        connect_optimized(&host, port),
    )
    .await
    .map_err(|_| anyhow::anyhow!("TCP connect timed out after 10s"))??;
    let url_encoded = urlencode(display_name);

    let remaining = size.saturating_sub(resume_offset);
    let header = format!(
        "POST /api/receive-file HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         Content-Type: application/octet-stream\r\n\
         X-Filename: {url_encoded}\r\n\
         Content-Length: {remaining}\r\n\
         X-Resume-Offset: {resume_offset}\r\n\
         Connection: close\r\n\
         \r\n"
    );

    // Wrap with keepalive
    let (ka_stream, _kh) = KeepaliveStream::new(stream);

    let (sent, _hash) = crate::http::stream_file_send_with_resume(
        ka_stream, &header, path, size, options.chunk_size, resume_offset, None, control, options.cancel_token.as_ref(),
    )
    .await?;

    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 {
        (sent as f64 * 8.0) / (elapsed * 1_000_000.0)
    } else {
        0.0
    };

    Ok(TransferSessionResult {
        bytes_sent: sent,
        bytes_total: size,
        speed_mbps: speed,
    })
}

fn parse_addr(addr: &str) -> anyhow::Result<(String, u16)> {
    match addr.rsplit_once(':') {
        Some((host, port_str)) => {
            let port: u16 = port_str.parse().map_err(|_| {
                anyhow::anyhow!("Invalid port in address: {}", addr)
            })?;
            Ok((host.to_string(), port))
        }
        None => Err(anyhow::anyhow!("Invalid address (expected host:port): {}", addr)),
    }
}
