pub mod network;
pub mod file_analyzer;
pub mod system;
pub mod profile;
pub mod strategy;
pub mod buffer;
pub mod stream;
pub mod scheduler;
pub mod monitor;
pub mod recovery;
pub mod learning;
pub mod capabilities;

use std::sync::Arc;
use tokio::sync::Mutex;
use crate::telemetry::new_transfer_session;

pub struct TransferContext {
    pub network: network::NetworkInfo,
    pub files: file_analyzer::FileAnalysis,
    pub system: system::SystemInfo,
    pub remote: network::RemoteCapabilities,
    pub profile: profile::TransferProfile,
    pub config: TransferConfig,
    pub state: Arc<Mutex<TransferState>>,
}

pub struct TransferConfig {
    pub dst_host: String,
    pub dst_port: u16,
    pub src_paths: Vec<String>,
    pub dst_base_path: String,
    pub tls_enabled: bool,
    pub max_memory_mb: u64,
    pub probe_duration_ms: u64,
    pub monitor_interval_ms: u64,
}

impl Default for TransferConfig {
    fn default() -> Self {
        TransferConfig {
            dst_host: String::new(),
            dst_port: 8080,
            src_paths: Vec::new(),
            dst_base_path: "~/Downloads/PDOS".into(),
            tls_enabled: false,
            max_memory_mb: 256,
            probe_duration_ms: 200,
            monitor_interval_ms: 1000,
        }
    }
}

pub struct TransferState {
    pub active_streams: u32,
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
    pub current_speed_mbps: f64,
    pub peak_speed_mbps: f64,
    pub packets_lost: u64,
    pub retransmits: u64,
    pub files_completed: u32,
    pub files_total: u32,
    pub is_running: bool,
}

impl Default for TransferState {
    fn default() -> Self {
        Self {
            active_streams: 0,
            total_bytes_sent: 0,
            total_bytes_received: 0,
            current_speed_mbps: 0.0,
            peak_speed_mbps: 0.0,
            packets_lost: 0,
            retransmits: 0,
            files_completed: 0,
            files_total: 0,
            is_running: false,
        }
    }
}

// ── TransferEngine: profile-driven execution ──────────────────────────────

pub struct TransferEngine {
    pub profile: profile::TransferProfile,
    pub monitor: monitor::TransferMonitor,
    pub learning_db: learning::LearningDb,
    pub config: TransferConfig,
}

impl TransferEngine {
    pub fn new(profile: profile::TransferProfile, config: TransferConfig) -> Self {
        Self {
            monitor: monitor::TransferMonitor::new(),
            learning_db: learning::LearningDb::new(1000),
            profile,
            config,
        }
    }

    pub async fn execute(&mut self) -> anyhow::Result<crate::telemetry::TransferSession> {
        let host = self.config.dst_host.clone();
        let port = self.config.dst_port;
        let base = self.config.dst_base_path.clone();
        let src = self.config.src_paths[0].clone();

        match self.profile.strategy {
            profile::TransferStrategy::SingleStream | profile::TransferStrategy::Pipelined => {
                self.execute_single_file(&host, port, &src, &base).await
            }
            profile::TransferStrategy::ParallelRanges => {
                self.execute_parallel(&host, port, &src, &base).await
            }
            profile::TransferStrategy::Batched => {
                self.execute_batch(&host, port, &base).await
            }
        }
    }

    async fn execute_single_file(
        &mut self, host: &str, port: u16, src: &str, _base: &str,
    ) -> anyhow::Result<crate::telemetry::TransferSession> {
        let path = std::path::Path::new(src);
        let chunk_size = self.profile.chunk_size;
        let total_bytes = self.profile.total_bytes;

        if path.is_dir() && self.profile.file_count > 1 {
            let files = self.analyze_dir_files(src).await;
            let cb = make_progress_fn(total_bytes);
            return strategy::StrategyExecutor::execute_streaming_directory(
                host, port, path,
                path.file_name().and_then(|n| n.to_str()).unwrap_or("dir"),
                &self.profile, &files, total_bytes,
                Some(cb),
            ).await;
        }

        let cb = make_progress_fn(total_bytes);
        let result = crate::http_transfer::http_upload_with_chunk_size(
            host, port, src, None, chunk_size, Some(cb),
        ).await?;

        self.record_learning(&result, true);
        Ok(result)
    }

    async fn execute_parallel(
        &mut self, host: &str, port: u16, src: &str, base: &str,
    ) -> anyhow::Result<crate::telemetry::TransferSession> {
        let fname = std::path::Path::new(src)
            .file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let out_path = format!("{}/{}", base.trim_end_matches('/'), fname);
        let cb = make_progress_fn(self.profile.total_bytes);
        let total = self.profile.total_bytes;
        let streams = self.profile.recommended_streams;

        strategy::StrategyExecutor::execute_parallel_ranges(
            host, port, src, &out_path,
            total, streams,
            &self.profile, Some(cb),
        ).await
    }

    async fn execute_batch(
        &mut self, _host: &str, _port: u16, _base: &str,
    ) -> anyhow::Result<crate::telemetry::TransferSession> {
        let total = self.profile.total_bytes;
        let mut session = new_transfer_session("batch", total);
        session.original_size = total;
        Ok(session)
    }

    async fn analyze_dir_files(&self, dir: &str) -> Vec<file_analyzer::FileEntry> {
        let paths = vec![dir.to_string()];
        let analysis = file_analyzer::analyze_files(&paths).await;
        analysis.entries
    }

    fn record_learning(&mut self, _session: &crate::telemetry::TransferSession, _success: bool) {
        let record = learning::TransferRecord {
            interface_label: self.profile.interface.label().to_string(),
            rtt_ms: self.profile.rtt_ms,
            file_size: self.profile.total_bytes,
            file_count: self.profile.file_count,
            strategy: format!("{:?}", self.profile.strategy),
            streams: self.profile.recommended_streams,
            buffer_kb: self.profile.recommended_buffer_kb,
            achieved_mbps: 0.0,
            duration_secs: 0.0,
            success: true,
        };
        self.learning_db.record(record);
    }
}

fn make_progress_fn(total: u64) -> Arc<dyn Fn(u64, u64) + Send + Sync> {
    Arc::new(move |sent, _total| {
        if total > 0 {
            let pct = sent as f64 / total as f64 * 100.0;
            let bar_len = 40;
            let filled = (pct / 100.0 * bar_len as f64) as usize;
            let bar: String = (0..bar_len).map(|i| if i < filled { '█' } else { '░' }).collect();
            print!("\r     {} {:5.1}%", bar, pct);
        }
    })
}

// ── Standalone run_transfer (legacy convenience) ──────────────────────────

pub async fn run_transfer(
    host: &str,
    port: u16,
    src_path: &str,
    dst_base: &str,
) -> anyhow::Result<()> {
    println!("\n  ┌──────────────────────────────────────────────┐");
    println!("  │ 🔄  Adaptive Transfer Engine                   │");
    println!("  └──────────────────────────────────────────────┘\n");

    // 1. Probe network
    print!("  ⏳ Probing network...");
    use std::time::Instant;
    let net_start = Instant::now();
    let net = network::probe_network(host, port).await;
    println!(" done ({:.0}ms)", net_start.elapsed().as_secs_f64() * 1000.0);

    println!("     ├─ Interface: {}", net.interface_type.label());
    println!("     ├─ RTT:       {:.1}ms", net.rtt_ms);
    println!("     ├─ Bandwidth: {:.0} Mbps", net.available_bandwidth_mbps);
    println!("     ├─ Jitter:    {:.1}ms", net.jitter_ms);
    println!("     ├─ Loss:      {:.2}%", net.packet_loss_pct);
    println!("     └─ Congested: {}", if net.is_congested { "yes" } else { "no" });

    // 2. Analyze files
    print!("  ⏳ Analyzing files...");
    let fstart = Instant::now();
    let paths = vec![src_path.to_string()];
    let files = file_analyzer::analyze_files(&paths).await;
    println!(" done ({:.0}ms)", fstart.elapsed().as_secs_f64() * 1000.0);

    let size_str = if files.total_size >= 1_000_000_000 {
        format!("{:.2} GB", files.total_size as f64 / 1e9)
    } else if files.total_size >= 1_000_000 {
        format!("{:.2} MB", files.total_size as f64 / 1e6)
    } else if files.total_size >= 1024 {
        format!("{:.2} KB", files.total_size as f64 / 1024.0)
    } else {
        format!("{} B", files.total_size)
    };

    println!("     ├─ Category: {}", files.category.label());
    println!("     ├─ Files:    {}", files.count);
    println!("     ├─ Total:    {}", size_str);
    println!("     └─ Ext:      {}", files.primary_extension);

    // 3. Probe system (with disk benchmark)
    print!("  ⏳ Probing system...");
    let sys = system::probe_system().await;
    let bench_disk = system::benchmark_disk_write().await;
    println!(" done");

    println!("     ├─ CPU:       {} cores", sys.cpu_cores);
    println!("     ├─ RAM:       {} MB available", sys.ram_available_mb);
    println!("     ├─ Disk:      {} ({:.0} GB free)", sys.disk_type.label(), sys.disk_free_gb);
    println!("     └─ Bench:     {:.0} MB/s write", bench_disk);

    // 4. Capability handshake with remote
    print!("  ⏳ Handshaking...");
    let our_caps = capabilities::CapabilityExchange::local().await;
    let remote_caps = capabilities::perform_handshake(host, port, &our_caps).await;
    match &remote_caps {
        Ok(remote) => {
            println!(" done");
            println!("     ├─ Peer:     {}", remote.node_id.chars().take(12).collect::<String>());
            println!("     ├─ Hardware: {} {} ({} cores, {} GB RAM)",
                remote.hardware.storage_type,
                remote.hardware.storage_write_mbps as u64,
                remote.hardware.cpu_cores,
                remote.hardware.ram_mb / 1024,
            );
            println!("     ├─ Network:  {} @ {:.0} Mbps", remote.network.interface_type, remote.network.link_speed_mbps);
            println!("     ├─ State:    battery {:.0}%, {}", remote.state.battery_pct, remote.state.thermal_state);
            println!("     └─ Features: zero-copy:{}, resume:{}, dir-stream:{}",
                if remote.features.zero_copy { "yes" } else { "no" },
                if remote.features.resume { "yes" } else { "no" },
                if remote.features.streaming_directory { "yes" } else { "no" },
            );

            // Store remote caps for engine
            if let Ok(mut rc) = crate::system_monitor::REMOTE_CAPABILITIES.lock() {
                *rc = Some(Box::new(remote.clone()));
            }
        }
        Err(e) => {
            println!(" fallback (handshake: {})", e);
        }
    }

    let remote = network::probe_remote_capabilities(host, port, "/").await;

    // 5. Build profile (now includes chunk_size from BDP)
    print!("  ⏳ Building transfer profile...");
    let profile = profile::build_profile(&net, &files, &sys, &remote);
    println!(" done");

    println!("     ├─ Strategy:  {:?}", profile.strategy);
    println!("     ├─ Streams:   {}", profile.recommended_streams);
    println!("     ├─ Chunk:     {} KB", profile.chunk_size / 1024);
    println!("     ├─ Buffer:    {} KB", profile.recommended_buffer_kb);
    println!("     ├─ Integrity: {:?}", profile.integrity);
    println!("     └─ Zero-copy: {}", if profile.use_zero_copy { "yes" } else { "no" });

    // 6. Create engine and execute
    let config = TransferConfig {
        dst_host: host.to_string(),
        dst_port: port,
        src_paths: vec![src_path.to_string()],
        dst_base_path: dst_base.to_string(),
        ..Default::default()
    };

    let mut engine = TransferEngine::new(profile, config);

    println!("\n  ──────────── Transfer ────────────\n");

    let start = Instant::now();
    let result = engine.execute().await;

    println!();
    let elapsed = start.elapsed().as_secs_f64();

    match result {
        Ok(session) => {
            let speed = if elapsed > 0.0 { (session.original_size as f64 * 8.0) / (elapsed * 1_000_000.0) } else { 0.0 };
            println!("\n  ✓ Transfer complete!");
            println!("     ├─ {} transferred ({:.2} Mbps avg)", session.filename, speed);
            println!("     ├─ Duration: {:.1}s", elapsed);
            if let Some(ref hash) = session.sha256 {
                println!("     └─ SHA-256: {}", hash);
            }
        }
        Err(e) => {
            eprintln!("\n  ✗ Transfer failed: {}", e);
            return Err(e);
        }
    }

    Ok(())
}
