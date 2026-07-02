//! User-meaningful error types for the transfer engine.
//!
//! Wraps internal errors into messages that users can act on.

use std::fmt;

/// User-facing transfer error with actionable messages.
#[derive(Debug, Clone)]
pub struct TransferError {
    /// Category of the error.
    pub kind: ErrorKind,
    /// Human-readable message suitable for UI display.
    pub message: String,
    /// Underlying cause (for logs, not for users).
    pub cause: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Peer is not reachable (wrong IP, firewall, offline).
    PeerUnreachable,
    /// Network is unstable (high loss, intermittent).
    NetworkUnstable,
    /// Connection timed out.
    Timeout,
    /// Transfer was cancelled by user or peer.
    Cancelled,
    /// File not found or inaccessible.
    FileNotFound,
    /// Disk full or permission denied.
    StorageError,
    /// Protocol version mismatch.
    IncompatiblePeer,
    /// Transfer is too large or system resource exhausted.
    ResourceExhausted,
    /// Peer doesn't support the requested feature.
    FeatureNotSupported,
    /// File integrity check failed (checksum mismatch).
    ChecksumMismatch,
    /// Internal engine error.
    Internal,
}

impl fmt::Display for TransferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for TransferError {}

impl TransferError {
    pub fn peer_unreachable(detail: impl Into<String>) -> Self {
        let d: String = detail.into();
        Self {
            kind: ErrorKind::PeerUnreachable,
            message: format!("Peer is not reachable. Make sure both devices are on the same network and the app is running. ({})", d),
            cause: Some(d),
        }
    }

    pub fn network_unstable(detail: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::NetworkUnstable,
            message: "Network connection is unstable. Try switching to a different network or moving closer to the hotspot.".into(),
            cause: Some(detail.into()),
        }
    }

    pub fn timeout(detail: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Timeout,
            message: "Connection timed out. The peer took too long to respond.".into(),
            cause: Some(detail.into()),
        }
    }

    pub fn cancelled(detail: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Cancelled,
            message: format!("Transfer cancelled: {}", detail.into()),
            cause: None,
        }
    }

    pub fn file_not_found(detail: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::FileNotFound,
            message: format!("File not found: {}", detail.into()),
            cause: None,
        }
    }

    pub fn storage_error(detail: impl Into<String>) -> Self {
        let d: String = detail.into();
        Self {
            kind: ErrorKind::StorageError,
            message: format!("Storage error: {}", d),
            cause: Some(d),
        }
    }

    pub fn incompatible(detail: impl Into<String>) -> Self {
        let d: String = detail.into();
        Self {
            kind: ErrorKind::IncompatiblePeer,
            message: format!("Peer is incompatible: {}", d),
            cause: Some(d),
        }
    }

    pub fn checksum_mismatch(detail: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::ChecksumMismatch,
            message: "File integrity check failed. The file may be corrupted.".into(),
            cause: Some(detail.into()),
        }
    }

    pub fn internal(detail: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Internal,
            message: "An unexpected error occurred. Please try again.".into(),
            cause: Some(detail.into()),
        }
    }

    /// Classify an `anyhow::Error` into a `TransferError`.
    pub fn from_anyhow(err: &anyhow::Error) -> Self {
        let msg = err.to_string();
        let lower = msg.to_lowercase();

        if lower.contains("connection refused") || lower.contains("unreachable") || lower.contains("no route to host") {
            Self::peer_unreachable(&msg)
        } else if lower.contains("timeout") || lower.contains("timed out") {
            Self::timeout(&msg)
        } else if lower.contains("cancelled") || lower.contains("cancel") {
            Self::cancelled(&msg)
        } else if lower.contains("not found") || lower.contains("no such file") {
            Self::file_not_found(&msg)
        } else if lower.contains("permission denied") || lower.contains("disk full") || lower.contains("storage") {
            Self::storage_error(&msg)
        } else if lower.contains("checksum") || lower.contains("integrity") || lower.contains("hash mismatch") {
            Self::checksum_mismatch(&msg)
        } else if lower.contains("network") || lower.contains("connection") || lower.contains("reset") || lower.contains("broken pipe") {
            Self::network_unstable(&msg)
        } else if lower.contains("incompatible") || lower.contains("version") {
            Self::incompatible(&msg)
        } else {
            Self::internal(&msg)
        }
    }
}

impl From<anyhow::Error> for TransferError {
    fn from(err: anyhow::Error) -> Self {
        Self::from_anyhow(&err)
    }
}
