//! TLS-wrapped TCP transport.
//!
//! Each device gets a self-signed certificate derived from its identity.
//! Peers authenticate via TOFU (Trust On First Use) — the first peer's
//! cert fingerprint is stored and enforced on subsequent connections.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::net::TcpStream;
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::control::ControlLoop;
use crate::identity::DeviceIdentity;
use crate::keepalive::KeepaliveStream;
use crate::streaming::TransferSessionResult;
use crate::TransferOptions;

use super::core::{Transport, TransportConnection, TransportListener};
use super::tofu::TofuStore;

// ── Certificate generation ──────────────────────────────────────────────

/// Generate a self-signed certificate from the device identity.
/// The certificate's CN is set to the device_id for peer identification.
pub fn generate_self_signed_cert(
    identity: &DeviceIdentity,
) -> anyhow::Result<(rustls::pki_types::CertificateDer<'static>, rustls::pki_types::PrivateKeyDer<'static>)> {
    let rcgen_cert = rcgen::generate_simple_self_signed(vec![identity.device_id.clone()])?;
    let cert_der = rcgen_cert.cert.der().clone();
    let key_der = rcgen_cert.key_pair.serialize_der();

    let cert = rustls::pki_types::CertificateDer::from(cert_der.to_vec());
    let key = rustls::pki_types::PrivateKeyDer::try_from(key_der)
        .map_err(|e| anyhow::anyhow!("failed to parse private key: {:?}", e))?;
    Ok((cert, key))
}

/// Build a TLS server config from the device cert+key.
pub fn server_config(
    cert: rustls::pki_types::CertificateDer<'static>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
) -> anyhow::Result<Arc<rustls::ServerConfig>> {
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)?;
    Ok(Arc::new(config))
}

/// Build a TLS client config that verifies the peer using TOFU.
/// The `CertVerifier` calls back into the `TofuStore` to check fingerprints.
pub fn client_config(
    store: Arc<std::sync::Mutex<TofuStore>>,
    peer_id: String,
    peer_name: String,
) -> anyhow::Result<Arc<rustls::ClientConfig>> {
    let verifier = TofuCertVerifier { store, peer_id, peer_name };
    let config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok(Arc::new(config))
}

// ── TOFU Certificate Verifier ───────────────────────────────────────────

#[derive(Debug)]
struct TofuCertVerifier {
    store: Arc<std::sync::Mutex<TofuStore>>,
    peer_id: String,
    peer_name: String,
}

impl rustls::client::danger::ServerCertVerifier for TofuCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let fp = TofuStore::fingerprint(end_entity.as_ref());
        let mut store = self.store.lock().unwrap();

        if store.is_trusted(&self.peer_id, &fp) {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else if store.fingerprint_for(&self.peer_id).is_none() {
            // First connection — trust on first use
            store.trust(&self.peer_id, &fp, &self.peer_name);
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            // Fingerprint mismatch — possible MITM
            Err(rustls::Error::General("TOFU fingerprint mismatch — peer identity changed".into()))
        }
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

// ── TlsTcpTransport ─────────────────────────────────────────────────────

/// TCP transport with TLS encryption via rustls.
pub struct TlsTcpTransport {
    server_cfg: Arc<rustls::ServerConfig>,
    client_cfg: Arc<rustls::ClientConfig>,
}

impl TlsTcpTransport {
    /// Create a new TLS-wrapped TCP transport.
    ///
    /// `identity`: our device identity (used for cert CN).
    /// `store`: TOFU trust store for peer verification.
    /// `peer_id`: expected peer device_id (for TOFU verification).
    /// `peer_name`: human-readable peer name.
    pub fn new(
        identity: &DeviceIdentity,
        store: Arc<std::sync::Mutex<TofuStore>>,
        peer_id: String,
        peer_name: String,
    ) -> anyhow::Result<Self> {
        let (cert, key) = generate_self_signed_cert(identity)?;
        let server_cfg = server_config(cert, key)?;
        let client_cfg = client_config(store, peer_id, peer_name)?;
        Ok(Self { server_cfg, client_cfg })
    }
}

#[async_trait]
impl Transport for TlsTcpTransport {
    fn name(&self) -> &'static str { "tls-tcp" }
    fn is_reliable(&self) -> bool { true }
    fn supports_multiplexing(&self) -> bool { false }

    async fn connect(&self, addr: &str) -> anyhow::Result<Box<dyn TransportConnection>> {
        let tcp = TcpStream::connect(addr).await?;
        let connector = TlsConnector::from(self.client_cfg.clone());
        let dns_name = rustls::pki_types::ServerName::try_from("xync.local")
            .map_err(|_| anyhow::anyhow!("invalid DNS name"))?;
        let tls = connector.connect(dns_name, tcp).await?;
        Ok(Box::new(TlsConnection::new_client(tls)))
    }

    async fn listen(&self, addr: &str) -> anyhow::Result<Box<dyn TransportListener>> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let local = listener.local_addr()?.to_string();
        let acceptor = TlsAcceptor::from(self.server_cfg.clone());
        Ok(Box::new(TlsListener { listener, acceptor, local }))
    }

    async fn upload(
        &self,
        addr: &str,
        path: &Path,
        display_name: &str,
        options: &TransferOptions,
        control: Option<&ControlLoop>,
    ) -> anyhow::Result<TransferSessionResult> {
        let size = tokio::fs::metadata(path).await?.len();
        let start = std::time::Instant::now();
        let resume_offset = options.resume_offset;

        let tcp = TcpStream::connect(addr).await?;
        let connector = TlsConnector::from(self.client_cfg.clone());
        let dns_name = rustls::pki_types::ServerName::try_from("xync.local")
            .map_err(|_| anyhow::anyhow!("invalid DNS name"))?;
        let tls = connector.connect(dns_name, tcp).await?;

        let url_encoded = crate::http::urlencode(display_name);
        let remaining = size.saturating_sub(resume_offset);
        let header = format!(
            "POST /api/receive-file HTTP/1.1\r\n\
             Host: {addr}\r\n\
             Content-Type: application/octet-stream\r\n\
             X-Filename: {url_encoded}\r\n\
             Content-Length: {remaining}\r\n\
             X-Resume-Offset: {resume_offset}\r\n\
             Connection: close\r\n\
             \r\n"
        );

        // Wrap with keepalive
        let (ka_stream, _kh) = KeepaliveStream::new(tls);

        let (sent, _hash) = crate::http::stream_file_send_with_resume(
            ka_stream, &header, path, size, options.chunk_size, resume_offset, None, control, options.cancel_token.as_ref(),
        )
        .await?;

        let elapsed = start.elapsed().as_secs_f64();
        let speed = if elapsed > 0.0 {
            (sent as f64 * 8.0) / (elapsed * 1_000_000.0)
        } else { 0.0 };

        Ok(TransferSessionResult { bytes_sent: sent, bytes_total: size, speed_mbps: speed })
    }
}

// ── Connection / Listener wrappers ──────────────────────────────────────

pub struct TlsConnection(pub tokio_rustls::TlsStream<TcpStream>);

impl TlsConnection {
    fn new_client(tls: tokio_rustls::client::TlsStream<TcpStream>) -> Self {
        Self(tokio_rustls::TlsStream::Client(tls))
    }
    fn new_server(tls: tokio_rustls::server::TlsStream<TcpStream>) -> Self {
        Self(tokio_rustls::TlsStream::Server(tls))
    }
}

#[async_trait]
impl TransportConnection for TlsConnection {
    async fn send(&mut self, data: &[u8]) -> anyhow::Result<()> {
        use tokio::io::AsyncWriteExt;
        self.0.write_all(data).await?;
        Ok(())
    }

    async fn recv(&mut self, buf: &mut [u8]) -> anyhow::Result<usize> {
        use tokio::io::AsyncReadExt;
        let n = self.0.read(buf).await?;
        Ok(n)
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        use tokio::io::AsyncWriteExt;
        self.0.shutdown().await?;
        Ok(())
    }
}

pub struct TlsListener {
    listener: tokio::net::TcpListener,
    acceptor: TlsAcceptor,
    local: String,
}

#[async_trait]
impl TransportListener for TlsListener {
    async fn accept(&mut self) -> anyhow::Result<Box<dyn TransportConnection>> {
        let (tcp, _) = self.listener.accept().await?;
        let tls = self.acceptor.accept(tcp).await?;
        Ok(Box::new(TlsConnection::new_server(tls)))
    }

    fn local_addr(&self) -> anyhow::Result<String> {
        Ok(self.local.clone())
    }
}
