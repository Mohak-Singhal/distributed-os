use crate::transport::TransportMode;
use crate::adaptive::{BottleneckReport, Bottleneck};

pub struct TransportSwitcher;

impl TransportSwitcher {
    pub fn select_transport(
        report: &BottleneckReport,
        file_size: u64,
        zero_copy_failed: bool,
        udp_handshake_failed: bool,
        rtt_ms: f64,
        packet_loss_pct: f64,
    ) -> (TransportMode, &'static str) {
        // Rule 1: Fallback if UDP/user-space handshake failed or packet loss is too high (>15%)
        if udp_handshake_failed {
            if !zero_copy_failed && file_size > 4 * 1024 * 1024 {
                return (TransportMode::TcpZeroCopy, "udp_failed_fallback_zero_copy");
            } else {
                return (TransportMode::TcpBuffered, "udp_failed_fallback_buffered");
            }
        }

        if packet_loss_pct > 15.0 {
            if !zero_copy_failed && file_size > 4 * 1024 * 1024 {
                return (TransportMode::TcpZeroCopy, "packet_loss_fallback_zero_copy");
            } else {
                return (TransportMode::TcpBuffered, "packet_loss_fallback_buffered");
            }
        }

        // Rule 2: If network bottleneck detected, select user-space protocols
        if report.bottleneck == Bottleneck::NetworkBound {
            if rtt_ms > 80.0 {
                return (TransportMode::Quic, "network_bound_high_rtt_quic");
            } else {
                return (TransportMode::UdpCustom, "network_bound_low_rtt_udp");
            }
        }

        // Rule 3: Default to TCP options
        if !zero_copy_failed && file_size > 4 * 1024 * 1024 {
            (TransportMode::TcpZeroCopy, "healthy_large_file_zero_copy")
        } else {
            (TransportMode::TcpBuffered, "healthy_standard_buffered")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_transport_switcher() {
        let mut report = BottleneckReport {
            timestamp_ms: 1000,
            bottleneck: Bottleneck::Healthy,
            confidence: 0.9,
            signals: vec![],
            scores: HashMap::new(),
        };

        // Case 1: Healthy baseline with small file -> TcpBuffered
        let (mode, reason) = TransportSwitcher::select_transport(&report, 1 * 1024 * 1024, false, false, 20.0, 0.0);
        assert_eq!(mode, TransportMode::TcpBuffered);
        assert_eq!(reason, "healthy_standard_buffered");

        // Case 2: Healthy baseline with large file -> TcpZeroCopy
        let (mode, reason) = TransportSwitcher::select_transport(&report, 10 * 1024 * 1024, false, false, 20.0, 0.0);
        assert_eq!(mode, TransportMode::TcpZeroCopy);
        assert_eq!(reason, "healthy_large_file_zero_copy");

        // Case 3: Network bound, high RTT -> Quic
        report.bottleneck = Bottleneck::NetworkBound;
        let (mode, reason) = TransportSwitcher::select_transport(&report, 10 * 1024 * 1024, false, false, 120.0, 1.0);
        assert_eq!(mode, TransportMode::Quic);
        assert_eq!(reason, "network_bound_high_rtt_quic");

        // Case 4: Network bound, low RTT -> UdpCustom
        let (mode, reason) = TransportSwitcher::select_transport(&report, 10 * 1024 * 1024, false, false, 30.0, 1.0);
        assert_eq!(mode, TransportMode::UdpCustom);
        assert_eq!(reason, "network_bound_low_rtt_udp");

        // Case 5: Network bound, but high packet loss -> Fallback to TcpZeroCopy
        let (mode, reason) = TransportSwitcher::select_transport(&report, 10 * 1024 * 1024, false, false, 30.0, 18.0);
        assert_eq!(mode, TransportMode::TcpZeroCopy);
        assert_eq!(reason, "packet_loss_fallback_zero_copy");

        // Case 6: Handshake failed -> Fallback to TcpZeroCopy
        let (mode, reason) = TransportSwitcher::select_transport(&report, 10 * 1024 * 1024, false, true, 30.0, 1.0);
        assert_eq!(mode, TransportMode::TcpZeroCopy);
        assert_eq!(reason, "udp_failed_fallback_zero_copy");
    }
}
