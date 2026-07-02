use std::time::Instant;
use tokio::net::TcpStream;
use tokio::io::AsyncWriteExt;

/// Interface type classification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InterfaceType {
    Ethernet100M,
    Ethernet1G,
    Ethernet10G,
    Ethernet25G,
    Ethernet40G,
    Ethernet100G,
    WiFi24GHz,
    WiFi5GHz,
    WiFi6,
    WiFi7,
    Thunderbolt,
    UsbEthernet,
    Vpn,
    Wan,
    Loopback,
    Unknown(f64), // measured bandwidth in Mbps
}

#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub interface_type: InterfaceType,
    pub rtt_ms: f64,
    pub available_bandwidth_mbps: f64,
    pub jitter_ms: f64,
    pub packet_loss_pct: f64,
    pub mtu: u16,
    pub is_congested: bool,
    pub is_duplex: bool,
}

#[derive(Debug, Clone)]
pub struct RemoteCapabilities {
    pub supports_range: bool,
    pub supports_http2: bool,
    pub supports_http3: bool,
    pub supports_quic: bool,
    pub max_parallel_connections: u16,
    pub server_load_pct: f64,
    pub tls_version: String,
}

/// Probe the network path to a remote host.
pub async fn probe_network(host: &str, port: u16) -> NetworkInfo {
    let mut rtts = Vec::with_capacity(5);
    let mut jitter_samples = Vec::new();

    // RTT measurement via TCP connect + single byte echo
    for _ in 0..5 {
        let start = Instant::now();
        if let Ok(mut stream) = TcpStream::connect(format!("{}:{}", host, port)).await {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            rtts.push(elapsed);
            let _ = stream.shutdown().await;

            // Quick bandwidth probe: try to send a small payload
            if let Ok(mut s) = TcpStream::connect(format!("{}:{}", host, port)).await {
                let probe_start = Instant::now();
                let payload = vec![0u8; 65536];
                let _ = s.write_all(&payload).await;
                let probe_time = probe_start.elapsed().as_secs_f64();
                if probe_time > 0.0 {
                    jitter_samples.push((65536.0 * 8.0) / probe_time / 1_000_000.0);
                }
                let _ = s.shutdown().await;
            }
        }
    }

    if rtts.is_empty() {
        return NetworkInfo {
            interface_type: InterfaceType::Unknown(0.0),
            rtt_ms: 0.0,
            available_bandwidth_mbps: 0.0,
            jitter_ms: 0.0,
            packet_loss_pct: 0.0,
            mtu: 1500,
            is_congested: false,
            is_duplex: true,
        };
    }

    let avg_rtt = rtts.iter().sum::<f64>() / rtts.len() as f64;
    let jitter = if jitter_samples.len() > 1 {
        let mean = jitter_samples.iter().sum::<f64>() / jitter_samples.len() as f64;
        jitter_samples.iter().map(|v| (v - mean).abs()).sum::<f64>() / jitter_samples.len() as f64
    } else {
        0.0
    };

    let bw_estimate = jitter_samples.iter().copied().fold(f64::MAX, f64::min);

    let interface_type = classify_interface(bw_estimate, avg_rtt);

    // Estimate packet loss from RTT variance
    let rtt_var = if rtts.len() > 1 {
        let mean = rtts.iter().sum::<f64>() / rtts.len() as f64;
        let variance = rtts.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / rtts.len() as f64;
        variance.sqrt()
    } else {
        0.0
    };
    let loss_estimate = (rtt_var / avg_rtt.max(1.0)).min(10.0);

    NetworkInfo {
        interface_type,
        rtt_ms: avg_rtt,
        available_bandwidth_mbps: bw_estimate,
        jitter_ms: jitter,
        packet_loss_pct: loss_estimate,
        mtu: 1500,
        is_congested: loss_estimate > 2.0 || jitter > 10.0,
        is_duplex: true,
    }
}

fn classify_interface(bw_mbps: f64, rtt_ms: f64) -> InterfaceType {
    if rtt_ms < 0.1 {
        return InterfaceType::Loopback;
    }
    if rtt_ms < 2.0 {
        // Wired / Thunderbolt — infer from bandwidth
        if bw_mbps > 40000.0 { return InterfaceType::Thunderbolt; }
        if bw_mbps > 10000.0 { return InterfaceType::Ethernet10G; }
        if bw_mbps > 1000.0 { return InterfaceType::Ethernet1G; }
        if bw_mbps > 100.0 { return InterfaceType::Ethernet1G; }
        return InterfaceType::Ethernet100M;
    }
    if rtt_ms < 15.0 {
        // WiFi — infer generation from bandwidth
        if bw_mbps > 2000.0 { return InterfaceType::WiFi7; }
        if bw_mbps > 1000.0 { return InterfaceType::WiFi6; }
        if bw_mbps > 400.0 { return InterfaceType::WiFi5GHz; }
        return InterfaceType::WiFi24GHz;
    }
    // WAN / VPN
    if bw_mbps < 100.0 { return InterfaceType::Wan; }
    InterfaceType::Vpn
}

impl InterfaceType {
    pub fn label(&self) -> &'static str {
        match self {
            InterfaceType::Ethernet100M => "100Mbps Ethernet",
            InterfaceType::Ethernet1G => "1Gbps Ethernet",
            InterfaceType::Ethernet10G => "10Gbps Ethernet",
            InterfaceType::Ethernet25G => "25Gbps Ethernet",
            InterfaceType::Ethernet40G => "40Gbps Ethernet",
            InterfaceType::Ethernet100G => "100Gbps Ethernet",
            InterfaceType::WiFi24GHz => "2.4GHz WiFi",
            InterfaceType::WiFi5GHz => "5GHz WiFi",
            InterfaceType::WiFi6 => "WiFi 6",
            InterfaceType::WiFi7 => "WiFi 7",
            InterfaceType::Thunderbolt => "Thunderbolt",
            InterfaceType::UsbEthernet => "USB Ethernet",
            InterfaceType::Vpn => "VPN",
            InterfaceType::Wan => "WAN",
            InterfaceType::Loopback => "Loopback",
            InterfaceType::Unknown(_) => "Unknown",
        }
    }

    pub fn expected_bandwidth_mbps(&self) -> f64 {
        match self {
            InterfaceType::Ethernet100M => 100.0,
            InterfaceType::Ethernet1G => 1000.0,
            InterfaceType::Ethernet10G => 10000.0,
            InterfaceType::Ethernet25G => 25000.0,
            InterfaceType::Ethernet40G => 40000.0,
            InterfaceType::Ethernet100G => 100000.0,
            InterfaceType::WiFi24GHz => 100.0,
            InterfaceType::WiFi5GHz => 500.0,
            InterfaceType::WiFi6 => 1200.0,
            InterfaceType::WiFi7 => 2400.0,
            InterfaceType::Thunderbolt => 40000.0,
            InterfaceType::UsbEthernet => 1000.0,
            InterfaceType::Vpn => 50.0,
            InterfaceType::Wan => 100.0,
            InterfaceType::Loopback => f64::MAX,
            InterfaceType::Unknown(bw) => *bw,
        }
    }
}

pub async fn probe_remote_capabilities(host: &str, port: u16, path: &str) -> RemoteCapabilities {
    // First try full handshake
    if let Some(caps) = try_full_handshake(host, port).await {
        return caps;
    }
    // Fall back to capabilities API
    if let Some(caps) = fetch_capabilities_api(host, port).await {
        return caps;
    }
    // Last resort: probe HEAD
    let supports_range = probe_http_range(host, port, path).await;
    RemoteCapabilities {
        supports_range,
        supports_http2: false,
        supports_http3: false,
        supports_quic: false,
        max_parallel_connections: 8,
        server_load_pct: 0.0,
        tls_version: "none".into(),
    }
}

async fn try_full_handshake(host: &str, port: u16) -> Option<RemoteCapabilities> {
    let local_caps = super::capabilities::CapabilityExchange::local().await;
    match super::capabilities::perform_handshake(host, port, &local_caps).await {
        Ok(remote) => {
            let supports_range = remote.features.resume;
            let max_conns = if remote.features.parallel_upload { 8 } else { 1 };
            Some(RemoteCapabilities {
                supports_range,
                supports_http2: remote.features.http2,
                supports_http3: remote.features.http3,
                supports_quic: remote.features.http3,
                max_parallel_connections: max_conns,
                server_load_pct: remote.state.cpu_load_pct,
                tls_version: "handshake".into(),
            })
        }
        Err(_) => None,
    }
}

async fn fetch_capabilities_api(host: &str, port: u16) -> Option<RemoteCapabilities> {
    use tokio::io::AsyncReadExt;
    if let Ok(mut stream) = TcpStream::connect(format!("{}:{}", host, port)).await {
        let request = format!(
            "GET /api/capabilities HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n\r\n",
            host, port
        );
        let _ = stream.write_all(request.as_bytes()).await;
        let mut buf = vec![0u8; 4096];
        if let Ok(n) = stream.read(&mut buf).await {
            let resp = String::from_utf8_lossy(&buf[..n]);
            if let Some(body_start) = resp.find("\r\n\r\n") {
                let body = &resp[body_start + 4..];
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
                    return Some(RemoteCapabilities {
                        supports_range: v.get("supports_range").and_then(|b| b.as_bool()).unwrap_or(false),
                        supports_http2: v.get("supports_http2").and_then(|b| b.as_bool()).unwrap_or(false),
                        supports_http3: false,
                        supports_quic: false,
                        max_parallel_connections: v.get("max_parallel_connections").and_then(|n| n.as_u64()).unwrap_or(8) as u16,
                        server_load_pct: 0.0,
                        tls_version: "none".into(),
                    });
                }
            }
        }
    }
    None
}

async fn probe_http_range(host: &str, port: u16, path: &str) -> bool {
    use tokio::io::AsyncReadExt;
    if let Ok(mut stream) = TcpStream::connect(format!("{}:{}", host, port)).await {
        let request = format!(
            "HEAD {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            path, host
        );
        let _ = stream.write_all(request.as_bytes()).await;
        let mut buf = vec![0u8; 4096];
        if let Ok(n) = stream.read(&mut buf).await {
            let resp = String::from_utf8_lossy(&buf[..n]);
            return resp.to_lowercase().contains("accept-ranges: bytes")
                || resp.to_lowercase().contains("content-range");
        }
    }
    false
}

/// Classify the local network interface based on system info.
pub fn classify_interface_local() -> (String, f64) {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("networksetup")
            .args(["-getairportnetwork", "en0"])
            .output()
        {
            let s = String::from_utf8_lossy(&output.stdout);
            if s.contains("Wi-Fi") || s.contains("AirPort") {
                // Check link speed via system_profiler
                if let Ok(sp) = std::process::Command::new("system_profiler")
                    .args(["SPAirPortDataType"])
                    .output()
                {
                    let sp_s = String::from_utf8_lossy(&sp.stdout);
                    if sp_s.contains("ac") { return ("WiFi5".into(), 500.0); }
                    if sp_s.contains("ax") { return ("WiFi6".into(), 1200.0); }
                    if sp_s.contains("be") { return ("WiFi7".into(), 2400.0); }
                    return ("WiFi".into(), 300.0);
                }
            }
        }
        // Check Thunderbolt bridge
        if let Ok(output) = std::process::Command::new("ifconfig")
            .arg("bridge0")
            .output()
        {
            let s = String::from_utf8_lossy(&output.stdout);
            if s.contains("inet ") {
                return ("Thunderbolt".into(), 40000.0);
            }
        }
        // Default to Ethernet
        ("Ethernet1G".into(), 1000.0)
    }
    #[cfg(target_os = "linux")]
    {
        ("Ethernet1G".into(), 1000.0)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        ("Unknown".into(), 100.0)
    }
}
