//! Error types for the `dos-crypto` crate.

use thiserror::Error;

/// Errors arising from cryptographic operations.
#[derive(Debug, Error)]
pub enum CryptoError {
    /// The provided key bytes were invalid.
    #[error("invalid key material")]
    InvalidKey,

    /// Signature verification failed.
    #[error("signature verification failed")]
    SignatureInvalid,

    /// The public key bytes could not be decoded from hex.
    #[error("hex decode error: {0}")]
    HexDecode(String),
}
