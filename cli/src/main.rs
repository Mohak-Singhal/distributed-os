#![recursion_limit = "512"]
//! `dos` — CLI management tool.

mod clipboard;
mod dashboard;
mod pairing;
mod file;
mod net;
mod notify;
mod p2p;
mod pair;
mod ping;
mod search;
mod system_monitor;
mod telemetry;
mod terminal;
mod tls;
mod http_transfer;
mod transfer_engine;
pub mod observability;
pub mod syscall_profiler;
pub mod adaptive;
pub mod transfer_mode;
pub mod tunnel;
pub mod zero_copy;
pub mod mode_switcher;
pub mod transport;
pub mod udp_transport;
pub mod quic_transport;
pub mod transport_switcher;
mod transfer_engine_adapter;

use transfer_engine_adapter::run_cli_upload as new_upload;

use std::time::Duration;
use tracing_subscriber::{fmt, EnvFilter};

fn setup_adb_port_forwarding() {
    // 1. Run "adb devices" to get the list of active serials
    if let Ok(output) = std::process::Command::new("adb").arg("devices").output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 && parts[1] == "device" {
                let serial = parts[0];
                // Apply forward and reverse for this specific device
                let _ = std::process::Command::new("adb").args(&["-s", serial, "forward", "tcp:7894", "tcp:7894"]).output();
                let _ = std::process::Command::new("adb").args(&["-s", serial, "forward", "tcp:7891", "tcp:7891"]).output();
                let _ = std::process::Command::new("adb").args(&["-s", serial, "forward", "tcp:7892", "tcp:7892"]).output();
                let _ = std::process::Command::new("adb").args(&["-s", serial, "forward", "tcp:7893", "tcp:7893"]).output();
                let _ = std::process::Command::new("adb").args(&["-s", serial, "reverse", "tcp:7895", "tcp:7895"]).output();
                let _ = std::process::Command::new("adb").args(&["-s", serial, "reverse", "tcp:8080", "tcp:8080"]).output();
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    setup_adb_port_forwarding();
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_help();
        return Ok(());
    }

    let cmd = args[1].as_str();
    // Load transfer history from disk
    telemetry::load_history_from_disk();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            // Phase 4: Start System Monitor (runs in background)
            tokio::spawn(system_monitor::start_monitoring());
            match cmd {
                "search" => {
                    let query = if args.len() > 2 {
                        args[2].clone()
                    } else {
                        String::new()
                    };
                    if let Err(e) = search::run_search(query).await {
                        eprintln!("Search failed: {}", e);
                    }
                }
                "ping" => {
                    if args.len() < 3 {
                        eprintln!("Usage: dos ping <node_id>");
                    } else if let Err(e) = ping::run_ping(&args[2]).await {
                        eprintln!("Ping failed: {}", e);
                    }
                }
                "pair" => {
                    if args.len() < 3 {
                        eprintln!("Usage: dos pair <node_id>");
                    } else if let Err(e) = pair::run_pair(&args[2]).await {
                        eprintln!("Pair failed: {}", e);
                    }
                }
                "clipboard" => {
                    if args.len() < 4 {
                        eprintln!("Usage: dos clipboard <get|set> <node_id> [text]");
                    } else {
                        let action = args[2].as_str();
                        let node_id_str = &args[3];
                        match action {
                            "get" => {
                                if let Ok(id) = uuid::Uuid::parse_str(node_id_str) {
                                    if let Err(e) = clipboard::run_clipboard_get(id).await {
                                        eprintln!("Clipboard get failed: {}", e);
                                    }
                                } else {
                                    eprintln!("Invalid Node ID");
                                }
                            }
                            "set" => {
                                if args.len() < 5 {
                                    eprintln!("Usage: dos clipboard set <node_id> <text>");
                                } else {
                                    let text = &args[4];
                                    if let Ok(id) = uuid::Uuid::parse_str(node_id_str) {
                                        if let Err(e) = clipboard::run_clipboard_set(id, text).await
                                        {
                                            eprintln!("Clipboard set failed: {}", e);
                                        }
                                    } else {
                                        eprintln!("Invalid Node ID");
                                    }
                                }
                            }
                            _ => eprintln!("Invalid clipboard action: {}", action),
                        }
                    }
                }
                "notify" => {
                    if args.len() < 5 {
                        eprintln!("Usage: dos notify <node_id> <title> <body>");
                    } else {
                        let node_id_str = &args[2];
                        let title = &args[3];
                        let body = &args[4];
                        if let Ok(id) = uuid::Uuid::parse_str(node_id_str) {
                            if let Err(e) = notify::run_notify(id, title, body).await {
                                eprintln!("Notification failed: {}", e);
                            }
                        } else {
                            eprintln!("Invalid Node ID");
                        }
                    }
                }
                "exec" => {
                    if args.len() < 4 {
                        eprintln!("Usage: dos exec <node_id> <command> [args...]");
                    } else {
                        let node_id_str = &args[2];
                        let command = &args[3];
                        let cmd_args: Vec<String> =
                            args[4..].iter().map(|s| s.to_string()).collect();
                        if let Ok(id) = uuid::Uuid::parse_str(node_id_str) {
                            if let Err(e) = terminal::run_terminal(id, command, &cmd_args).await {
                                eprintln!("Execution failed: {}", e);
                            }
                        } else {
                            eprintln!("Invalid Node ID");
                        }
                    }
                }
                "list-files" => {
                    if args.len() >= 4 && args[2] == "--http" {
                        let target = &args[3];
                        let remote_path = args.get(4).map(|s| s.as_str()).unwrap_or("/");
                        let host_port: Vec<&str> = target.split(':').collect();
                        if host_port.len() == 2 {
                            let host = host_port[0];
                            let port: u16 = host_port[1].parse().unwrap_or(8080);
                            match http_transfer::http_list_files(host, port, remote_path).await {
                                Ok(entries) => {
                                    println!("{} entries:", entries.len());
                                    for e in entries {
                                        let kind = if e.is_dir { "DIR" } else { "   " };
                                        println!("  {} {:>12}  {}", kind, e.size, e.name);
                                    }
                                }
                                Err(e) => eprintln!("List failed: {}", e),
                            }
                        } else {
                            eprintln!("Invalid host:port format: {}", target);
                        }
                    } else {
                        eprintln!("Usage: dos list-files --http <host:port> [remote_path]");
                    }
                }
                "send-file" => {
                    // Auto-discovery mode: `dos send-file <local_path>` discovers Android nodes
                    if args.len() < 4 {
                        // Try auto-discover if user just provided a file path
                        if args.len() == 3 {
                            let local_path = &args[2];
                            println!("🔍 Discovering Android nodes...");
                            match p2p::discover_xync_nodes(5).await {
                                Ok(nodes) => {
                                    let android: Vec<_> = nodes.into_iter().filter(|n| n.platform == "android").collect();
                                    if android.is_empty() {
                                        eprintln!("No Android nodes found. Use --http <ip:7894> to specify manually.");
                                        return Ok::<(), anyhow::Error>(());
                                    }
                                    let node = &android[0];
                                    if android.len() > 1 {
                                        println!("Found {} Android nodes, using first: {} ({})", android.len(), node.node_name, node.ip);
                                    }
                                    send_file_auto(&node.ip, local_path, None).await;
                                }
                                Err(e) => eprintln!("Discovery failed: {}. Use --http <ip:7894> to specify manually.", e),
                            }
                            return Ok::<(), anyhow::Error>(());
                        }
                        eprintln!("Usage: dos send-file [--http <host:port>] <local_path> [remote_filename]");
                        return Ok::<(), anyhow::Error>(());
                    }
                    if args.len() >= 4 && args[2] == "--http" {
                        // Auto-discovery via `--http auto`
                        if args[3] == "auto" {
                            let local_path = &args[4];
                            println!("🔍 Discovering Android nodes...");
                            match p2p::discover_xync_nodes(5).await {
                                Ok(nodes) => {
                                    let android: Vec<_> = nodes.into_iter().filter(|n| n.platform == "android").collect();
                                    if android.is_empty() {
                                        eprintln!("No Android nodes discovered.");
                                        return Ok::<(), anyhow::Error>(());
                                    }
                                    let node = &android[0];
                                    if android.len() > 1 {
                                        println!("Found {} Android nodes, using first: {} ({})", android.len(), node.node_name, node.ip);
                                    }
                                    // Use 7894 (file server), not 7891 (P2P port from mDNS)
                                    // Parse remaining flags from args[5..]
                                    let mut remote_filename: Option<&str> = None;
                                    let mut i = 5;
                                    while i < args.len() {
                                        if remote_filename.is_none() {
                                            remote_filename = Some(args[i].as_str());
                                        }
                                        i += 1;
                                    }
                                    send_file_auto(&node.ip, local_path, remote_filename).await;
                                }
                                Err(e) => eprintln!("Discovery failed: {}", e),
                            }
                            return Ok::<(), anyhow::Error>(());
                        }
                        if args.len() < 5 {
                            eprintln!("Usage: dos send-file --http <host:port|auto> <local_path> [remote_filename] [--progress-callback <port>]");
                        } else {
                            let target = &args[3];
                            let local_path = &args[4];
                            let mut remote_filename: Option<&str> = None;
                            let mut callback_port: Option<u16> = None;
                            let mut parallel_streams: usize = 1;
                            
                            // Parse optional flags and filename
                            let mut i = 5;
                            while i < args.len() {
                                match args[i].as_str() {
                                    "--progress-callback" if i + 1 < args.len() => {
                                        callback_port = args[i + 1].parse().ok();
                                        i += 2;
                                    }
                                    "--mode" if i + 1 < args.len() => {
                                        let mode_str = args[i + 1].to_lowercase();
                                        let mode = match mode_str.as_str() {
                                            "tcp" | "tcpbuffered" => Some(crate::transport::TransportMode::TcpBuffered),
                                            "zerocopy" | "tcpzerocopy" => Some(crate::transport::TransportMode::TcpZeroCopy),
                                            "udp" | "udpcustom" => Some(crate::transport::TransportMode::UdpCustom),
                                            "quic" => Some(crate::transport::TransportMode::Quic),
                                            _ => None,
                                        };
                                        if let Some(m) = mode {
                                            if let Ok(mut lock) = crate::adaptive::OVERRIDE_TRANSPORT_MODE.lock() {
                                                *lock = Some(m);
                                            }
                                        }
                                        i += 2;
                                    }
                                    "--parallel" if i + 1 < args.len() => {
                                        parallel_streams = args[i + 1].parse().unwrap_or(1).max(1);
                                        i += 2;
                                    }
                                    _ => {
                                        if remote_filename.is_none() {
                                            remote_filename = Some(args[i].as_str());
                                        }
                                        i += 1;
                                    }
                                }
                            }

                            // Auto-detect Wi-Fi (non-loopback) and adapt settings.
                            // Multi-stream *hurts* on Wi-Fi (tested: 594→525→467 Mbps) because
                            // parallel TCP streams compete for shared wireless airtime.
                            // Single stream with dynamic throttle-back yields best sustained results.
                            let host_clean = target.split(':').next().unwrap_or("127.0.0.1");
                            if host_clean != "127.0.0.1" && host_clean != "localhost" {
                                if parallel_streams > 1 {
                                    eprintln!("\x1b[33m[warn]\x1b[0m --parallel {} on Wi-Fi degrades throughput. Forcing 1 stream.", parallel_streams);
                                    parallel_streams = 1;
                                }
                            }

                            // If progress callback is set, start listener on sender side
                            if let Some(cb_port) = callback_port {
                                let cb_host = get_sender_lan_ip().unwrap_or_else(|| "0.0.0.0".to_string());
                                let host_port: Vec<&str> = target.split(':').collect();
                                if host_port.len() == 2 {
                                    let host = host_port[0];
                                    let port: u16 = host_port[1].parse().unwrap_or(8080);
                                    // Spawn listener for progress callbacks
                                    let cb_host_clone = cb_host.clone();
                                    let cb_port_clone = cb_port.clone();
                                    tokio::spawn(async move {
                                        if let Ok(listener) = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", cb_port_clone)).await {
                                            loop {
                                                if let Ok((mut s, _)) = listener.accept().await {
                                                    let mut buf = vec![0u8; 4096];
                                                    let _ = tokio::io::AsyncReadExt::read(&mut s, &mut buf).await;
                                                    // Parse body from HTTP request
                                                    if let Some(body_start) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                                        let body = &buf[body_start + 4..];
                                                        if let Ok(prog) = serde_json::from_slice::<serde_json::Value>(body) {
                                                            if let Ok(mut global) = crate::system_monitor::REMOTE_PROGRESS.lock() {
                                                                *global = prog;
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    });
                                    let session = if parallel_streams > 1 {
                                        let p = std::path::Path::new(local_path);
                                        let name = remote_filename.unwrap_or_else(|| p.file_name().and_then(|n| n.to_str()).unwrap_or("file"));
                                        http_transfer::http_upload_parallel(host, port, p, name, parallel_streams, 1048576, None).await
                                    } else {
                                        http_transfer::http_upload(host, port, local_path, remote_filename, None).await
                                    };
                                    match session {
                                        Ok(s) => println!("Upload complete: {} ({} bytes, {:.2} Mbps avg)",
                                            s.filename, s.original_size, s.average_speed_mbps),
                                        Err(e) => eprintln!("Upload failed: {}", e),
                                    }
                                } else {
                                    eprintln!("Invalid host:port format: {}", target);
                                }
                            } else {
                                let host_port: Vec<&str> = target.split(':').collect();
                                if host_port.len() == 2 {
                                    let host = host_port[0];
                                    let port: u16 = host_port[1].parse().unwrap_or(8080);
                                    let session = if parallel_streams > 1 {
                                        let p = std::path::Path::new(local_path);
                                        let name = remote_filename.unwrap_or_else(|| p.file_name().and_then(|n| n.to_str()).unwrap_or("file"));
                                        http_transfer::http_upload_parallel(host, port, p, name, parallel_streams, 1048576, None).await
                                    } else {
                                        http_transfer::http_upload(host, port, local_path, remote_filename, None).await
                                    };
                                    match session {
                                        Ok(s) => println!("Upload complete: {} ({} bytes, {:.2} Mbps avg)",
                                            s.filename, s.original_size, s.average_speed_mbps),
                                        Err(e) => eprintln!("Upload failed: {}", e),
                                    }
                                } else {
                                    eprintln!("Invalid host:port format: {}", target);
                                }
                            }
                        }
                    } else if args.len() < 5 {
                        eprintln!("Usage: dos send-file [--http <host:port>] <node_id> <local_path> <remote_path>");
                    } else {
                        let node_id_str = &args[2];
                        let local_path = &args[3];
                        let remote_path = &args[4];
                        if let Ok(id) = uuid::Uuid::parse_str(node_id_str) {
                            if let Err(e) = file::run_file_write(id, local_path, remote_path).await
                            {
                                eprintln!("Send file failed: {}", e);
                            }
                        } else {
                            eprintln!("Invalid Node ID");
                        }
                    }
                }
                "get-file" => {
                    if args.len() >= 4 && args[2] == "--http" {
                        if args.len() < 5 {
                            eprintln!("Usage: dos get-file --http <host:port> <path> <output_path>");
                        } else {
                            let target = &args[3];
                            let remote_path = &args[4];
                            let local_path = &args[5];
                            let host_port: Vec<&str> = target.split(':').collect();
                            if host_port.len() == 2 {
                                let host = host_port[0];
                                let port: u16 = host_port[1].parse().unwrap_or(8080);
                                match http_transfer::http_download(host, port, remote_path, local_path, None).await {
                                    Ok(session) => println!("Download complete: {} ({} bytes, {:.2} Mbps avg)",
                                        session.filename, session.original_size, session.average_speed_mbps),
                                    Err(e) => eprintln!("Download failed: {}", e),
                                }
                            } else {
                                eprintln!("Invalid host:port format: {}", target);
                            }
                        }
                    } else if args.len() < 5 {
                        eprintln!("Usage: dos get-file [--http <host:port>] <node_id> <remote_path> <local_path>");
                    } else {
                        let node_id_str = &args[2];
                        let remote_path = &args[3];
                        let local_path = &args[4];
                        if let Ok(id) = uuid::Uuid::parse_str(node_id_str) {
                            if let Err(e) = file::run_file_read(id, remote_path, local_path).await {
                                eprintln!("Get file failed: {}", e);
                            }
                        } else {
                            eprintln!("Invalid Node ID");
                        }
                    }
                }
                "discover" => {
                    let timeout = if args.len() > 2 {
                        args[2].parse::<u64>().unwrap_or(5)
                    } else {
                        5
                    };
                    match p2p::discover_xync_nodes(timeout).await {
                        Ok(nodes) => {
                            if nodes.is_empty() {
                                println!("No Xync nodes discovered on the network.");
                            } else {
                                println!("Discovered {} Xync node(s):", nodes.len());
                                for (i, node) in nodes.iter().enumerate() {
                                    println!("  {}. {} ({}:{}) — {} ({})",
                                        i + 1, node.name, node.ip, node.port, node.node_name, node.platform);
                                }
                            }
                        }
                        Err(e) => eprintln!("Discovery failed: {}", e),
                    }
                }
                "mirror" | "screen" => {
                    if args.len() < 3 {
                        eprintln!("Usage: dos mirror <ip> [port]");
                    } else {
                        let ip = &args[2];
                        let port: u16 = args.get(3).and_then(|p| p.parse().ok()).unwrap_or(7892);
                        println!("Connecting to screen mirror at {ip}:{port}...");
                        match p2p::receive_video_stream(ip, port).await {
                            Ok(_) => println!("Mirror session ended"),
                            Err(e) => eprintln!("Mirror error: {e}"),
                        }
                    }
                }
                "camera" => {
                    if args.len() < 3 {
                        eprintln!("Usage: dos camera <ip> [port]");
                    } else {
                        let ip = &args[2];
                        let port: u16 = args.get(3).and_then(|p| p.parse().ok()).unwrap_or(7893);
                        println!("Connecting to camera stream at {ip}:{port}...");
                        match p2p::receive_video_stream(ip, port).await {
                            Ok(_) => println!("Camera session ended"),
                            Err(e) => eprintln!("Camera error: {e}"),
                        }
                    }
                }
                "connect" => {
                    if args.len() < 3 {
                        eprintln!("Usage: dos connect <ip> [port]");
                    } else {
                        let ip = &args[2];
                        let port: u16 = args.get(3).and_then(|p| p.parse().ok()).unwrap_or(7891);
                        let node = p2p::P2pNode {
                            name: format!("{}:{}", ip, port),
                            ip: ip.clone(),
                            port,
                            node_name: "Unknown".to_string(),
                            platform: "android".to_string(),
                        };
                        match p2p::connect_to_node(&node).await {
                            Ok(conn) => {
                                println!("Connected to {}:{} ✓", ip, port);
                                // Keep connection alive for now
                                let _ = conn;
                                loop {
                                    tokio::time::sleep(Duration::from_secs(3600)).await;
                                }
                            }
                            Err(e) => eprintln!("Connection failed: {}", e),
                        }
                    }
                }
                "transfer" => {
                    if args.len() < 4 {
                        eprintln!("Usage: dos transfer <host:port> <path> [dst_base]");
                    } else {
                        let target = &args[2];
                        let path = &args[3];
                        let dst_base = args.get(4).map(|s| s.as_str()).unwrap_or("~/Downloads/PDOS");
                        let host_port: Vec<&str> = target.split(':').collect();
                        if host_port.len() == 2 {
                            let host = host_port[0];
                            let port: u16 = host_port[1].parse().unwrap_or(8080);
                            if let Err(e) = transfer_engine::run_transfer(host, port, path, dst_base).await {
                                eprintln!("Transfer failed: {}", e);
                            }
                        } else {
                            eprintln!("Invalid host:port format: {}", target);
                        }
                    }
                }
                "serve" => {
                    let port = if args.len() > 2 {
                        args[2].parse::<u16>().unwrap_or(7894)
                    } else {
                        7894
                    };
                    if let Err(e) = run_serve(port).await {
                        eprintln!("Serve failed: {}", e);
                    }
                }
                "dashboard" => {
                    let port = if args.len() > 2 {
                        args[2].parse::<u16>().unwrap_or(8080)
                    } else {
                        8080
                    };
                    if let Err(e) = dashboard::run_dashboard(port).await {
                        eprintln!("Dashboard failed: {}", e);
                    }
                }
                _ => {
                    println!("Unknown command: {}", cmd);
                    print_help();
                }
            }
            Ok(())
        });

    Ok(())
}

/// Try direct HTTP upload first; if that fails (hotspot blocks inbound), fall back to reverse tunnel.
async fn send_file_auto(ip: &str, local_path: &str, remote_filename: Option<&str>) {
    let p = std::path::Path::new(local_path);
    let name = remote_filename.unwrap_or_else(|| p.file_name().and_then(|n| n.to_str()).unwrap_or("file"));

    // Try direct HTTP upload first (works on WiFi, USB tether)
    let file_size = std::fs::metadata(local_path).map(|m| m.len()).unwrap_or(0);
    let mut tunnel_needed = false;
    match http_transfer::http_upload(ip, 7894, local_path, remote_filename, None).await {
        Ok(s) => {
            if s.average_speed_mbps > 0.0 || file_size == 0 {
                println!("Upload complete: {} ({} bytes, {:.2} Mbps avg)",
                    s.filename, s.original_size, s.average_speed_mbps);
                return;
            }
            // 0 Mbps means Samsung firewall blocked data silently
            eprintln!("⚠ Direct upload returned 0 Mbps — likely firewall blocked data");
            tunnel_needed = true;
        }
        Err(e) => {
            eprintln!("Direct upload failed: {}", e);
            tunnel_needed = true;
        }
    }

    if tunnel_needed {
        println!("🔄 Trying reverse tunnel (port 7895)...");
    }

    // Try reverse tunnel (phone→Mac TCP, allowed over hotspot)
    match tunnel::send_through_tunnel(ip, local_path, name).await {
        Ok(()) => {
            println!("Upload complete via tunnel: {} ({} bytes)", name, file_size);
        }
        Err(e) => {
            eprintln!("Tunnel upload also failed: {}", e);
            eprintln!("💡 Tip: Ensure phone is on same network and 'dos serve' is running.");
        }
    }
}

/// Run the persistent serve mode: advertise Mac on `_xync._tcp`,
/// continuously discover Android nodes, and optionally receive files.
async fn run_serve(advertise_port: u16) -> anyhow::Result<()> {
    let hostname = get_sender_lan_ip()
        .map(|_| gethostname())
        .unwrap_or_else(|| "Mac".to_string());
    let node_name = format!("PDOS-Mac-{}", hostname.replace(' ', "-").chars().filter(|c| c.is_alphanumeric() || *c == '-').collect::<String>());

    // Start the reverse tunnel server for hotspot workaround
    let tunnel_port = advertise_port + 1; // 7895
    tokio::spawn(tunnel::start_tunnel_server(tunnel_port));
    // Start the tunnel receive server for Android → Mac file pushes (separate port to avoid concurrency issues)
    let recv_port = tunnel_port + 1; // 7896
    tokio::spawn(tunnel::start_tunnel_receive_server(recv_port));

    // Advertise this Mac on _xync._tcp
    let _mdns = p2p::advertise_xync(advertise_port, &node_name)?;
    println!("📡 Advertising as '{}' on _xync._tcp (port {})", node_name, advertise_port);
    println!("🔄 Reverse tunnel on port {} (phone connects here for hotspot)", tunnel_port);
    println!("⬆️  Receive tunnel on port {} (phone pushes files to Mac)", recv_port);
    println!("🔍 Discovering nodes... (Ctrl+C to stop)");

    // Continuously browse for _xync._tcp peers
    let mdns = mdns_sd::ServiceDaemon::new()?;
    let receiver = mdns.browse("_xync._tcp.local.")?;

    let mut known = std::collections::HashSet::new();
    loop {
        match receiver.recv_async().await {
            Ok(mdns_sd::ServiceEvent::ServiceResolved(svc)) => {
                let ip = svc
                    .get_addresses_v4()
                    .iter()
                    .next()
                    .map(|a| a.to_string())
                    .unwrap_or_default();
                let platform = svc
                    .get_property_val_str("platform")
                    .unwrap_or("unknown")
                    .to_string();
                let node = svc
                    .get_property_val_str("node_name")
                    .unwrap_or("unknown")
                    .to_string();
                let addr = format!("{}:{}", ip, svc.get_port());
                if known.insert(addr.clone()) {
                    println!("  📱 {} ({}) — {}", node, platform, addr);
                }
            }
            Ok(mdns_sd::ServiceEvent::ServiceRemoved(_, name)) => {
                println!("  ❌ Removed: {}", name);
            }
            Err(e) => {
                eprintln!("mDNS error: {}", e);
                break;
            }
            _ => {}
        }
    }

    Ok(())
}

fn gethostname() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("scutil")
            .arg("--get")
            .arg("ComputerName")
            .output()
        {
            if let Ok(name) = String::from_utf8(output.stdout) {
                return name.trim().to_string();
            }
        }
    }
    whoami::hostname().unwrap_or_else(|_| "Mac".to_string())
}

fn get_sender_lan_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

fn print_help() {
    println!(
        "dos — Personal Distributed OS v{}",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("USAGE:");
    println!("  dos <COMMAND>");
    println!();
    println!("COMMANDS:");
    println!("  serve            Advertise Mac + discover devices (persistent)");
    println!("  discover         Discover Xync P2P nodes via mDNS");
    println!("  send-file <path> Auto-discover Android node and send file");
    println!("  send-file --http <host:port> <local> [name]  Send via HTTP");
    println!("  send-file --http auto <local> [name]  Auto-discover & send");
    println!("  get-file --http <host:port> <path> <output>  Get via HTTP");
    println!("  list-files --http <host:port> [path]    Remote file browser");
    println!("  connect          Connect to a P2P node directly");
    println!("  search           Search for a device via relay");
    println!("  pair             Pair with a new device");
    println!("  ping             Ping a node");
    println!("  clipboard        Get or set clipboard contents");
    println!("  notify           Send a push notification");
    println!("  exec             Execute a terminal command");
    println!("  transfer         Smart transfer (auto-probe → profile → execute)");
    println!("  dashboard        Start the local control hub web UI");
    println!("  mirror           Receive screen mirror stream");
    println!("  camera           Receive camera stream");
}


