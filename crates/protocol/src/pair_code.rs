//! Cryptographically-generated pair codes.
//!
//! A [`PairCode`] is a 6-character uppercase alphanumeric token displayed
//! to the user during manual device pairing. It is valid for
//! [`PAIR_CODE_TTL_SECS`] seconds after generation.
//!
//! Characters are drawn from the alphabet `ABCDEFGHJKLMNPQRSTUVWXYZ23456789`
//! (32 chars, no ambiguous I/O/0/1) so the code is easy to read aloud.

use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::error::ProtocolError;

/// Validity window for a pair code.
pub const PAIR_CODE_TTL_SECS: i64 = 300; // 5 minutes

/// Unambiguous characters for human-readable codes.
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// A 6-character uppercase alphanumeric device pairing token.
///
/// Generated with OS entropy, valid for 5 minutes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairCode {
    /// The token string (always 6 uppercase alphanumeric characters).
    pub code: String,
    /// Wall-clock time this code was generated.
    pub created_at: DateTime<Utc>,
}

impl PairCode {
    /// Generate a new pair code using OS entropy.
    pub fn generate() -> Self {
        let mut rng = rand::thread_rng();
        let code: String = (0..6)
            .map(|_| {
                let idx = rng.gen_range(0..ALPHABET.len());
                ALPHABET[idx] as char
            })
            .collect();
        Self { code, created_at: Utc::now() }
    }

    /// Returns `true` if this code is still within its validity window.
    pub fn is_valid(&self) -> bool {
        Utc::now() - self.created_at < Duration::seconds(PAIR_CODE_TTL_SECS)
    }

    /// Validate a user-entered string against this code.
    ///
    /// Comparison is case-insensitive to forgive mobile keyboard capitalisation.
    ///
    /// # Errors
    /// Returns [`ProtocolError::InvalidPairCode`] if the code has expired or
    /// does not match.
    pub fn verify(&self, input: &str) -> Result<(), ProtocolError> {
        if !self.is_valid() {
            return Err(ProtocolError::InvalidPairCode("pair code has expired".into()));
        }
        if !input.eq_ignore_ascii_case(&self.code) {
            return Err(ProtocolError::InvalidPairCode("pair code does not match".into()));
        }
        Ok(())
    }

    /// Returns the code string as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.code
    }
}

impl std::fmt::Display for PairCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_six_chars() {
        let code = PairCode::generate();
        assert_eq!(code.code.len(), 6);
    }

    #[test]
    fn only_alphabet_chars() {
        for _ in 0..50 {
            let code = PairCode::generate();
            for ch in code.code.chars() {
                assert!(
                    ALPHABET.contains(&(ch as u8)),
                    "unexpected char: {ch}"
                );
            }
        }
    }

    #[test]
    fn fresh_code_is_valid() {
        let code = PairCode::generate();
        assert!(code.is_valid());
    }

    #[test]
    fn verify_correct_code() {
        let code = PairCode::generate();
        let input = code.code.clone();
        assert!(code.verify(&input).is_ok());
    }

    #[test]
    fn verify_case_insensitive() {
        let code = PairCode::generate();
        let lower = code.code.to_lowercase();
        assert!(code.verify(&lower).is_ok());
    }

    #[test]
    fn verify_wrong_code_fails() {
        let code = PairCode::generate();
        assert!(code.verify("AAAAAA").is_err() || code.code == "AAAAAA");
    }

    #[test]
    fn expired_code_fails() {
        use chrono::Duration;
        let mut code = PairCode::generate();
        // backdate creation time beyond TTL
        code.created_at = Utc::now() - Duration::seconds(PAIR_CODE_TTL_SECS + 1);
        assert!(!code.is_valid());
        assert!(code.verify(&code.code.clone()).is_err());
    }
}
