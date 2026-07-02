use std::sync::Arc;
use tokio_rustls::rustls::{
    pki_types::CertificateDer,
    ServerConfig,
};

fn load_or_create_identity() -> anyhow::Result<dos_crypto::NodeIdentity> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let pdos_dir = format!("{}/.pdos", home);
    std::fs::create_dir_all(&pdos_dir)?;

    let identity_path = format!("{}/identity.bin", pdos_dir);
    if std::path::Path::new(&identity_path).exists() {
        let bytes = std::fs::read(&identity_path)?;
        if bytes.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Ok(dos_crypto::NodeIdentity::from_bytes(&key)?);
        }
    }
    let identity = dos_crypto::NodeIdentity::generate();
    std::fs::write(&identity_path, identity.to_signing_key_bytes())?;
    Ok(identity)
}

pub fn get_or_create_tls_config() -> anyhow::Result<(Arc<ServerConfig>, String)> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let pdos_dir = format!("{}/.pdos", home);
    std::fs::create_dir_all(&pdos_dir)?;

    let cert_path = format!("{}/tls_cert.pem", pdos_dir);
    let key_path = format!("{}/tls_key.pem", pdos_dir);

    let (cert_pem, key_pem) =
        if std::path::Path::new(&cert_path).exists() && std::path::Path::new(&key_path).exists() {
            (
                std::fs::read_to_string(&cert_path)?,
                std::fs::read_to_string(&key_path)?,
            )
        } else {
            let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519)?;
            let mut params = rcgen::CertificateParams::new(vec![
                "localhost".to_string(),
                "pdos.local".to_string(),
            ])?;
            params.distinguished_name = rcgen::DistinguishedName::new();
            params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);

            let cert = params.self_signed(&key_pair)?;
            let cert_pem = cert.pem();
            let key_pem = key_pair.serialize_pem();
            std::fs::write(&cert_path, &cert_pem)?;
            std::fs::write(&key_path, &key_pem)?;
            (cert_pem, key_pem)
        };

    use std::io::Cursor;
    let mut cert_reader = Cursor::new(cert_pem.as_bytes());
    let mut key_reader = Cursor::new(key_pem.as_bytes());

    let certs: Vec<CertificateDer> =
        rustls_pemfile::certs(&mut cert_reader).collect::<Result<Vec<_>, _>>()?;

    let key_opt = rustls_pemfile::private_key(&mut key_reader)?;
    let key = key_opt.ok_or_else(|| anyhow::anyhow!("No private key found"))?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    let identity = load_or_create_identity()?;
    let fingerprint = identity.public_key_hex();

    Ok((Arc::new(config), fingerprint))
}

pub fn get_node_identity() -> anyhow::Result<dos_crypto::NodeIdentity> {
    load_or_create_identity()
}
