//! Transport selection algorithm.
//!
//! During handshake, peers exchange supported transports. The decision
//! logic prefers QUIC when both support it and network conditions
//! are favorable, falling back to TCP on failure.

use crate::handshake::HandshakePayload;
use crate::transport::TransportMode;

/// Flags exchanged during handshake indicating transport support.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransportSupport {
    /// Whether this peer supports TCP.
    pub tcp: bool,
    /// Whether this peer supports QUIC.
    pub quic: bool,
    /// Whether this peer supports UDP (custom, no reliability).
    pub udp_custom: bool,
}

impl Default for TransportSupport {
    fn default() -> Self {
        Self {
            tcp: true,
            quic: cfg!(feature = "quic"),
            udp_custom: false,
        }
    }
}

/// Result of the transport selection process.
#[derive(Debug, Clone)]
pub struct TransportPreference {
    /// The chosen transport mode.
    pub mode: TransportMode,
    /// Reason for the selection.
    pub reason: String,
    /// Whether a fallback to another transport is available.
    pub fallback_available: bool,
    /// The fallback transport mode if the primary fails.
    pub fallback_mode: Option<TransportMode>,
}

/// Select the best transport based on both peers' capabilities.
///
/// Algorithm:
/// 1. If both support QUIC → prefer QUIC (faster, safer, connection migration)
/// 2. If both support TCP → TCP is the reliable fallback
/// 3. If only one transport is common → use that
/// 4. If none → error
pub fn select_best_transport(local: &HandshakePayload, remote: &HandshakePayload) -> TransportPreference {
    let local_ts = extract_transport_support(local);
    let remote_ts = extract_transport_support(remote);

    // QUIC is preferred when both support it (only available with `quic` feature)
    #[cfg(feature = "quic")]
    if local_ts.quic && remote_ts.quic {
        return TransportPreference {
            mode: TransportMode::Quic,
            reason: "Both peers support QUIC — preferred for faster connection and loss recovery".into(),
            fallback_available: local_ts.tcp && remote_ts.tcp,
            fallback_mode: if local_ts.tcp && remote_ts.tcp {
                Some(TransportMode::TcpBuffered)
            } else {
                None
            },
        };
    }

    // TCP fallback
    if local_ts.tcp && remote_ts.tcp {
        return TransportPreference {
            mode: TransportMode::TcpBuffered,
            reason: "TCP selected (QUIC not available on one or both peers)".into(),
            fallback_available: false,
            fallback_mode: None,
        };
    }

    // Last resort
    TransportPreference {
        mode: TransportMode::TcpBuffered,
        reason: "TCP fallback (default)".into(),
        fallback_available: false,
        fallback_mode: None,
    }
}

/// Select transport after a QUIC failure, falling back to TCP.
pub fn fallback_on_failure(failed_mode: TransportMode) -> TransportPreference {
    match failed_mode {
        #[cfg(feature = "quic")]
        TransportMode::Quic => TransportPreference {
            mode: TransportMode::TcpBuffered,
            reason: "QUIC connection failed — falling back to TCP".into(),
            fallback_available: false,
            fallback_mode: None,
        },
        _ => TransportPreference {
            mode: TransportMode::TcpBuffered,
            reason: "Transport failed — using TCP".into(),
            fallback_available: false,
            fallback_mode: None,
        },
    }
}

/// Extract transport support from a handshake payload.
fn extract_transport_support(payload: &HandshakePayload) -> TransportSupport {
    payload.transport_support.clone().unwrap_or_else(|| TransportSupport {
        tcp: true,
        quic: false,
        udp_custom: false,
    })
}
