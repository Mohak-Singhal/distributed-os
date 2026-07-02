//! Pluggable transport layer: TCP, QUIC, and future transports.
//!
//! Architecture:
//! - `Transport` trait: connect, listen, send, recv, close
//! - `TcpTransport`: wraps existing `reliable.rs` + HTTP streaming
//! - `QuicTransport`: QUIC via `quinn` (feature `quic`)
//! - `select_best_transport`: negotiated during handshake

pub mod core;
pub mod selection;
pub mod tofu;

#[cfg(feature = "quic")]
pub mod quic;
#[cfg(feature = "tls")]
pub mod tls;

pub use core::{Transport, TcpTransport};
pub use selection::{select_best_transport, TransportPreference};

/// Identifies the wire transport protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransportMode {
    TcpBuffered,
    TcpZeroCopy,
    #[cfg(feature = "quic")]
    Quic,
    UdpCustom,
}

impl Default for TransportMode {
    fn default() -> Self {
        TransportMode::TcpBuffered
    }
}

impl TransportMode {
    /// Human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            TransportMode::TcpBuffered => "tcp-buffered",
            TransportMode::TcpZeroCopy => "tcp-zerocopy",
            #[cfg(feature = "quic")]
            TransportMode::Quic => "quic",
            TransportMode::UdpCustom => "udp-custom",
        }
    }

    /// Whether this transport provides built-in reliability.
    pub fn is_reliable(self) -> bool {
        match self {
            TransportMode::TcpBuffered | TransportMode::TcpZeroCopy => true,
            #[cfg(feature = "quic")]
            TransportMode::Quic => true,
            TransportMode::UdpCustom => false,
        }
    }

    /// Whether this transport supports multiplexed streams.
    pub fn supports_multiplexing(self) -> bool {
        #[cfg(feature = "quic")]
        { matches!(self, TransportMode::Quic) }
        #[cfg(not(feature = "quic"))]
        { false }
    }
}
