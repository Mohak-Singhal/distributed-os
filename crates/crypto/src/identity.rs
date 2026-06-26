//! Node identity: ed25519 key pair + derived node ID.

use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use uuid::Uuid;

use crate::CryptoError;

/// The cryptographic identity of a node.
///
/// Generated once on first boot, then persisted. The `node_id` is a
/// deterministic UUID derived from the public key bytes (UUID v5 / SHA-1
/// namespace `dns`), ensuring stability across restarts.
pub struct NodeIdentity {
    signing_key: SigningKey,
    /// The public half of the key pair. Safe to share.
    pub verifying_key: VerifyingKey,
    /// Stable node ID derived from `verifying_key`.
    pub node_id: Uuid,
}

impl NodeIdentity {
    /// Generate a brand-new identity using OS entropy.
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let node_id = Self::derive_id(&verifying_key);
        Self { signing_key, verifying_key, node_id }
    }

    /// Restore an identity from a 32-byte raw signing key.
    ///
    /// # Errors
    /// Returns [`CryptoError::InvalidKey`] if the bytes are not a valid scalar.
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self, CryptoError> {
        let signing_key = SigningKey::from_bytes(bytes);
        let verifying_key = signing_key.verifying_key();
        let node_id = Self::derive_id(&verifying_key);
        Ok(Self { signing_key, verifying_key, node_id })
    }

    /// Export the signing key as raw bytes for persistence.
    ///
    /// **Keep these bytes secret.** Store in an OS keychain or encrypted file.
    pub fn to_signing_key_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    /// Hex-encode the public (verifying) key for use in [`dos_core::Node`].
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.verifying_key.to_bytes())
    }

    /// Sign a message, returning a 64-byte signature.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        use ed25519_dalek::Signer;
        self.signing_key.sign(message).to_bytes()
    }

    /// Verify a signature against this identity's public key.
    ///
    /// # Errors
    /// Returns [`CryptoError::SignatureInvalid`] if verification fails.
    pub fn verify(&self, message: &[u8], signature_bytes: &[u8; 64]) -> Result<(), CryptoError> {
        use ed25519_dalek::{Signature, Verifier};
        let sig = Signature::from_bytes(signature_bytes);
        self.verifying_key
            .verify(message, &sig)
            .map_err(|_| CryptoError::SignatureInvalid)
    }

    fn derive_id(key: &VerifyingKey) -> Uuid {
        Uuid::new_v5(&Uuid::NAMESPACE_DNS, key.as_bytes())
    }
}

impl std::fmt::Debug for NodeIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeIdentity")
            .field("node_id", &self.node_id)
            .field("public_key", &self.public_key_hex())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_is_deterministic_from_bytes() {
        let identity = NodeIdentity::generate();
        let bytes = identity.to_signing_key_bytes();
        let restored = NodeIdentity::from_bytes(&bytes).expect("restore");
        assert_eq!(identity.node_id, restored.node_id);
        assert_eq!(identity.public_key_hex(), restored.public_key_hex());
    }

    #[test]
    fn sign_and_verify() {
        let identity = NodeIdentity::generate();
        let message = b"hello distributed os";
        let sig = identity.sign(message);
        assert!(identity.verify(message, &sig).is_ok());
    }

    #[test]
    fn verify_bad_sig_fails() {
        let identity = NodeIdentity::generate();
        let bad_sig = [0u8; 64];
        assert!(identity.verify(b"msg", &bad_sig).is_err());
    }
}
