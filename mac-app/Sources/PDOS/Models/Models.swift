import Foundation

struct SystemMetrics: Codable, Equatable {
    let cpu_usage: String?
    let memory_mb: String?
    let total_memory_mb: String?
    let used_memory_mb: String?
    let global_cpu: String?
    let network_rx_mbps: String?
    let network_tx_mbps: String?
    let disks: [DiskInfo]?
    let interfaces: [InterfaceInfo]?
    let thermal: ThermalInfo?
    let processes: [String: ProcessMetrics]?
    let protocol_stats: ProtocolStats?
    let logs: [String]?
}

struct DiskInfo: Codable, Identifiable, Equatable {
    var id: String { name }
    let name: String
    let mount: String
    let total_gb: Double
    let available_gb: Double
    let file_system: String?
}

struct InterfaceInfo: Codable, Identifiable, Equatable {
    var id: String { name }
    let name: String
    let mac: String
    let rx: Int
    let tx: Int
}

struct ThermalInfo: Codable, Equatable {
    let cpu_temp_c: Double?
    let thermal_state: String?
    let fan_rpm: Double?
    let battery_pct: Double?
    let battery_temp_c: Double?
    let thermal_status: String?
}

struct ProcessMetrics: Codable, Equatable {
    let cpu: Double
    let ram_mb: Double
}

struct ProtocolStats: Codable, Equatable {
    let auth_requests: Int
    let cancelled_transfers: Int
    let completed_transfers: Int
    let discovery_packets: Int
    let failed_transfers: Int
    let range_requests: Int
    let resume_requests: Int
    let tls_handshakes: Int
    let transfer_requests: Int
}

struct HealthScore: Codable, Equatable {
    let overall: Double
    let cpu: Double
    let disk: Double
    let network: Double
    let integrity: Double
    let recovery: Double
}

struct TransferSession: Codable, Identifiable {
    let id: String?
    let filename: String?
    let file_size: UInt64?
    let compressed_size: UInt64?
    let compression_ratio: Double?
    let compression_time_ms: UInt64?
    let cpu_used_compression: Double?
    let bandwidth_saved: UInt64?
    let transfer_speed_mbps: Double?
    let status: String?
    let timestamp: String?
    let sha256: String?

    var displayId: String { id ?? UUID().uuidString }
    var displayRatio: String {
        guard let r = compression_ratio else { return "N/A" }
        return String(format: "%.2f", r)
    }
    var displaySpeed: String {
        guard let s = transfer_speed_mbps else { return "--" }
        return String(format: "%.1f Mbps", s)
    }
    var displaySize: String {
        guard let s = file_size else { return "--" }
        if s > 1_048_576 { return String(format: "%.1f MB", Double(s) / 1_048_576) }
        if s > 1024 { return String(format: "%.1f KB", Double(s) / 1024) }
        return "\(s) B"
    }
}

struct Bottleneck: Codable, Equatable {
    let bottleneck: String?
    let recommendation: String?
}

struct BufferAnalysis: Codable {
    let read_buffer_kb: Int
    let write_buffer_kb: Int
    let average_queue_depth: Double
    let max_queue_depth: Int
    let backpressure_events: Int
}

struct TransferProgress: Codable {
    let active: Bool
    let filename: String
    let bytes_sent: UInt64
    let total_bytes: UInt64
    let progress_pct: Double
    let speed_mbps: Double
    let status: String

    var displaySpeed: String {
        if speed_mbps > 1000 { return String(format: "%.1f Gbps", speed_mbps / 1000) }
        if speed_mbps > 1 { return String(format: "%.1f Mbps", speed_mbps) }
        return String(format: "%.0f Kbps", speed_mbps * 1000)
    }
    var displaySize: String {
        if bytes_sent > 1_048_576 { return String(format: "%.1f MB", Double(bytes_sent) / 1_048_576) }
        if bytes_sent > 1024 { return String(format: "%.1f KB", Double(bytes_sent) / 1024) }
        return "\(bytes_sent) B"
    }
    var displayTotal: String {
        if total_bytes > 1_048_576 { return String(format: "%.1f MB", Double(total_bytes) / 1_048_576) }
        if total_bytes > 1024 { return String(format: "%.1f KB", Double(total_bytes) / 1024) }
        return "\(total_bytes) B"
    }
}

struct TrustedDevice: Codable, Identifiable, Equatable {
    let id: String
    let name: String
    let fingerprint: String
    var lastSeen: Date
    var autoAccept: Bool
    var allowedDirectories: [String]
}

struct StorageForecast: Codable {
    let enough_space: Bool
    let file_size_gb: Double
    let free_gb: Double
    let remaining_gb: Double
}

// MARK: - Capability Handshake Models

struct CapabilityExchange: Codable {
    let protocol_version: String
    let node_id: String
    let hardware: HardwareCapabilities
    let network: NetworkCapabilities
    let state: DynamicTelemetry
    let features: SupportedFeatures
}

struct HardwareCapabilities: Codable {
    let cpu_architecture: String
    let cpu_cores: Int
    let cpu_performance_cores: Int
    let cpu_efficiency_cores: Int
    let ram_mb: Int
    let storage_type: String
    let storage_read_mbps: Double
    let storage_write_mbps: Double
    let storage_free_gb: Double
}

struct NetworkCapabilities: Codable {
    let interface_type: String
    let link_speed_mbps: Double
    let rtt_ms: Double
    let measured_bandwidth_mbps: Double
    let mtu: Int
}

struct DynamicTelemetry: Codable {
    let battery_pct: Double
    let charging: Bool
    let thermal_state: String
    let cpu_load_pct: Double
    let memory_pressure: String
    let disk_utilization_pct: Double
}

struct SupportedFeatures: Codable {
    let zero_copy: Bool
    let parallel_upload: Bool
    let parallel_download: Bool
    let resume: Bool
    let streaming_directory: Bool
    let compression: [String]
    let integrity: [String]
    let http2: Bool
    let http3: Bool
}

struct SSEEvent: Codable {
    let progress: TransferProgress?
    let telemetry: SSEUpdateTelemetry?
}

struct SSEUpdateTelemetry: Codable {
    let cpu_load_pct: Double
    let thermal_state: String
    let battery_pct: Double
    let memory_pressure: String
}
