//! QUIC transport using `quinn`.
//!
//! Single QUIC connection, multiple bi-directional streams for parallelism.
//! QUIC handles reliability natively — no ChunkTracker, no ACK/NACK.
//!
//! Control loop only manages: stream parallelism, pacing rate.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use sha2::Digest;
use tokio::io::AsyncReadExt;
use tokio::sync::Semaphore;

use crate::control::ControlLoop;
use crate::streaming::TransferSessionResult;
use crate::window::FlowWindow;
use crate::TransferOptions;

use super::core::{Transport, TransportConnection, TransportListener};

// ── TLS configuration (self-signed for P2P) ─────────────────────────────

pub(crate) fn generate_tls_config() -> anyhow::Result<(quinn::ClientConfig, quinn::ServerConfig)> {
    let cert = rcgen::generate_simple_self_signed(vec!["xync.local".into()])?;
    let cert_der = cert.cert.der().clone();
    let key_der = cert.key_pair.serialize_der();

    let tls_server = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert_der],
            rustls::pki_types::PrivateKeyDer::from(
                rustls::pki_types::PrivatePkcs8KeyDer::from(key_der),
            ),
        )?;

    let server_crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls_server)?;
    let server_config = quinn::ServerConfig::with_crypto(Arc::new(server_crypto));

    let tls_client = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
        .with_no_client_auth();
    let client_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls_client)?;
    let client_config = quinn::ClientConfig::new(Arc::new(client_crypto));

    Ok((client_config, server_config))
}

#[derive(Debug)]
struct SkipServerVerification;

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

// ── Shared runtime ──────────────────────────────────────────────────────

fn make_endpoint(
    server_config: Option<quinn::ServerConfig>,
    bind_addr: &str,
) -> anyhow::Result<quinn::Endpoint> {
    let socket = std::net::UdpSocket::bind(bind_addr)?;
    socket.set_nonblocking(true)?;
    let runtime = Arc::new(quinn::TokioRuntime);
    Ok(quinn::Endpoint::new(
        quinn::EndpointConfig::default(),
        server_config,
        socket,
        runtime,
    )?)
}

// ── QUIC Transport ──────────────────────────────────────────────────────

pub struct QuicTransport {
    client_config: quinn::ClientConfig,
    server_config: quinn::ServerConfig,
}

impl QuicTransport {
    pub fn new() -> anyhow::Result<Self> {
        let (client_config, server_config) = generate_tls_config()?;
        Ok(Self { client_config, server_config })
    }

    #[allow(dead_code)]
    async fn connect_raw(&self, addr: &str) -> anyhow::Result<quinn::Connection> {
        let endpoint = make_endpoint(None, "0.0.0.0:0")?;
        let remote_addr: std::net::SocketAddr = addr.parse()?;
        let conn = endpoint
            .connect_with(self.client_config.clone(), remote_addr, "xync.local")?
            .await?;
        // Keep endpoint alive by storing it in the connection's app data
        let _ = conn;
        // We need the endpoint to stay alive — return it tied to the conn
        Ok(endpoint.connect_with(self.client_config.clone(), remote_addr, "xync.local")?.await?)
    }
}

#[async_trait]
impl Transport for QuicTransport {
    fn name(&self) -> &'static str { "quic" }
    fn is_reliable(&self) -> bool { true }
    fn supports_multiplexing(&self) -> bool { true }

    async fn connect(&self, addr: &str) -> anyhow::Result<Box<dyn TransportConnection>> {
        let endpoint = make_endpoint(None, "0.0.0.0:0")?;
        let remote_addr: std::net::SocketAddr = addr.parse()?;
        let connection = endpoint
            .connect_with(self.client_config.clone(), remote_addr, "xync.local")?
            .await?;
        Ok(Box::new(QuicConnection { connection, _endpoint: endpoint }))
    }

    async fn listen(&self, addr: &str) -> anyhow::Result<Box<dyn TransportListener>> {
        let endpoint = make_endpoint(Some(self.server_config.clone()), addr)?;
        let local = endpoint.local_addr()?;
        Ok(Box::new(QuicListener { endpoint, local: local.to_string() }))
    }

    async fn upload(
        &self,
        addr: &str,
        path: &std::path::Path,
        display_name: &str,
        options: &TransferOptions,
    control: Option<&ControlLoop>,
    ) -> anyhow::Result<TransferSessionResult> {
        quic_upload(self, addr, path, display_name, options, control).await
    }
}

// ── QUIC connection wrapper ─────────────────────────────────────────────

pub struct QuicConnection {
    connection: quinn::Connection,
    _endpoint: quinn::Endpoint,
}

#[async_trait]
impl TransportConnection for QuicConnection {
    async fn send(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let (mut send_stream, _recv_stream) = self.connection.open_bi().await?;
        tokio::io::AsyncWriteExt::write_all(&mut send_stream, data).await?;
        send_stream.finish()?;
        Ok(())
    }

    async fn recv(&mut self, buf: &mut [u8]) -> anyhow::Result<usize> {
        let (_send_stream, mut recv_stream) = self.connection.accept_bi().await?;
        let n = recv_stream.read(buf).await?.unwrap_or(0);
        Ok(n)
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        self.connection.close(0u32.into(), b"transfer complete");
        Ok(())
    }
}

pub struct QuicListener {
    endpoint: quinn::Endpoint,
    local: String,
}

#[async_trait]
impl TransportListener for QuicListener {
    async fn accept(&mut self) -> anyhow::Result<Box<dyn TransportConnection>> {
        let connection = self.endpoint.accept().await
            .ok_or_else(|| anyhow::anyhow!("QUIC listener closed"))?
            .await?;
        let ep = make_endpoint(None, "0.0.0.0:0")?;
        Ok(Box::new(QuicConnection { connection, _endpoint: ep }))
    }

    fn local_addr(&self) -> anyhow::Result<String> {
        Ok(self.local.clone())
    }
}

// ── QUIC upload logic (streaming, memory-safe) ──────────────────────

/// Stream a file over QUIC without loading it entirely into memory.
///
/// Uses `tokio::fs::File` + `tokio::io::BufReader` for chunked reads.
/// A `FlowWindow` bounds in-flight bytes to prevent OOM.
/// SHA-256 is computed incrementally during send.
async fn quic_upload(
    transport: &QuicTransport,
    addr: &str,
    path: &Path,
    display_name: &str,
    options: &TransferOptions,
    control: Option<&ControlLoop>,
) -> anyhow::Result<TransferSessionResult> {
    let _ = control; // will wire control loop in Phase 2
    let size = tokio::fs::metadata(path).await?.len();
    let start = Instant::now();
    let resume_offset = options.resume_offset;

    // Connect once, reuse for all streams
    let endpoint = make_endpoint(None, "0.0.0.0:0")?;
    let remote_addr: std::net::SocketAddr = addr.parse()?;
    let connecting = endpoint
        .connect_with(transport.client_config.clone(), remote_addr, "xync.local")
        .map_err(|e| anyhow::anyhow!("QUIC connect failed: {}", e))?;
    let connection = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        connecting,
    )
    .await
    .map_err(|_| anyhow::anyhow!("QUIC connect timed out after 10s"))?
    .map_err(|e| anyhow::anyhow!("QUIC handshake failed: {}", e))?;
    let conn = Arc::new(connection);

    // Send metadata on stream 0
    let meta = serde_json::json!({
        "filename": display_name,
        "size": size,
        "resume_offset": resume_offset,
    });
    let meta_bytes = serde_json::to_vec(&meta)?;
    {
        let (mut send0, _recv0) = conn.open_bi().await?;
        let len_bytes = (meta_bytes.len() as u32).to_be_bytes();
        send0.write_all(&len_bytes).await?;
        send0.write_all(&meta_bytes).await?;
        send0.finish()?;
    }

    let num_streams = if options.parallel { options.parallel_streams.max(1) } else { 1 };
    let chunk_size = options.chunk_size;
    let pacing_rate = options.throughput_limit_mbps;

    // Flow window bounds in-flight data across all streams
    let flow_window = Arc::new(FlowWindow::default());
    let total_sent = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let hasher = Arc::new(std::sync::Mutex::new(sha2::Sha256::new()));

    let mut handles = Vec::new();
    let offset = Arc::new(std::sync::atomic::AtomicU64::new(resume_offset));

    // Open the file once and share via Arc<Mutex<File>> for concurrent reads
    let file = tokio::fs::File::open(path).await?;
    let file = Arc::new(tokio::sync::Mutex::new(file));

    // Pre-hash skipped bytes for resume
    if resume_offset > 0 {
        let mut prehash_buf = vec![0u8; 65536];
        let mut file_guard = file.lock().await;
        let mut remaining = resume_offset;
        while remaining > 0 {
            let to_read = remaining.min(65536);
            file_guard.read_exact(&mut prehash_buf[..to_read as usize]).await?;
            hasher.lock().unwrap().update(&prehash_buf[..to_read as usize]);
            remaining -= to_read;
        }
    }

    let semaphore = Arc::new(Semaphore::new(num_streams));

    while offset.load(std::sync::atomic::Ordering::Relaxed) < size {
        // Acquire a stream slot
        let _permit = semaphore.clone().acquire_owned().await?;

        let offset_clone = offset.clone();
        let file_clone = file.clone();
        let hasher_clone = hasher.clone();
        let total_sent_clone = total_sent.clone();
        let flow = flow_window.clone();
        let pacing = pacing_rate;
        let conn_clone = conn.clone();
        let chunk_sz = chunk_size;

        let h = tokio::spawn(async move {
            // Read a chunk from file
            let (data, chunk_len) = {
                let mut file_guard = file_clone.lock().await;
                let pos = offset_clone.load(std::sync::atomic::Ordering::Relaxed);
                if pos >= size {
                    return anyhow::Result::<()>::Ok(());
                }
                let to_read = (size - pos).min(chunk_sz as u64);
                let mut buf = vec![0u8; to_read as usize];
                file_guard.read_exact(&mut buf).await?;
                offset_clone.fetch_add(to_read, std::sync::atomic::Ordering::Relaxed);
                (buf, to_read)
            };

            // Acquire flow window permit (bounds memory)
            let _wp = flow.acquire(chunk_len).await;

            let (mut send, _recv) = conn_clone.open_bi().await?;
            let write_start = Instant::now();

            // Write the chunk
            send.write_all(&data).await?;
            send.finish()?;

            // Hash the chunk
            hasher_clone.lock().unwrap().update(&data);

            total_sent_clone.fetch_add(chunk_len, std::sync::atomic::Ordering::Relaxed);

            // Pacing
            if let Some(rate) = pacing {
                if rate > 0.0 {
                    let elapsed = write_start.elapsed().as_secs_f64();
                    let target = (chunk_len as f64 * 8.0) / (rate * 1_000_000.0);
                    if elapsed < target {
                        tokio::time::sleep(std::time::Duration::from_secs_f64(target - elapsed)).await;
                    }
                }
            }

            anyhow::Result::<()>::Ok(())
        });
        handles.push(h);
    }

    // Wait for all streams to complete
    for h in handles {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(300), h)
            .await
            .map_err(|_| anyhow::anyhow!("QUIC stream timed out"))?;
    }

    // Send final checksum on a control stream
    let hash = {
        let h = hasher.lock().unwrap();
        h.clone().finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>()
    };
    {
        let (mut send_hash, _recv_hash) = conn.open_bi().await?;
        let checksum_msg = serde_json::json!({"checksum": hash});
        let checksum_bytes = serde_json::to_vec(&checksum_msg)?;
        let len_buf = (checksum_bytes.len() as u32).to_be_bytes();
        send_hash.write_all(&len_buf).await?;
        send_hash.write_all(&checksum_bytes).await?;
        send_hash.finish()?;
    }

    conn.close(0u32.into(), b"transfer complete");
    let _ = endpoint.wait_idle().await;

    let total_sent_val = total_sent.load(std::sync::atomic::Ordering::Relaxed);
    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 {
        (total_sent_val as f64 * 8.0) / (elapsed * 1_000_000.0)
    } else { 0.0 };

    Ok(TransferSessionResult { bytes_sent: total_sent_val, bytes_total: size, speed_mbps: speed })
}

/// Quick QUIC connectivity check — used to verify a punched UDP path.
///
/// Connects to `peer` and immediately closes. Returns `Ok(())` if the
/// full TLS+QUIC handshake succeeds within 3 seconds.
pub async fn try_connect_quick(peer: std::net::SocketAddr) -> anyhow::Result<()> {
    let (client_config, _) = generate_tls_config()?;
    let endpoint = make_endpoint(None, "0.0.0.0:0")?;
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        async {
            let conn = endpoint
                .connect_with(client_config, peer, "xync.local")?
                .await?;
            conn.close(0u32.into(), b"probe");
            Ok::<_, anyhow::Error>(())
        },
    )
    .await
    .map_err(|_| anyhow::anyhow!("QUIC quick-connect timed out after 3s"))??;
    Ok(())
}
