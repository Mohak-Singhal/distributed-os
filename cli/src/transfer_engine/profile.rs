use super::network::{InterfaceType, NetworkInfo, RemoteCapabilities};
use super::file_analyzer::{FileAnalysis, FileCategory};
use super::system::SystemInfo;

#[derive(Debug, Clone)]
pub struct TransferProfile {
    pub interface: InterfaceType,
    pub rtt_ms: f64,
    pub bandwidth_mbps: f64,
    pub packet_loss_pct: f64,
    pub file_category: FileCategory,
    pub file_count: u32,
    pub total_bytes: u64,
    pub disk_type_label: String,
    pub cpu_cores: u32,
    pub supports_range: bool,
    pub supports_http2: bool,
    pub recommended_streams: u32,
    pub recommended_buffer_kb: u32,
    pub chunk_size: usize,
    pub socket_buffer_size: usize,
    pub strategy: TransferStrategy,
    pub integrity: IntegrityLevel,
    pub compression_enabled: bool,
    pub use_zero_copy: bool,
    pub pacing_bytes_per_sec: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransferStrategy {
    SingleStream,
    ParallelRanges,
    Batched,
    Pipelined,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntegrityLevel {
    None,
    Fast,     // crc32
    Standard, // sha256 after transfer
    Full,     // incremental sha256 during transfer
}

pub fn build_profile(net: &NetworkInfo, files: &FileAnalysis, sys: &SystemInfo, remote: &RemoteCapabilities) -> TransferProfile {
    let strategy = select_strategy(net, files, remote, sys);
    let streams = calc_stream_count(net, files, strategy);
    let buffer_kb = calc_buffer_size(net, sys);
    let chunk_size = (buffer_kb as usize * 1024).max(65536).min(16_777_216);
    let socket_buffer_size = (net.available_bandwidth_mbps as usize * 1_000_000 / 8).max(65536).min(16_777_216);
    let integrity = calc_integrity(files, sys);
    let compression_enabled = files.compression_ratio_estimate < 0.8 && files.total_size > 1_000_000;
    let paced_bps = (sys.disk_write_speed_mbps * 1_000_000.0 / 8.0) as u64;

    TransferProfile {
        interface: net.interface_type,
        rtt_ms: net.rtt_ms,
        bandwidth_mbps: net.available_bandwidth_mbps,
        packet_loss_pct: net.packet_loss_pct,
        file_category: files.category,
        file_count: files.count,
        total_bytes: files.total_size,
        disk_type_label: sys.disk_type.label().to_string(),
        cpu_cores: sys.cpu_cores,
        supports_range: remote.supports_range,
        supports_http2: remote.supports_http2,
        recommended_streams: streams,
        recommended_buffer_kb: buffer_kb,
        chunk_size,
        socket_buffer_size,
        strategy,
        integrity,
        compression_enabled,
        use_zero_copy: sys.supports_sendfile,
        pacing_bytes_per_sec: paced_bps,
    }
}

fn select_strategy(net: &NetworkInfo, files: &FileAnalysis, remote: &RemoteCapabilities, sys: &SystemInfo) -> TransferStrategy {
    match files.category {
        FileCategory::Tiny | FileCategory::Small => TransferStrategy::SingleStream,
        FileCategory::ManyTiny | FileCategory::ManySmall => TransferStrategy::Batched,
        FileCategory::Mixed => {
            if files.count < 10 { TransferStrategy::SingleStream }
            else { TransferStrategy::Batched }
        }
        FileCategory::Medium | FileCategory::Large | FileCategory::Huge | FileCategory::Massive => {
            if remote.supports_range && (net.packet_loss_pct > 1.0 || net.interface_type.label().contains("WiFi")) {
                TransferStrategy::ParallelRanges
            } else {
                TransferStrategy::SingleStream
            }
        }
    }
}

fn calc_stream_count(net: &NetworkInfo, _files: &FileAnalysis, strategy: TransferStrategy) -> u32 {
    match strategy {
        TransferStrategy::SingleStream | TransferStrategy::Batched | TransferStrategy::Pipelined => 1,
        TransferStrategy::ParallelRanges => {
            if net.packet_loss_pct > 5.0 { 8 }
            else if net.packet_loss_pct > 2.0 { 4 }
            else if net.interface_type == InterfaceType::WiFi24GHz { 4 }
            else if net.interface_type == InterfaceType::WiFi5GHz { 4 }
            else if net.interface_type == InterfaceType::WiFi6 { 4 }
            else if net.interface_type == InterfaceType::WiFi7 { 6 }
            else { 2 }
        }
    }
}

fn calc_buffer_size(net: &NetworkInfo, sys: &SystemInfo) -> u32 {
    // BDP = bandwidth * RTT. Buffer should be at least 2x BDP per stream.
    let bdp_bytes = (net.available_bandwidth_mbps * 1_000_000.0 / 8.0 * net.rtt_ms / 1000.0) as u64;
    let mut buf = (bdp_bytes / 1024).max(64).min(16_384) as u32; // 64KB – 16MB

    // Cap by disk speed for NVMe vs HDD
    if sys.disk_type.label().contains("HDD") {
        buf = buf.min(512);
    }

    buf
}

fn calc_integrity(files: &FileAnalysis, _sys: &SystemInfo) -> IntegrityLevel {
    match files.category {
        FileCategory::Tiny => IntegrityLevel::Full,
        FileCategory::Small => IntegrityLevel::Full,
        FileCategory::Medium => IntegrityLevel::Full,
        FileCategory::Large => IntegrityLevel::Full,
        FileCategory::Huge | FileCategory::Massive => IntegrityLevel::Full,
        FileCategory::ManyTiny | FileCategory::ManySmall => IntegrityLevel::Fast,
        FileCategory::Mixed => IntegrityLevel::Standard,
    }
}
