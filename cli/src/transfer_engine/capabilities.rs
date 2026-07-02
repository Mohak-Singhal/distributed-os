use serde::{Deserialize, Serialize};

/// Full capability exchange: sent by both sides during handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityExchange {
    pub protocol_version: String,
    pub node_id: String,
    pub hardware: HardwareCapabilities,
    pub network: NetworkCapabilities,
    pub state: DynamicTelemetry,
    pub features: SupportedFeatures,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareCapabilities {
    pub cpu_architecture: String,          // arm64, x86_64
    pub cpu_cores: u32,                    // total logical cores
    pub cpu_performance_cores: u32,        // P-cores (0 if uniform)
    pub cpu_efficiency_cores: u32,         // E-cores (0 if uniform)
    pub ram_mb: u64,
    pub storage_type: String,              // nvme, ssd, hdd, ufs, sd_card, unknown
    pub storage_read_mbps: f64,            // measured sequential read
    pub storage_write_mbps: f64,           // measured sequential write
    pub storage_free_gb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkCapabilities {
    pub interface_type: String,            // WiFi6, WiFi5, Ethernet1G, Thunderbolt, etc.
    pub link_speed_mbps: f64,              // nominal link speed
    pub rtt_ms: f64,                       // measured round-trip time
    pub measured_bandwidth_mbps: f64,      // measured TCP throughput
    pub mtu: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicTelemetry {
    pub battery_pct: f64,
    pub charging: bool,
    pub thermal_state: String,             // nominal, fair, serious, critical
    pub cpu_load_pct: f64,
    pub memory_pressure: String,           // low, medium, high, critical
    pub disk_utilization_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupportedFeatures {
    pub zero_copy: bool,
    pub parallel_upload: bool,
    pub parallel_download: bool,
    pub resume: bool,
    pub streaming_directory: bool,
    pub compression: Vec<String>,          // "none", "zstd", "gzip"
    pub integrity: Vec<String>,            // "crc32", "sha256"
    pub http2: bool,
    pub http3: bool,
}

impl CapabilityExchange {
    /// Build a capability exchange from local probes.
    pub async fn local() -> Self {
        let sys = super::system::probe_system().await;
        let hw = HardwareCapabilities {
            cpu_architecture: std::env::consts::ARCH.to_string(),
            cpu_cores: sys.cpu_cores,
            cpu_performance_cores: sys.cpu_cores,  // simplified
            cpu_efficiency_cores: 0,
            ram_mb: sys.ram_mb,
            storage_type: sys.disk_type.label().to_lowercase().replace(' ', "_"),
            storage_read_mbps: sys.disk_read_speed_mbps.max(1.0),
            storage_write_mbps: sys.disk_write_speed_mbps.max(1.0),
            storage_free_gb: sys.disk_free_gb,
        };

        let net_interface = super::network::classify_interface_local();
        let net = NetworkCapabilities {
            interface_type: net_interface.0,
            link_speed_mbps: net_interface.1,
            rtt_ms: 0.0,   // filled by probe_network
            measured_bandwidth_mbps: 0.0, // filled by probe_network
            mtu: 1500,
        };

        let state = collect_telemetry().await;

        let features = SupportedFeatures {
            zero_copy: sys.supports_sendfile,
            parallel_upload: true,
            parallel_download: false,
            resume: true,
            streaming_directory: true,
            compression: vec!["none".into()],
            integrity: vec!["sha256".into()],
            http2: false,
            http3: false,
        };

        let node_id = crate::tls::get_node_identity()
            .map(|id| id.node_id.to_string())
            .unwrap_or_else(|_| "unknown".into());

        Self {
            protocol_version: "1.0".into(),
            node_id,
            hardware: hw,
            network: net,
            state,
            features,
        }
    }
}

/// Collect current dynamic telemetry from the system.
pub async fn collect_telemetry() -> DynamicTelemetry {
    let thermal = crate::telemetry::THERMAL_STATE
        .lock()
        .map(|t| t.thermal_state.clone())
        .unwrap_or_else(|_| "nominal".into());

    let cpu_load = crate::system_monitor::CURRENT_METRICS
        .lock()
        .ok()
        .and_then(|m| m.get("cpu_usage").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    DynamicTelemetry {
        battery_pct: 100.0,
        charging: true,
        thermal_state: thermal,
        cpu_load_pct: cpu_load,
        memory_pressure: "low".into(),
        disk_utilization_pct: 0.0,
    }
}

/// Perform a full handshake with a remote peer.
/// Sends our capabilities and receives theirs.
pub async fn perform_handshake(
    host: &str,
    port: u16,
    our_caps: &CapabilityExchange,
) -> anyhow::Result<CapabilityExchange> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let mut stream = TcpStream::connect(format!("{}:{}", host, port)).await?;
    let body = serde_json::to_string(our_caps)?;

    let request = format!(
        "POST /api/handshake HTTP/1.1\r\n\
         Host: {}:{}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        host, port, body.len(), body
    );

    stream.write_all(request.as_bytes()).await?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    let resp = String::from_utf8_lossy(&buf);

    if let Some(body_start) = resp.find("\r\n\r\n") {
        let body = &resp[body_start + 4..];
        if resp.contains("200 OK") {
            let remote: CapabilityExchange = serde_json::from_str(body)
                .map_err(|e| anyhow::anyhow!("Handshake parse error: {} — body: {}", e, body.chars().take(200).collect::<String>()))?;
            return Ok(remote);
        }
    }
    Err(anyhow::anyhow!("Handshake failed: {}", resp.lines().next().unwrap_or("?")))
}

impl std::fmt::Display for CapabilityExchange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,
            "{} | {} cores/{}GB RAM | {} {} ({} Mbps) | battery:{:.0}% thermal:{}",
            self.node_id.chars().take(8).collect::<String>(),
            self.hardware.cpu_cores,
            self.hardware.ram_mb / 1024,
            self.hardware.storage_type,
            self.hardware.storage_write_mbps as u64,
            self.network.link_speed_mbps as u64,
            self.state.battery_pct,
            self.state.thermal_state,
        )
    }
}
