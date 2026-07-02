#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransportMode {
    TcpBuffered,
    TcpZeroCopy,
    Quic,
    UdpCustom,
}

impl Default for TransportMode {
    fn default() -> Self {
        TransportMode::TcpBuffered
    }
}
