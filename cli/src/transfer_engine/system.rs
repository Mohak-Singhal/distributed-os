use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub cpu_cores: u32,
    pub cpu_usage_pct: f64,
    pub ram_mb: u64,
    pub ram_available_mb: u64,
    pub disk_type: DiskType,
    pub disk_read_speed_mbps: f64,
    pub disk_write_speed_mbps: f64,
    pub disk_free_gb: f64,
    pub disk_total_gb: f64,
    pub disk_queue_depth: u32,
    pub supports_sendfile: bool,
    pub supports_splice: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DiskType {
    Nvme,
    Ssd,
    Hdd,
    Network,
    Unknown,
}

pub async fn probe_system() -> SystemInfo {
    let cpu_cores = num_cpus::get() as u32;
    let ram_mb = total_ram_mb();
    let ram_avail = available_ram_mb();
    let disk_type = detect_disk_type();
    let disk_free = free_disk_gb();
    let disk_total = total_disk_gb();

    SystemInfo {
        cpu_cores,
        cpu_usage_pct: 0.0,
        ram_mb,
        ram_available_mb: ram_avail,
        disk_type,
        disk_read_speed_mbps: 0.0,
        disk_write_speed_mbps: 0.0,
        disk_free_gb: disk_free.0,
        disk_total_gb: disk_total.0,
        disk_queue_depth: 0,
        supports_sendfile: true,
        supports_splice: cfg!(target_os = "linux"),
    }
}

fn total_ram_mb() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    (sys.total_memory() / 1024 / 1024) as u64
}

fn available_ram_mb() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    (sys.available_memory() / 1024 / 1024) as u64
}

fn detect_disk_type() -> DiskType {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("diskutil")
            .args(["info", "/"])
            .output()
        {
            let s = String::from_utf8_lossy(&output.stdout);
            if s.contains("Solid State") || s.contains("NVMe") {
                if s.contains("NVMe") { return DiskType::Nvme; }
                return DiskType::Ssd;
            }
            if s.contains("Rotational") { return DiskType::Hdd; }
        }
        DiskType::Ssd // modern Macs are all SSD
    }
    #[cfg(target_os = "linux")]
    {
        let rotational = std::fs::read_to_string("/sys/block/sda/queue/rotational").unwrap_or_default();
        match rotational.trim() {
            "0" => DiskType::Ssd,
            "1" => DiskType::Hdd,
            _ => DiskType::Unknown,
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    { DiskType::Unknown }
}

fn free_disk_gb() -> (f64, f64) {
    let mut disks = sysinfo::Disks::new_with_refreshed_list();
    disks.refresh(true);
    for d in &disks {
        if d.mount_point() == std::path::Path::new("/") {
            let free = d.available_space() as f64 / 1e9;
            let total = d.total_space() as f64 / 1e9;
            return (free, total);
        }
    }
    (0.0, 0.0)
}

fn total_disk_gb() -> (f64, f64) {
    free_disk_gb()
}

impl DiskType {
    pub fn label(&self) -> &'static str {
        match self {
            DiskType::Nvme => "NVMe SSD",
            DiskType::Ssd => "SATA SSD",
            DiskType::Hdd => "HDD",
            DiskType::Network => "Network Drive",
            DiskType::Unknown => "Unknown",
        }
    }

    pub fn sequential_read_mbps(&self) -> f64 {
        match self {
            DiskType::Nvme => 6000.0,
            DiskType::Ssd => 500.0,
            DiskType::Hdd => 150.0,
            DiskType::Network => 100.0,
            DiskType::Unknown => 500.0,
        }
    }
}

/// Benchmark disk write speed by writing a 256MB temp file.
pub async fn benchmark_disk_write() -> f64 {
    let path = std::env::temp_dir().join(format!("pdos_bench_{}", std::process::id()));
    let size: u64 = 256 * 1024 * 1024; // 256 MB
    let buf = vec![0u8; 4_194_304]; // 4MB chunks

    let start = std::time::Instant::now();
    match tokio::fs::File::create(&path).await {
        Ok(mut file) => {
            let mut written: u64 = 0;
            while written < size {
                let n = std::cmp::min(buf.len() as u64, size - written) as usize;
                if file.write_all(&buf[..n]).await.is_err() { break; }
                written += n as u64;
            }
            let _ = file.sync_all().await;
            let elapsed = start.elapsed().as_secs_f64();
            let _ = tokio::fs::remove_file(&path).await;
            if elapsed > 0.0 {
                return (size as f64 / elapsed) / 1_000_000.0; // MB/s
            }
        }
        Err(_) => {}
    }
    0.0
}

/// Benchmark disk read speed by reading the same 256MB temp file.
pub async fn benchmark_disk_read() -> f64 {
    let path = std::env::temp_dir().join(format!("pdos_bench_{}", std::process::id()));
    let size: u64 = 256 * 1024 * 1024;
    let buf = vec![0u8; 4_194_304];

    // Write first, then read back
    if let Ok(mut file) = tokio::fs::File::create(&path).await {
        let mut written: u64 = 0;
        while written < size {
            let n = std::cmp::min(buf.len() as u64, size - written) as usize;
            let _ = file.write_all(&buf[..n]).await;
            written += n as u64;
        }
        let _ = file.sync_all().await;
    }

    let start = std::time::Instant::now();
    match tokio::fs::File::open(&path).await {
        Ok(mut file) => {
            let mut read_buf = vec![0u8; 4_194_304];
            let mut total: u64 = 0;
            loop {
                let n = file.read(&mut read_buf).await.unwrap_or(0);
                if n == 0 { break; }
                total += n as u64;
            }
            let elapsed = start.elapsed().as_secs_f64();
            let _ = tokio::fs::remove_file(&path).await;
            if elapsed > 0.0 {
                return (total as f64 / elapsed) / 1_000_000.0;
            }
        }
        Err(_) => {
            let _ = std::fs::remove_file(&path);
        }
    }
    0.0
}
