use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use serde::Deserialize;

#[derive(Deserialize)]
struct NotifyRequest {
    node_id: String,
    title: String,
    body: String,
}

#[derive(Deserialize)]
struct ClipboardSetRequest {
    node_id: String,
    content: String,
    use_local: Option<bool>,
}

#[derive(Deserialize)]
struct ClipboardGetRequest {
    node_id: String,
}

#[derive(Deserialize)]
struct ExecRequest {
    node_id: String,
    command: String,
}

#[derive(Deserialize)]
struct FileRequest {
    node_id: String,
    local_path: String,
    remote_path: String,
}

#[derive(Deserialize)]
struct UploadRequest {
    node_id: String,
    filename: String,
    content_base64: String,
    remote_path: String,
}

#[derive(Deserialize)]
struct ShareFileLanRequest {
    node_id: String,
    filename: String,
    content_base64: String,
}

fn read_local_clipboard() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("pbpaste").output() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    String::new()
}

fn write_local_clipboard(text: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::io::Write;
        if let Ok(mut child) = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return true;
        }
    }
    false
}

pub async fn run_dashboard(port: u16) -> anyhow::Result<()> {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    println!("------------------------------------------------------------");
    println!("🚀 PDOS Dashboard running on http://{}", addr);
    println!("👉 Open this link in your browser to test your devices!");
    println!("------------------------------------------------------------");

    // Phase 2: mDNS Zero-Config Discovery
    let mdns_port = port;
    tokio::spawn(async move {
        use mdns_sd::{ServiceDaemon, ServiceInfo};
        let mdns = ServiceDaemon::new().expect("Failed to create mDNS daemon");
        let hostname = format!("pdos-mac-{}", uuid::Uuid::new_v4().to_string().chars().take(4).collect::<String>());
        let service_type = "_pdos._tcp.local.";
        let instance_name = "PDOS Mac Node";
        let ip = "0.0.0.0"; 
        
        let mut properties = std::collections::HashMap::new();
        properties.insert("txtvers".to_string(), "1".to_string());

        let service_info = ServiceInfo::new(
            service_type,
            instance_name,
            &format!("{}.local.", hostname),
            ip,
            mdns_port,
            properties
        ).expect("valid service info");

        mdns.register(service_info).expect("Failed to register mDNS service");
        println!("📡 mDNS Discovery active on _pdos._tcp.local");
        // Keep alive
        loop { tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await; }
    });

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream).await {
                eprintln!("Error handling connection: {}", e);
            }
        });
    }
}

async fn handle_connection(mut stream: TcpStream) -> anyhow::Result<()> {
    use tokio::io::AsyncReadExt;

    // Read headers: keep reading until we find \r\n\r\n
    let mut header_buf: Vec<u8> = Vec::with_capacity(4096);
    let mut tmp = [0u8; 1];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 { return Ok(()); }
        header_buf.push(tmp[0]);
        if header_buf.ends_with(b"\r\n\r\n") { break; }
        if header_buf.len() > 32768 { return Ok(()); } // safety limit
    }

    let header_str = String::from_utf8_lossy(&header_buf);
    let mut lines = header_str.lines();
    let request_line = match lines.next() {
        Some(l) => l.trim_end_matches('\r'),
        None => return Ok(()),
    };

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let full_path = parts.next().unwrap_or("/");

    // Split path and query string
    let (path, query) = if let Some(idx) = full_path.find('?') {
        (&full_path[..idx], &full_path[idx+1..])
    } else {
        (full_path, "")
    };

    // Extract Content-Length header
    let content_length: usize = header_str
        .lines()
        .find(|l| l.to_lowercase().starts_with("content-length:"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);

    // Extract Content-Type header
    let content_type: String = header_str
        .lines()
        .find(|l| l.to_lowercase().starts_with("content-type:"))
        .and_then(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()))
        .unwrap_or_default();

    // --- Remote file browser: GET /api/list-files?path=<dir> ---
    if method == "GET" && path == "/api/list-files" {
        let dir_path = parse_query_param(query, "path").unwrap_or("/");
        let expanded = url_decode(dir_path);
        let expanded = if expanded.starts_with("~/") {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            expanded.replacen("~/", &format!("{}/", home), 1)
        } else {
            expanded
        };
        match list_directory_contents(&expanded) {
            Ok(entries) => {
                let json = serde_json::json!(entries).to_string();
                send_json_response(&mut stream, 200, &json).await?;
            }
            Err(e) => {
                let err_json = serde_json::json!({"error": e.to_string()}).to_string();
                send_json_response(&mut stream, 500, &err_json).await?;
            }
        }
        return Ok(());
    }

    // --- Binary streaming upload (no base64, no size limit) ---
    if method == "POST" && path == "/api/stream-upload" {
        return handle_stream_upload(&mut stream, query, content_length, &content_type).await;
    }

    // --- Stream Download from Device via Dashboard ---
    if method == "GET" && path == "/api/proxy-download" {
        let host = parse_query_param(query, "host").unwrap_or("");
        let port: u16 = parse_query_param(query, "port").unwrap_or("7894").parse().unwrap_or(7894);
        let remote_path = url_decode(parse_query_param(query, "path").unwrap_or(""));
        
        if host.is_empty() || remote_path.is_empty() {
            let err = serde_json::json!({"error": "Missing host or path"}).to_string();
            send_json_response(&mut stream, 400, &err).await?;
            return Ok(());
        }
        
        let request = format!(
            "GET {} HTTP/1.1\r\n\
             Host: {}:{}\r\n\
             Connection: close\r\n\
             \r\n",
            remote_path, host, port
        );
        match tokio::net::TcpStream::connect(format!("{}:{}", host, port)).await {
            Ok(mut remote_stream) => {
                remote_stream.write_all(request.as_bytes()).await?;
                // Stream directly back to the client
                let mut buf = vec![0u8; 65536];
                loop {
                    let n = remote_stream.read(&mut buf).await?;
                    if n == 0 { break; }
                    stream.write_all(&buf[..n]).await?;
                }
                return Ok(());
            }
            Err(e) => {
                let err = serde_json::json!({"error": format!("Failed to connect to device: {}", e)}).to_string();
                send_json_response(&mut stream, 500, &err).await?;
                return Ok(());
            }
        }
    }

    // --- File download: GET /api/files/<filename> ---
    if method == "GET" && path.starts_with("/api/files/") {
        let filename = &path["/api/files/".len()..];
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let downloads_dir = format!("{}/Downloads/PDOS", home);
        let file_path = format!("{}/{}", downloads_dir, filename);
        if let Ok(metadata) = tokio::fs::metadata(&file_path).await {
            if metadata.is_file() {
                let file_size = metadata.len();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"{}\"\r\nConnection: close\r\n\r\n",
                    file_size, filename
                );
                stream.write_all(response.as_bytes()).await?;
                let mut file = tokio::fs::File::open(&file_path).await?;
                let mut buf = [0u8; 65536];
                loop {
                    let n = file.read(&mut buf).await?;
                    if n == 0 { break; }
                    stream.write_all(&buf[..n]).await?;
                }
                return Ok(());
            }
        }
        send_json_response(&mut stream, 404, "{\"error\":\"File not found\"}").await?;
        return Ok(());
    }

    // --- Resource Monitor Metrics ---
    
    // Phase 16: Pending Transfers Long-Polling
    if method == "GET" && path == "/api/pending-transfers" {
        let json_resp = {
            let mut pending = PENDING_TRANSFERS.lock().unwrap();
            let resp = serde_json::json!(*pending).to_string();
            pending.clear();
            resp
        };
        send_json_response(&mut stream, 200, &json_resp).await?;
        return Ok(());
    }

if method == "GET" && path == "/api/system-metrics" {
        let json = {
            if let Ok(metrics) = crate::system_monitor::CURRENT_METRICS.lock() {
                serde_json::to_string(&*metrics).unwrap_or_else(|_| "{}".to_string())
            } else {
                "{}".to_string()
            }
        };
        send_json_response(&mut stream, 200, &json).await?;
        return Ok(());
    }

    // --- Phase 4A: Poll Sender-Side Progress ---
    if method == "GET" && path == "/api/transfer-status" {
        let json = {
            if let Ok(map) = get_transfer_progress().lock() {
                let values: Vec<&serde_json::Value> = map.values().collect();
                serde_json::to_string(&values).unwrap_or_else(|_| "[]".to_string())
            } else {
                "[]".to_string()
            }
        };
        send_json_response(&mut stream, 200, &json).await?;
        return Ok(());
    }

    // --- Live transfer progress (for progress bar) ---
    if method == "GET" && path == "/api/transfer-progress" {
        let json = {
            if let Ok(prog) = crate::system_monitor::TRANSFER_PROGRESS.lock() {
                serde_json::to_string(&*prog).unwrap_or_else(|_| "{}".to_string())
            } else {
                "{}".to_string()
            }
        };
        send_json_response(&mut stream, 200, &json).await?;
        return Ok(());
    }

    // --- Sender-side transfer progress (when dashboard is uploading) ---
    if method == "GET" && path == "/api/sender-transfer-progress" {
        let json = {
            if let Ok(prog) = crate::system_monitor::SENDER_TRANSFER_PROGRESS.lock() {
                serde_json::to_string(&*prog).unwrap_or_else(|_| "{}".to_string())
            } else {
                "{}".to_string()
            }
        };
        send_json_response(&mut stream, 200, &json).await?;
        return Ok(());
    }

    // --- Trigger HTTP file send from dashboard to remote device ---
    if method == "POST" && path == "/api/send-to-device" && content_length > 0 && content_length < 65536 {
        return handle_send_to_device(&mut stream, content_length).await;
    }

    // --- Trigger HTTP file download from remote device via dashboard ---
    if method == "POST" && path == "/api/download-from-device" && content_length > 0 && content_length < 65536 {
        return handle_download_from_device(&mut stream, content_length).await;
    }

    // --- Android → Mac receive file ---
    if method == "POST" && path == "/api/receive-file" {
        return handle_receive_file(&mut stream, content_length, &content_type, &header_str).await;
    }

    // For all other endpoints: read body as text (JSON, small payloads)
    let body_bytes = if method == "POST" && content_length > 0 {
        let to_read = content_length.min(10 * 1024 * 1024); // max 10MB for JSON endpoints
        let mut body = vec![0u8; to_read];
        let mut read = 0;
        while read < to_read {
            let n = stream.read(&mut body[read..]).await?;
            if n == 0 { break; }
            read += n;
        }
        body[..read].to_vec()
    } else {
        vec![]
    };
    let body = std::str::from_utf8(&body_bytes).unwrap_or("");

    match (method, path) {
        ("GET", "/") => {
            let html = get_dashboard_html();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                html.len(),
                html
            );
            stream.write_all(response.as_bytes()).await?;
        }
        ("GET", "/api/pending-transfers") => {
            let transfers = PENDING_TRANSFERS.lock()
                .map(|t| t.clone())
                .unwrap_or_default();
            let json = serde_json::to_string(&transfers).unwrap_or_else(|_| "[]".to_string());
            send_json_response(&mut stream, 200, &json).await?;
        }
        ("GET", "/api/benchmark-metrics") => {
            let resp = serde_json::json!({
                "success": true,
                "samples": [],
                "linux_metrics_before": null,
                "linux_metrics_after": null
            }).to_string();
            send_json_response(&mut stream, 200, &resp).await?;
        }
        ("GET", "/api/devices") => {
            match crate::search::run_search_raw(String::new()).await {
                Ok(mut results) => {
                    // Filter out transient CLI connections (they have no capabilities)
                    results.retain(|r| !r.capabilities.is_empty());
                    let json = serde_json::to_string(&results)?;
                    send_json_response(&mut stream, 200, &json).await?;
                }
                Err(e) => {
                    let err_json = serde_json::json!({ "error": e.to_string() }).to_string();
                    send_json_response(&mut stream, 500, &err_json).await?;
                }
            }
        }
        ("POST", "/api/initiate-udp") => {
            if let Ok(req) = serde_json::from_str::<serde_json::Value>(body) {
                let filename = req.get("filename").and_then(|v| v.as_str()).unwrap_or("udp_file.bin").to_string();
                let size = req.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                let mode = req.get("mode").and_then(|v| v.as_str()).unwrap_or("UdpCustom").to_string();

                match tokio::net::UdpSocket::bind("0.0.0.0:0").await {
                    Ok(udp_socket) => {
                        let udp_port = udp_socket.local_addr().map(|a| a.port()).unwrap_or(0);
                        if udp_port == 0 {
                            send_json_response(&mut stream, 500, "{\"error\":\"Failed to get bound UDP port\"}").await?;
                        } else {
                            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                            let downloads_dir = format!("{}/Downloads/PDOS", home);
                            let _ = std::fs::create_dir_all(&downloads_dir);
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0);
                            let final_filename = if filename.is_empty() { format!("received_{}", timestamp) } else { filename };
                            let final_path = format!("{}/{}", downloads_dir, final_filename);

                            tokio::spawn(async move {
                                let start = std::time::Instant::now();
                                if let Ok(mut prog) = crate::system_monitor::TRANSFER_PROGRESS.lock() {
                                    *prog = serde_json::json!({
                                        "active": true,
                                        "filename": final_filename.clone(),
                                        "bytes_sent": 0,
                                        "total_bytes": size,
                                        "progress_pct": 0.0,
                                        "speed_mbps": 0.0,
                                        "status": "receiving"
                                    });
                                }

                                let prog_cb = {
                                    let final_filename_clone = final_filename.clone();
                                    move |received: u64, total: u64| {
                                        let elapsed = start.elapsed().as_secs_f64();
                                        let speed = if elapsed > 0.0 { (received as f64 * 8.0) / (elapsed * 1_000_000.0) } else { 0.0 };
                                        if let Ok(mut prog) = crate::system_monitor::TRANSFER_PROGRESS.lock() {
                                            *prog = serde_json::json!({
                                                "active": true,
                                                "filename": final_filename_clone.clone(),
                                                "bytes_sent": received,
                                                "total_bytes": total,
                                                "progress_pct": (received as f64 / total.max(1) as f64) * 100.0,
                                                "speed_mbps": speed,
                                                "status": "receiving"
                                            });
                                        }
                                    }
                                };

                                let recv_res = if mode == "Quic" {
                                    crate::quic_transport::receive_file_quic_with_socket(udp_socket, std::path::Path::new(&final_path), size, Some(Box::new(prog_cb))).await
                                } else {
                                    crate::udp_transport::receive_file_udp_with_socket(udp_socket, std::path::Path::new(&final_path), size, Some(Box::new(prog_cb))).await
                                };

                                if let Ok(mut prog) = crate::system_monitor::TRANSFER_PROGRESS.lock() {
                                    *prog = serde_json::json!({
                                        "active": false,
                                        "filename": final_filename.clone(),
                                        "bytes_sent": size,
                                        "total_bytes": size,
                                        "progress_pct": 100.0,
                                        "speed_mbps": 0.0,
                                        "status": if recv_res.is_ok() { "completed" } else { "failed" }
                                    });
                                }
                            });

                            let resp = serde_json::json!({
                                "success": true,
                                "udp_port": udp_port
                            }).to_string();
                            send_json_response(&mut stream, 200, &resp).await?;
                        }
                    }
                    Err(e) => {
                        let err_json = serde_json::json!({ "error": format!("Failed to bind UDP socket: {}", e) }).to_string();
                        send_json_response(&mut stream, 500, &err_json).await?;
                    }
                }
            } else {
                send_json_response(&mut stream, 400, "{\"error\":\"Invalid JSON body\"}").await?;
            }
        }
        ("POST", "/api/notify") => {
            if let Ok(req) = serde_json::from_str::<NotifyRequest>(body) {
                if let Ok(uuid) = uuid::Uuid::parse_str(&req.node_id) {
                    match crate::notify::run_notify_raw(uuid, &req.title, &req.body).await {
                        Ok(_) => {
                            send_json_response(&mut stream, 200, "{\"success\":true}").await?;
                        }
                        Err(e) => {
                            let err_json = serde_json::json!({ "error": e.to_string() }).to_string();
                            send_json_response(&mut stream, 500, &err_json).await?;
                        }
                    }
                } else {
                    send_json_response(&mut stream, 400, "{\"error\":\"Invalid UUID\"}").await?;
                }
            } else {
                send_json_response(&mut stream, 400, "{\"error\":\"Invalid request JSON\"}").await?;
            }
        }
        ("POST", "/api/clipboard/get") => {
            if let Ok(req) = serde_json::from_str::<ClipboardGetRequest>(body) {
                if let Ok(uuid) = uuid::Uuid::parse_str(&req.node_id) {
                    match crate::clipboard::run_clipboard_get_raw(uuid).await {
                        Ok(content) => {
                            write_local_clipboard(&content);
                            let resp = serde_json::json!({ "success": true, "content": content }).to_string();
                            send_json_response(&mut stream, 200, &resp).await?;
                        }
                        Err(e) => {
                            let err_json = serde_json::json!({ "error": e.to_string() }).to_string();
                            send_json_response(&mut stream, 500, &err_json).await?;
                        }
                    }
                } else {
                    send_json_response(&mut stream, 400, "{\"error\":\"Invalid UUID\"}").await?;
                }
            } else {
                send_json_response(&mut stream, 400, "{\"error\":\"Invalid request JSON\"}").await?;
            }
        }
        ("POST", "/api/clipboard/set") => {
            if let Ok(req) = serde_json::from_str::<ClipboardSetRequest>(body) {
                if let Ok(uuid) = uuid::Uuid::parse_str(&req.node_id) {
                    let content = if req.use_local.unwrap_or(false) {
                        read_local_clipboard()
                    } else {
                        req.content.clone()
                    };
                    match crate::clipboard::run_clipboard_set_raw(uuid, &content).await {
                        Ok(_) => {
                            let resp = serde_json::json!({ "success": true, "content": content }).to_string();
                            send_json_response(&mut stream, 200, &resp).await?;
                        }
                        Err(e) => {
                            let err_json = serde_json::json!({ "error": e.to_string() }).to_string();
                            send_json_response(&mut stream, 500, &err_json).await?;
                        }
                    }
                } else {
                    send_json_response(&mut stream, 400, "{\"error\":\"Invalid UUID\"}").await?;
                }
            } else {
                send_json_response(&mut stream, 400, "{\"error\":\"Invalid request JSON\"}").await?;
            }
        }
        ("POST", "/api/exec") => {
            if let Ok(req) = serde_json::from_str::<ExecRequest>(body) {
                if let Ok(uuid) = uuid::Uuid::parse_str(&req.node_id) {
                    // Split command & args
                    let mut parts = req.command.split_whitespace();
                    if let Some(cmd) = parts.next() {
                        let args: Vec<String> = parts.map(|s| s.to_string()).collect();
                        match crate::terminal::run_terminal_raw(uuid, cmd, &args).await {
                            Ok(output) => {
                                let resp = serde_json::json!({ "success": true, "output": output }).to_string();
                                send_json_response(&mut stream, 200, &resp).await?;
                            }
                            Err(e) => {
                                let err_json = serde_json::json!({ "error": e.to_string() }).to_string();
                                send_json_response(&mut stream, 500, &err_json).await?;
                            }
                        }
                    } else {
                        send_json_response(&mut stream, 400, "{\"error\":\"Empty command\"}").await?;
                    }
                } else {
                    send_json_response(&mut stream, 400, "{\"error\":\"Invalid UUID\"}").await?;
                }
            } else {
                send_json_response(&mut stream, 400, "{\"error\":\"Invalid request JSON\"}").await?;
            }
        }
        // Consumer-friendly: browser uploads file bytes, we write to /tmp and relay it
        ("POST", "/api/upload-and-send") => {
            if let Ok(req) = serde_json::from_str::<UploadRequest>(body) {
                if let Ok(uuid) = uuid::Uuid::parse_str(&req.node_id) {
                    use base64::Engine;
                    match base64::engine::general_purpose::STANDARD.decode(&req.content_base64) {
                        Ok(bytes) => {
                            let tmp_path = format!("/tmp/pdos_upload_{}", req.filename);
                            match tokio::fs::write(&tmp_path, &bytes).await {
                                Ok(_) => {
                                    let remote = if req.remote_path.is_empty() {
                                        format!("/sdcard/Download/{}", req.filename)
                                    } else {
                                        req.remote_path.clone()
                                    };
                                    match crate::file::run_file_write_raw(uuid, &tmp_path, &remote).await {
                                        Ok(_) => {
                                            let _ = tokio::fs::remove_file(&tmp_path).await;
                                            let resp = serde_json::json!({ "success": true, "remote_path": remote }).to_string();
                                            send_json_response(&mut stream, 200, &resp).await?;
                                        }
                                        Err(e) => {
                                            let _ = tokio::fs::remove_file(&tmp_path).await;
                                            let err = serde_json::json!({ "error": e.to_string() }).to_string();
                                            send_json_response(&mut stream, 500, &err).await?;
                                        }
                                    }
                                }
                                Err(e) => {
                                    let err = serde_json::json!({ "error": format!("Failed to write temp file: {}", e) }).to_string();
                                    send_json_response(&mut stream, 500, &err).await?;
                                }
                            }
                        }
                        Err(_) => {
                            send_json_response(&mut stream, 400, "{\"error\":\"Invalid base64 content\"}").await?;
                        }
                    }
                } else {
                    send_json_response(&mut stream, 400, "{\"error\":\"Invalid UUID\"}").await?;
                }
            } else {
                send_json_response(&mut stream, 400, "{\"error\":\"Invalid request JSON\"}").await?;
            }
        }
        ("POST", "/api/send-file") => {
            if let Ok(req) = serde_json::from_str::<FileRequest>(body) {
                if let Ok(uuid) = uuid::Uuid::parse_str(&req.node_id) {
                    match crate::file::run_file_write_raw(uuid, &req.local_path, &req.remote_path).await {
                        Ok(_) => {
                            send_json_response(&mut stream, 200, "{\"success\":true}").await?;
                        }
                        Err(e) => {
                            let err_json = serde_json::json!({ "error": e.to_string() }).to_string();
                            send_json_response(&mut stream, 500, &err_json).await?;
                        }
                    }
                } else {
                    send_json_response(&mut stream, 400, "{\"error\":\"Invalid UUID\"}").await?;
                }
            } else {
                send_json_response(&mut stream, 400, "{\"error\":\"Invalid request JSON\"}").await?;
            }
        }
        ("POST", "/api/get-file") => {
            if let Ok(req) = serde_json::from_str::<FileRequest>(body) {
                if let Ok(uuid) = uuid::Uuid::parse_str(&req.node_id) {
                    match crate::file::run_file_read_raw(uuid, &req.remote_path, &req.local_path).await {
                        Ok(_) => {
                            send_json_response(&mut stream, 200, "{\"success\":true}").await?;
                        }
                        Err(e) => {
                            let err_json = serde_json::json!({ "error": e.to_string() }).to_string();
                            send_json_response(&mut stream, 500, &err_json).await?;
                        }
                    }
                } else {
                    send_json_response(&mut stream, 400, "{\"error\":\"Invalid UUID\"}").await?;
                }
            } else {
                send_json_response(&mut stream, 400, "{\"error\":\"Invalid request JSON\"}").await?;
            }
        }
        // LocalSend-style: relay signals only, file bytes flow directly peer-to-peer over LAN
        ("POST", "/api/share-file-lan") => {
            if let Ok(req) = serde_json::from_str::<ShareFileLanRequest>(body) {
                if let Ok(target_uuid) = uuid::Uuid::parse_str(&req.node_id) {
                    use base64::Engine;
                    match base64::engine::general_purpose::STANDARD.decode(&req.content_base64) {
                        Ok(bytes) => {
                            let tmp_path = format!("/tmp/pdos_lan_{}", &req.filename);
                            match tokio::fs::write(&tmp_path, &bytes).await {
                                Ok(_) => {
                                    let file_size = bytes.len();
                                    match get_lan_ip() {
                                        Some(lan_ip) => {
                                            match tokio::net::TcpListener::bind("0.0.0.0:0").await {
                                                Ok(file_listener) => {
                                                    let port = file_listener.local_addr()
                                                        .map(|a| a.port()).unwrap_or(0);
                                                    let download_url = format!(
                                                        "http://{}:{}/{}", lan_ip, port, req.filename
                                                    );
                                                    let path_clone = tmp_path.clone();
                                                    let fname_clone = req.filename.clone();
                                                    // One-shot background HTTP file server
                                                    tokio::spawn(async move {
                                                        let accept = tokio::time::timeout(
                                                            tokio::time::Duration::from_secs(600),
                                                            file_listener.accept()
                                                        ).await;
                                                        if let Ok(Ok((mut client, _))) = accept {
use tokio::io::{AsyncReadExt, AsyncWriteExt};
                                                            let mut req_buf = vec![0u8; 4096];
                                                            let _ = client.read(&mut req_buf).await;
                                                            if let Ok(data) = tokio::fs::read(&path_clone).await {
                                                                let header = format!(
                                                                    "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"{}\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                                                    fname_clone, data.len()
                                                                );
                                                                let _ = client.write_all(header.as_bytes()).await;
                                                                let _ = client.write_all(&data).await;
                                                            }
                                                        }
                                                        let _ = tokio::fs::remove_file(&path_clone).await;
                                                    });
                                                    // Tiny relay signal — only the URL, not file bytes
                                                    let signal = format!(
                                                        "PDOS_DOWNLOAD::{{\"url\":\"{}\",\"filename\":\"{}\",\"size\":{}}}",
                                                        download_url, req.filename, file_size
                                                    );
                                                    match crate::notify::run_notify_raw(
                                                        target_uuid, "File Ready", &signal
                                                    ).await {
                                                        Ok(_) => {
                                                            let resp = serde_json::json!({
                                                                "success": true,
                                                                "url": download_url,
                                                                "message": "Device is downloading directly over LAN"
                                                            }).to_string();
                                                            send_json_response(&mut stream, 200, &resp).await?;
                                                        }
                                                        Err(e) => {
                                                            let _ = tokio::fs::remove_file(&tmp_path).await;
                                                            let err = serde_json::json!({"error": format!("Relay signal failed: {}", e)}).to_string();
                                                            send_json_response(&mut stream, 500, &err).await?;
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    let _ = tokio::fs::remove_file(&tmp_path).await;
                                                    let err = serde_json::json!({"error": format!("Port bind failed: {}", e)}).to_string();
                                                    send_json_response(&mut stream, 500, &err).await?;
                                                }
                                            }
                                        }
                                        None => {
                                            let _ = tokio::fs::remove_file(&tmp_path).await;
                                            send_json_response(&mut stream, 500, "{\"error\":\"Cannot detect LAN IP — check Wi-Fi\"}").await?;
                                        }
                                    }
                                }
                                Err(e) => {
                                    let err = serde_json::json!({"error": format!("Temp write failed: {}", e)}).to_string();
                                    send_json_response(&mut stream, 500, &err).await?;
                                }
                            }
                        }
                        Err(_) => {
                            send_json_response(&mut stream, 400, "{\"error\":\"Invalid base64\"}").await?;
                        }
                    }
                } else {
                    send_json_response(&mut stream, 400, "{\"error\":\"Invalid UUID\"}").await?;
                }
            } else {
                send_json_response(&mut stream, 400, "{\"error\":\"Invalid request JSON\"}").await?;
            }
        }
        // ── Telemetry & Observability API ──────────────────────────────────
        ("GET", "/api/transfer-history") => {
            let json = {
                if let Ok(sessions) = crate::telemetry::TRANSFER_SESSIONS.lock() {
                    let list: Vec<serde_json::Value> = sessions.iter().map(|s| serde_json::json!({
                        "id": s.id,
                        "filename": s.filename,
                        "size": s.original_size,
                        "duration_secs": s.duration_secs,
                        "average_speed_mbps": s.average_speed_mbps,
                        "peak_speed_mbps": s.peak_speed_mbps,
                        "health_score": s.health_score,
                        "completed": s.completed,
                        "verified": s.verified,
                        "start_time": s.start_time,
                        "end_time": s.end_time,
                        "bottleneck": s.bottleneck,
                        "compression_ratio": s.compression_ratio,
                        "compressed_size": s.compressed_size,
                        "bandwidth_saved": s.bandwidth_saved,
                        "reconnects": s.reconnects,
                    })).collect();
                    serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string())
                } else { "[]".to_string() }
            };
            send_json_response(&mut stream, 200, &json).await?;
        }
        ("GET", "/api/transfer-report") => {
            let id = parse_query_param(query, "id").unwrap_or("");
            let json = {
                if let Ok(sessions) = crate::telemetry::TRANSFER_SESSIONS.lock() {
                    if let Some(session) = sessions.iter().find(|s| s.id == id) {
                        serde_json::to_string(&crate::telemetry::export_session_json(session))
                            .unwrap_or_else(|_| "{}".to_string())
                    } else {
                        "{}".to_string()
                    }
                } else { "{}".to_string() }
            };
            send_json_response(&mut stream, 200, &json).await?;
        }
        ("GET", "/api/transfer-waterfall") => {
            let id = parse_query_param(query, "id").unwrap_or("");
            let json = {
                if let Ok(sessions) = crate::telemetry::TRANSFER_SESSIONS.lock() {
                    if let Some(session) = sessions.iter().find(|s| s.id == id) {
                        let waterfall: Vec<serde_json::Value> = session.phases.iter().map(|p| serde_json::json!({
                            "name": p.name,
                            "start_ms": p.start,
                            "end_ms": p.end,
                            "duration_ms": p.duration_ms,
                        })).collect();
                        serde_json::to_string(&waterfall).unwrap_or_else(|_| "[]".to_string())
                    } else { "[]".to_string() }
                } else { "[]".to_string() }
            };
            send_json_response(&mut stream, 200, &json).await?;
        }
        // ── Capability handshake ────────────────────────────────────────────
        ("POST", "/api/handshake") => {
            if !body.is_empty() {
                if let Ok(remote_caps) = serde_json::from_str::<crate::transfer_engine::capabilities::CapabilityExchange>(body) {
                    let our_caps = crate::transfer_engine::capabilities::CapabilityExchange::local().await;
                    // Store remote capabilities for ongoing transfer
                    if let Ok(mut rem) = crate::system_monitor::REMOTE_CAPABILITIES.lock() {
                        *rem = Some(Box::new(remote_caps));
                    }
                    let json = serde_json::to_string(&our_caps)?;
                    send_json_response(&mut stream, 200, &json).await?;
                } else {
                    send_json_response(&mut stream, 400, "{\"error\":\"Invalid capability JSON\"}").await?;
                }
            } else {
                send_json_response(&mut stream, 400, "{\"error\":\"Empty body\"}").await?;
            }
        }

        // ── Live telemetry ───────────────────────────────────────────────────
        ("GET", "/api/telemetry") => {
            let telemetry = crate::transfer_engine::capabilities::collect_telemetry().await;
            let json = serde_json::to_string(&telemetry)?;
            send_json_response(&mut stream, 200, &json).await?;
        }

        ("GET", "/api/protocol-stats") => {
            let json = {
                if let Ok(counters) = crate::telemetry::PROTOCOL_COUNTERS.lock() {
                    serde_json::json!({
                        "discovery_packets": counters.discovery_packets,
                        "auth_requests": counters.auth_requests,
                        "transfer_requests": counters.transfer_requests,
                        "resume_requests": counters.resume_requests,
                        "cancelled_transfers": counters.cancelled_transfers,
                        "completed_transfers": counters.completed_transfers,
                        "failed_transfers": counters.failed_transfers,
                        "tls_handshakes": counters.tls_handshakes,
                        "range_requests": counters.range_requests,
                    }).to_string()
                } else { "{}".to_string() }
            };
            send_json_response(&mut stream, 200, &json).await?;
        }
        ("GET", "/api/health-score") => {
            let json = {
                let sessions = crate::telemetry::TRANSFER_SESSIONS.lock().ok();
                let cpu = crate::system_monitor::CURRENT_METRICS.lock().ok();
                let thermal = crate::telemetry::THERMAL_STATE.lock().ok();

                let overall = sessions.as_ref()
                    .and_then(|s| s.last())
                    .and_then(|s| s.health_score)
                    .unwrap_or(100.0);
                let network_score = if let Some(m) = cpu.as_ref() {
                    let tx = m.get("network_tx_mbps").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                    if tx > 50.0 { 100.0 } else if tx > 10.0 { 85.0 } else { 70.0 }
                } else { 100.0 };
                let cpu_score = if let Some(m) = cpu.as_ref() {
                    let cpu = m.get("cpu_usage").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                    if cpu < 30.0 { 100.0 } else if cpu < 60.0 { 85.0 } else { 60.0 }
                } else { 100.0 };
                let _thermal_score = if let Some(t) = thermal.as_ref() {
                    match t.thermal_state.as_str() {
                        "Nominal" => 100.0,
                        "Fair" => 70.0,
                        "Critical" => 40.0,
                        _ => 100.0,
                    }
                } else { 100.0 };
                let disk_score = if let Some(m) = cpu.as_ref() {
                    if let Some(disks) = m.get("disks").and_then(|d| d.as_array()) {
                        if let Some(disk) = disks.first() {
                            let free = disk.get("available_gb").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let total = disk.get("total_gb").and_then(|v| v.as_f64()).unwrap_or(1.0);
                            let pct = free / total;
                            if pct > 0.2 { 100.0 } else if pct > 0.1 { 70.0 } else { 40.0 }
                        } else { 100.0 }
                    } else { 100.0 }
                } else { 100.0 };
                let recovery_score = if let Some(sessions) = sessions.as_ref() {
                    let completed = sessions.iter().filter(|s| s.completed).count();
                    let total = sessions.len().max(1);
                    (completed as f64 / total as f64) * 100.0
                } else { 100.0 };

                serde_json::json!({
                    "overall": overall,
                    "cpu": cpu_score,
                    "network": network_score,
                    "disk": disk_score,
                    "integrity": 100.0,
                    "recovery": recovery_score,
                }).to_string()
            };
            send_json_response(&mut stream, 200, &json).await?;
        }
        ("GET", "/api/thermal") => {
            let json = {
                if let Ok(thermal) = crate::telemetry::THERMAL_STATE.lock() {
                    serde_json::json!({
                        "cpu_temp_c": thermal.cpu_temp_c,
                        "thermal_state": thermal.thermal_state,
                        "fan_rpm": thermal.fan_rpm,
                        "battery_pct": thermal.battery_pct,
                        "battery_temp_c": thermal.battery_temp_c,
                        "thermal_status": thermal.thermal_status,
                    }).to_string()
                } else { "{}".to_string() }
            };
            send_json_response(&mut stream, 200, &json).await?;
        }
        ("GET", "/api/network-path") => {
            let json = {
                if let Ok(history) = crate::telemetry::NETWORK_HISTORY.lock() {
                    let list: Vec<serde_json::Value> = history.iter().map(|n| serde_json::json!({
                        "time": n.time,
                        "interface": n.interface,
                        "ip": n.ip,
                        "rssi": n.rssi,
                        "link_speed": n.link_speed,
                        "event": n.signal_event,
                        "tx_bytes": n.tx_bytes,
                        "rx_bytes": n.rx_bytes,
                    })).collect();
                    serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string())
                } else { "[]".to_string() }
            };
            send_json_response(&mut stream, 200, &json).await?;
        }
        ("GET", "/api/storage-forecast") => {
            let file_path = parse_query_param(query, "path").unwrap_or("/tmp");
            let file_size_str = parse_query_param(query, "size").unwrap_or("0");
            let file_size: u64 = file_size_str.parse().unwrap_or(0);
            let info = crate::telemetry::get_storage_info(file_path);
            let json = serde_json::json!({
                "total_gb": info.total_gb,
                "free_gb": info.free_gb,
                "file_size_gb": info.file_size_gb.max(file_size as f64 / 1e9),
                "remaining_gb": info.remaining_gb.max(info.free_gb - file_size as f64 / 1e9),
                "enough_space": info.enough_space || info.free_gb > file_size as f64 / 1e9,
            }).to_string();
            send_json_response(&mut stream, 200, &json).await?;
        }
        ("GET", "/api/compression-analytics") => {
            let json = {
                if let Ok(sessions) = crate::telemetry::TRANSFER_SESSIONS.lock() {
                    let compressed: Vec<&crate::telemetry::TransferSession> = sessions.iter()
                        .filter(|s| s.compressed_size.is_some()).collect();
                    let list: Vec<serde_json::Value> = compressed.iter().map(|s| serde_json::json!({
                        "filename": s.filename,
                        "original_size": s.original_size,
                        "compressed_size": s.compressed_size,
                        "compression_ratio": s.compression_ratio,
                        "compression_time_ms": s.compression_time_ms,
                        "bandwidth_saved": s.bandwidth_saved,
                        "time_saved_sec": s.time_saved_sec,
                    })).collect();
                    serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string())
                } else { "[]".to_string() }
            };
            send_json_response(&mut stream, 200, &json).await?;
        }
        ("GET", "/api/export-session") => {
            let id = parse_query_param(query, "id").unwrap_or("");
            let json = {
                if let Ok(sessions) = crate::telemetry::TRANSFER_SESSIONS.lock() {
                    if let Some(session) = sessions.iter().find(|s| s.id == id) {
                        serde_json::to_string_pretty(&crate::telemetry::export_session_json(session))
                            .unwrap_or_else(|_| "{}".to_string())
                    } else {
                        "{}".to_string()
                    }
                } else { "{}".to_string() }
            };
            send_json_response(&mut stream, 200, &json).await?;
        }
        ("GET", "/api/bottleneck") => {
            let json = {
                if let Ok(sessions) = crate::telemetry::TRANSFER_SESSIONS.lock() {
                    if let Some(session) = sessions.last() {
                        serde_json::json!({
                            "bottleneck": session.bottleneck,
                            "recommendation": session.recommendation,
                            "health_score": session.health_score,
                        }).to_string()
                    } else {
                        serde_json::json!({
                            "bottleneck": "Idle",
                            "recommendation": "No transfers yet.",
                        }).to_string()
                    }
                } else {
                    serde_json::json!({"bottleneck": "Unknown"}).to_string()
                }
            };
            send_json_response(&mut stream, 200, &json).await?;
        }
        ("GET", "/api/process-metrics") => {
            let json = {
                let metrics = crate::system_monitor::CURRENT_METRICS.lock().ok();
                serde_json::json!({
                    "processes": metrics.as_ref().and_then(|m| m.get("processes").cloned()).unwrap_or(serde_json::json!({})),
                    "global_cpu": metrics.as_ref().and_then(|m| m.get("global_cpu").cloned()).unwrap_or(serde_json::json!("0.0")),
                    "total_memory_mb": metrics.as_ref().and_then(|m| m.get("total_memory_mb").cloned()).unwrap_or(serde_json::json!("0.0")),
                    "used_memory_mb": metrics.as_ref().and_then(|m| m.get("used_memory_mb").cloned()).unwrap_or(serde_json::json!("0.0")),
                }).to_string()
            };
            send_json_response(&mut stream, 200, &json).await?;
        }
        ("GET", "/api/node-identity") => {
            match crate::tls::get_node_identity() {
                Ok(id) => {
                    let json = serde_json::json!({
                        "node_id": id.node_id.to_string(),
                        "public_key_hex": id.public_key_hex(),
                    }).to_string();
                    send_json_response(&mut stream, 200, &json).await?;
                }
                Err(e) => {
                    let err = serde_json::json!({"error": e.to_string()}).to_string();
                    send_json_response(&mut stream, 500, &err).await?;
                }
            }
        }
        ("POST", "/api/report-progress") => {
            if let Ok(prog) = serde_json::from_str::<serde_json::Value>(body) {
                if let Ok(mut global) = crate::system_monitor::REMOTE_PROGRESS.lock() {
                    *global = prog.clone();
                }
                send_json_response(&mut stream, 200, "{\"ok\":true}").await?;
            } else {
                send_json_response(&mut stream, 400, "{\"error\":\"invalid json\"}").await?;
            }
        }
        ("GET", "/api/sender-progress") => {
            let json = {
                if let Ok(prog) = crate::system_monitor::REMOTE_PROGRESS.lock() {
                    serde_json::to_string(&*prog).unwrap_or_else(|_| "{}".to_string())
                } else {
                    "{}".to_string()
                }
            };
            send_json_response(&mut stream, 200, &json).await?;
        }
        ("GET", "/api/capabilities") => {
            let sys_info = crate::transfer_engine::system::probe_system().await;
            let json = serde_json::json!({
                "cpu_cores": sys_info.cpu_cores,
                "ram_available_mb": sys_info.ram_available_mb,
                "disk_type": sys_info.disk_type.label(),
                "disk_free_gb": sys_info.disk_free_gb,
                "supports_range": true,
                "supports_http2": false,
                "max_parallel_connections": 8,
                "protocol_version": "1.0",
            }).to_string();
            send_json_response(&mut stream, 200, &json).await?;
        }

        // ── SSE control stream: persistent progress channel ────────────────
        ("GET", "/api/control-stream") => {
            let transfer_id = parse_query_param(query, "id").unwrap_or("default");
            let response = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nAccess-Control-Allow-Origin: *\r\n\r\n";
            stream.write_all(response.as_bytes()).await?;
            stream.flush().await?;

            let mut last_speed = 0.0f64;
            let start = std::time::Instant::now();
            loop {
                let progress = crate::system_monitor::TRANSFER_PROGRESS.lock()
                    .map(|p| p.clone())
                    .unwrap_or(serde_json::json!({}));
                let telemetry = crate::transfer_engine::capabilities::collect_telemetry().await;
                let speed = progress.get("speed_mbps").and_then(|v| v.as_f64()).unwrap_or(0.0);

                if (speed - last_speed).abs() > 0.1 || last_speed == 0.0 {
                    let enriched = serde_json::json!({
                        "progress": progress,
                        "telemetry": {
                            "cpu_load_pct": telemetry.cpu_load_pct,
                            "thermal_state": telemetry.thermal_state,
                            "battery_pct": telemetry.battery_pct,
                            "memory_pressure": telemetry.memory_pressure,
                        }
                    });
                    let event = format!("data: {}\n\n", serde_json::to_string(&enriched).unwrap_or_default());
                    if stream.write_all(event.as_bytes()).await.is_err() { break; }
                    if stream.flush().await.is_err() { break; }
                    last_speed = speed;
                }

                let is_active = progress.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
                if !is_active && start.elapsed().as_secs_f64() > 2.0 { break; }

                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            return Ok(());
        }

        // ── Transfer status for resume support ────────────────────────────
        ("GET", "/api/transfer-status") => {
            let id = parse_query_param(query, "id").unwrap_or("");
            let offset_param = parse_query_param(query, "offset").unwrap_or("0");
            let client_offset: u64 = offset_param.parse().unwrap_or(0);

            let store = crate::transfer_engine::recovery::RecoveryStore::load();
            let checkpoint = store.checkpoints.get(id);
            let server_offset = checkpoint.map(|c| c.completed_chunks.len() as u64 * c.chunk_size).unwrap_or(0);

            let json = serde_json::json!({
                "transfer_id": id,
                "client_offset": client_offset,
                "server_offset": server_offset,
                "can_resume": server_offset > 0 && client_offset <= server_offset,
                "completed_chunks": checkpoint.map(|c| c.completed_chunks.len()).unwrap_or(0),
                "total_size": checkpoint.map(|c| c.file_size).unwrap_or(0),
            }).to_string();
            send_json_response(&mut stream, 200, &json).await?;
        }

        ("GET", "/api/buffer-analysis") => {
            let json = {
                if let Ok(buf) = crate::telemetry::BUFFER_STATE.lock() {
                    serde_json::json!({
                        "read_buffer_kb": buf.read_buffer_kb,
                        "write_buffer_kb": buf.write_buffer_kb,
                        "average_queue_depth": buf.average_queue_depth,
                        "max_queue_depth": buf.max_queue_depth,
                        "backpressure_events": buf.backpressure_events,
                    }).to_string()
                } else { "{}".to_string() }
            };
            send_json_response(&mut stream, 200, &json).await?;
        }
        _ => {

            let not_found = "404 Not Found";
            let response = format!(
                "HTTP/1.1 404 NOT FOUND\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                not_found.len(),
                not_found
            );
            stream.write_all(response.as_bytes()).await?;
        }
    }

    Ok(())
}

async fn send_json_response(stream: &mut TcpStream, status: u16, json: &str) -> anyhow::Result<()> {
    let status_text = match status {
        200 => "200 OK",
        400 => "400 BAD REQUEST",
        500 => "500 INTERNAL SERVER ERROR",
        _ => "200 OK",
    };
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
        status_text,
        json.len(),
        json
    );
    stream.write_all(response.as_bytes()).await?;
    Ok(())
}

// --- Shared state for incoming transfers (Android → Mac) ---
use std::sync::Mutex;
static PENDING_TRANSFERS: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

// --- Phase 4A: Sender Side Progress (Mac → Android) ---
fn get_transfer_progress() -> &'static Mutex<std::collections::HashMap<String, serde_json::Value>> {
    static TRANSFER_PROGRESS: std::sync::OnceLock<Mutex<std::collections::HashMap<String, serde_json::Value>>> = std::sync::OnceLock::new();
    TRANSFER_PROGRESS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn parse_query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let mut kv = pair.splitn(2, '=');
        let k = kv.next()?;
        let v = kv.next()?;
        if k == key { Some(v) } else { None }
    })
}

fn url_decode(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next().unwrap_or('0');
            let h2 = chars.next().unwrap_or('0');
            if let Ok(byte) = u8::from_str_radix(&format!("{}{}", h1, h2), 16) {
                result.push(byte as char);
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

/// Phase 1A: Stream binary upload from browser → no base64, no RAM limit
async fn handle_stream_upload(
    stream: &mut TcpStream,
    query: &str,
    content_length: usize,
    _content_type: &str,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let node_id = parse_query_param(query, "node_id").unwrap_or("").to_string();
    let filename = url_decode(parse_query_param(query, "filename").unwrap_or("upload.bin"));

    if node_id.is_empty() || content_length == 0 {
        send_json_response(stream, 400, "{\"error\":\"Missing node_id or empty file\"}").await?;
        return Ok(());
    }

    let safe_tmp_name = filename.replace("/", "_");
    let tmp_path = format!("/tmp/pdos_stream_{}", safe_tmp_name);
    let mut file = tokio::fs::File::create(&tmp_path).await?;
    let mut remaining = content_length;
    let mut buf = vec![0u8; 65536]; // 64KB chunks

    while remaining > 0 {
        let to_read = buf.len().min(remaining);
        let n = stream.read(&mut buf[..to_read]).await?;
        if n == 0 { break; }
        file.write_all(&buf[..n]).await?;
        remaining -= n;
    }
    drop(file);
    let file_size = content_length - remaining;

    let target_uuid = match uuid::Uuid::parse_str(&node_id) {
        Ok(u) => u,
        Err(_) => {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            send_json_response(stream, 400, "{\"error\":\"Invalid UUID\"}").await?;
            return Ok(());
        }
    };

    match get_lan_ip() {
        Some(lan_ip) => {
            match tokio::net::TcpListener::bind("0.0.0.0:0").await {
                Ok(file_listener) => {
                    let port = file_listener.local_addr().map(|a| a.port()).unwrap_or(0);
                    let download_url = format!("https://{}:{}/{}", lan_ip, port, filename);
                    let path_clone = tmp_path.clone();
                    let fname_clone = filename.clone();
                    // One-shot LAN file server — Phase 2B: Secure TLS stream
                    tokio::spawn(async move {
                        let accept = tokio::time::timeout(
                            tokio::time::Duration::from_secs(600),
                            file_listener.accept()
                        ).await;
                        if let Ok(Ok((client, _))) = accept {
                            // Phase 3: TCP Buffer Optimization
                            let _ = client.set_nodelay(true);

                            use tokio::io::{AsyncReadExt, AsyncWriteExt};
                            
                            // Load TLS config
                            if let Ok((tls_config, _)) = crate::tls::get_or_create_tls_config() {
                                let acceptor = tokio_rustls::TlsAcceptor::from(tls_config);
                                if let Ok(mut tls_stream) = acceptor.accept(client).await {
                                    crate::system_monitor::log_op("info", &format!("TLS handshake successful. Starting chunked stream of {}", fname_clone));
                                    let mut req_buf = vec![0u8; 4096];
                                    let _ = tls_stream.read(&mut req_buf).await;
                                    let req_str = String::from_utf8_lossy(&req_buf);
                                    
                                    // Phase 4C: Resume Support - Parse Range header
                                    let mut start_offset = 0u64;
                                    if let Some(range_line) = req_str.lines().find(|l| l.to_lowercase().starts_with("range:")) {
                                        if let Some(bytes_range) = range_line.split('=').nth(1) {
                                            if let Some(start_str) = bytes_range.split('-').next() {
                                                if let Ok(start) = start_str.trim().parse::<u64>() {
                                                    start_offset = start;
                                                }
                                            }
                                        }
                                    }
                                    
                                    // Phase 4A + Fix Memory Crash: Stream file in chunks, calculate SHA-256 on the fly, track progress
                                    if let Ok(mut file) = tokio::fs::File::open(&path_clone).await {
                                        let metadata = file.metadata().await.unwrap();
                                        let file_size = metadata.len();
                                        
                                        if start_offset > 0 {
                                            use std::io::SeekFrom;
                                            use tokio::io::AsyncSeekExt;
                                            let _ = file.seek(SeekFrom::Start(start_offset)).await;
                                        }
                                        
                                        // Phase 2: Removed full-file pre-hash calculation to eliminate handshake latency.
                                        // TLS and TCP already guarantee transport integrity. 
                                        let sha256_hex = String::new();
                                        
                                        let header = if start_offset > 0 {
                                            let remaining = file_size - start_offset;
                                            format!(
                                                "HTTP/1.1 206 Partial Content\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"{}\"\r\nContent-Range: bytes {}-{}/{}\r\nContent-Length: {}\r\nX-File-Checksum: {}\r\nConnection: close\r\n\r\n",
                                                fname_clone, start_offset, file_size - 1, file_size, remaining, sha256_hex
                                            )
                                        } else {
                                            format!(
                                                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"{}\"\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\nX-File-Checksum: {}\r\nConnection: close\r\n\r\n",
                                                fname_clone, file_size, sha256_hex
                                            )
                                        };
                                        let _ = tls_stream.write_all(header.as_bytes()).await;
                                        
                                        let mut buf = vec![0u8; 1024 * 1024]; // 1MB chunks
                                        let mut total_sent = start_offset;
                                        while let Ok(n) = file.read(&mut buf).await {
                                            if n == 0 { break; }
                                            if tls_stream.write_all(&buf[..n]).await.is_err() { break; }
                                            total_sent += n as u64;
                                            
                                            // Update progress
                                            if let Ok(mut map) = get_transfer_progress().lock() {
                                                map.insert(fname_clone.clone(), serde_json::json!({
                                                    "filename": fname_clone.clone(),
                                                    "total": file_size,
                                                    "sent": total_sent,
                                                    "status": if total_sent == file_size { "completed" } else { "in_progress" }
                                                }));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        let _ = tokio::fs::remove_file(&path_clone).await;
                    });
                    // Signal device via relay — pass TLS fingerprint for TrustManager
                    let mut fingerprint = String::new();
                    if let Ok((_, fp)) = crate::tls::get_or_create_tls_config() {
                        fingerprint = fp;
                    }
                    let signal = format!(
                        "PDOS_DOWNLOAD::{{\"url\":\"{}\",\"filename\":\"{}\",\"size\":{},\"fingerprint\":\"{}\"}}",
                        download_url, filename, file_size, fingerprint
                    );
                    match crate::notify::run_notify_raw(target_uuid, "File Ready", &signal).await {
                        Ok(_) => {
                            let resp = serde_json::json!({
                                "success": true,
                                "url": download_url,
                                "message": "Device is downloading at full LAN speed"
                            }).to_string();
                            send_json_response(stream, 200, &resp).await?;
                        }
                        Err(e) => {
                            let _ = tokio::fs::remove_file(&tmp_path).await;
                            let err = serde_json::json!({"error": format!("Signal failed: {}", e)}).to_string();
                            send_json_response(stream, 500, &err).await?;
                        }
                    }
                }
                Err(e) => {
                    let _ = tokio::fs::remove_file(&tmp_path).await;
                    let err = serde_json::json!({"error": format!("Port bind failed: {}", e)}).to_string();
                    send_json_response(stream, 500, &err).await?;
                }
            }
        }
        None => {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            send_json_response(stream, 500, "{\"error\":\"Cannot detect LAN IP\"}").await?;
        }
    }
    Ok(())
}

/// Phase 1B: Receive files from Android → Mac (saves to ~/Downloads/PDOS/)
async fn handle_receive_file(
    stream: &mut TcpStream,
    content_length: usize,
    _content_type: &str,
    all_headers: &str,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::io::AsyncSeekExt;

    // Extract progress callback URL from headers if provided
    let progress_callback_url: Option<String> = all_headers
        .lines()
        .find(|l| l.to_lowercase().starts_with("x-report-progress-to:"))
        .and_then(|l| {
            let idx = l.find(':')?;
            let val = l[idx + 1..].trim();
            if val.is_empty() { None } else { Some(val.to_string()) }
        });

    // For now read up to 2GB
    if content_length == 0 {
        send_json_response(stream, 400, "{\"error\":\"Empty file\"}").await?;
        return Ok(());
    }

    // Parse multi-stream headers
    let offset: u64 = all_headers.lines()
        .find(|l| l.to_lowercase().starts_with("x-offset:"))
        .and_then(|l| l.split(':').nth(1).and_then(|v| v.trim().parse::<u64>().ok()))
        .unwrap_or(0);

    let total_size: Option<u64> = all_headers.lines()
        .find(|l| l.to_lowercase().starts_with("x-total-size:"))
        .and_then(|l| l.split(':').nth(1).and_then(|v| v.trim().parse::<u64>().ok()));

    // Create PDOS downloads dir
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let downloads_dir = format!("{}/Downloads/PDOS", home);
    tokio::fs::create_dir_all(&downloads_dir).await?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Use X-Filename header for naming incoming file
    let tmp_filename = all_headers.lines()
        .find(|l| l.to_lowercase().starts_with("x-filename:"))
        .and_then(|l| l.split(':').nth(1).map(|v| v.trim().to_string()))
        .unwrap_or_else(|| format!("received_{}", timestamp));
    let tmp_path = format!("{}/{}", downloads_dir, tmp_filename);

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&tmp_path)
        .await?;

    if offset == 0 {
        if let Some(ts) = total_size {
            let _ = file.set_len(ts).await;
        }
    }
    if offset > 0 {
        file.seek(std::io::SeekFrom::Start(offset)).await?;
    }

    let mut remaining = content_length;
    let mut buf = vec![0u8; 65536];
    let total = content_length as u64;
    let start = std::time::Instant::now();

    while remaining > 0 {
        let to_read = buf.len().min(remaining);
        let n = stream.read(&mut buf[..to_read]).await?;
        if n == 0 { break; }
        file.write_all(&buf[..n]).await?;
        remaining -= n;
        let sent = (total - remaining as u64) as u64;
        let elapsed = start.elapsed().as_secs_f64();
        let speed = if elapsed > 0.0 { (sent as f64 * 8.0) / (elapsed * 1_000_000.0) } else { 0.0 };
        if let Ok(mut prog) = crate::system_monitor::TRANSFER_PROGRESS.lock() {
            *prog = serde_json::json!({
                "active": true,
                "filename": tmp_filename,
                "bytes_sent": sent,
                "total_bytes": total,
                "progress_pct": (sent as f64 / total.max(1) as f64) * 100.0,
                "speed_mbps": speed,
                "status": "receiving"
            });
        }
        // Report progress back to sender if callback URL provided
        if let Some(cb_url) = &progress_callback_url {
            let pct = (sent as f64 / total.max(1) as f64) * 100.0;
            let body = serde_json::json!({
                "active": true,
                "filename": tmp_filename,
                "bytes_sent": sent,
                "total_bytes": total,
                "progress_pct": pct,
                "speed_mbps": speed,
                "status": "receiving"
            }).to_string();
            let url = cb_url.clone();
            tokio::spawn(async move {
                let _ = send_http_post(&url, &body).await;
            });
        }
    }
    drop(file);
    if let Ok(mut prog) = crate::system_monitor::TRANSFER_PROGRESS.lock() {
        *prog = serde_json::json!({
            "active": false,
            "filename": tmp_filename,
            "bytes_sent": total,
            "total_bytes": total,
            "progress_pct": 100.0,
            "speed_mbps": 0.0,
            "status": "completed"
        });
    }

    // Auto-extract zip files alongside the received zip
    let extracted_dir = if tmp_filename.ends_with(".zip") || tmp_path.ends_with(".zip") {
        let dir_name = tmp_path.trim_end_matches(".zip");
        std::fs::create_dir_all(dir_name).ok();
        match crate::http_transfer::unzip_to(&tmp_path, dir_name) {
            Ok(_) => {
                crate::system_monitor::log_op("info", &format!("Extracted zip to {}", dir_name));
                Some(dir_name.to_string())
            }
            Err(e) => {
                crate::system_monitor::log_op("error", &format!("Zip extraction failed: {}", e));
                None
            }
        }
    } else {
        None
    };

    // Signal completion to sender if callback URL provided
    if let Some(cb_url) = &progress_callback_url {
        let done_body = serde_json::json!({
            "active": false,
            "filename": tmp_filename,
            "bytes_sent": total,
            "total_bytes": total,
            "progress_pct": 100.0,
            "speed_mbps": 0.0,
            "status": "completed"
        }).to_string();
        let url = cb_url.clone();
        tokio::spawn(async move {
            let _ = send_http_post(&url, &done_body).await;
        });
    }

    // Add to pending list for UI polling
    let entry = serde_json::json!({
        "id": timestamp,
        "filename": tmp_filename,
        "path": tmp_path,
        "size": content_length,
        "received_at": timestamp,
        "status": "received",
        "extracted_to": extracted_dir
    });
    if let Ok(mut transfers) = PENDING_TRANSFERS.lock() {
        transfers.push(entry);
        if transfers.len() > 50 { transfers.remove(0); }
    }

    // Show macOS notification
    let script = format!("display notification \"File received from Android\" with title \"PDOS: File Received\" sound name \"default\"");
    let _ = std::process::Command::new("osascript").arg("-e").arg(&script).output();

    let resp = serde_json::json!({
        "success": true,
        "saved_to": tmp_path,
        "size": content_length
    }).to_string();
    send_json_response(stream, 200, &resp).await?;
    Ok(())
}

/// POST /api/send-to-device — trigger HTTP file upload from dashboard to a remote device.
/// Body: {"host":"192.168.1.x","port":8080,"local_path":"/path/to/file","filename":"optional_name"}
async fn handle_send_to_device(stream: &mut TcpStream, content_length: usize) -> anyhow::Result<()> {
    use tokio::io::AsyncReadExt;
    let mut body = vec![0u8; content_length];
    let mut read = 0;
    while read < content_length {
        let n = stream.read(&mut body[read..]).await?;
        if n == 0 { break; }
        read += n;
    }
    let req: serde_json::Value = match serde_json::from_slice(&body[..read]) {
        Ok(v) => v,
        Err(_) => {
            send_json_response(stream, 400, "{\"error\":\"invalid json\"}").await?;
            return Ok(());
        }
    };
    let host = req.get("host").and_then(|v| v.as_str()).unwrap_or("");
    let port = req.get("port").and_then(|v| v.as_u64()).unwrap_or(8080) as u16;
    let local_path = req.get("local_path").and_then(|v| v.as_str()).unwrap_or("");
    let filename = req.get("filename").and_then(|v| v.as_str());

    if host.is_empty() || local_path.is_empty() {
        send_json_response(stream, 400, "{\"error\":\"missing host or local_path\"}").await?;
        return Ok(());
    }

    // Reset sender progress
    if let Ok(mut prog) = crate::system_monitor::SENDER_TRANSFER_PROGRESS.lock() {
        *prog = serde_json::json!({
            "active": true,
            "filename": filename.unwrap_or("unknown"),
            "bytes_sent": 0,
            "total_bytes": 0,
            "progress_pct": 0.0,
            "speed_mbps": 0.0,
            "status": "starting"
        });
    }

    let host = host.to_string();
    let local_path = local_path.to_string();
    let filename = filename.map(|s| s.to_string());

    // Spawn the upload in background
    tokio::spawn(async move {
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let cb = {
            let host_c = host.clone();
            let fname = filename.clone().unwrap_or_else(|| "file".to_string());
            move |sent: u64, total: u64| {
                if let Ok(mut prog) = crate::system_monitor::SENDER_TRANSFER_PROGRESS.lock() {
                    let pct = if total > 0 { (sent as f64 / total as f64) * 100.0 } else { 0.0 };
                    *prog = serde_json::json!({
                        "active": sent < total,
                        "filename": fname,
                        "bytes_sent": sent,
                        "total_bytes": total,
                        "progress_pct": pct,
                        "speed_mbps": 0.0,
                        "status": if sent >= total { "completed" } else { "sending" }
                    });
                }
            }
        };
        match crate::http_transfer::http_upload(&host, port, &local_path, filename.as_deref(), Some(std::sync::Arc::new(cb))).await {
            Ok(session) => {
                crate::system_monitor::log_op("info", &format!("HTTP send complete: {} ({})", session.filename, session.original_size));
                if let Ok(mut prog) = crate::system_monitor::SENDER_TRANSFER_PROGRESS.lock() {
                    *prog = serde_json::json!({
                        "active": false,
                        "filename": session.filename,
                        "bytes_sent": session.original_size,
                        "total_bytes": session.original_size,
                        "progress_pct": 100.0,
                        "speed_mbps": session.average_speed_mbps,
                        "status": "completed"
                    });
                }
            }
            Err(e) => {
                crate::system_monitor::log_op("error", &format!("HTTP send failed: {}", e));
                if let Ok(mut prog) = crate::system_monitor::SENDER_TRANSFER_PROGRESS.lock() {
                    *prog = serde_json::json!({
                        "active": false,
                        "filename": filename.unwrap_or_else(|| "unknown".to_string()),
                        "bytes_sent": 0,
                        "total_bytes": 0,
                        "progress_pct": 0.0,
                        "speed_mbps": 0.0,
                        "status": "failed"
                    });
                }
            }
        }
    });

    send_json_response(stream, 200, "{\"success\":true,\"message\":\"Transfer started\"}").await?;
    Ok(())
}

/// POST /api/download-from-device — trigger HTTP download from a remote device via the dashboard.
/// Body: {"host":"192.168.1.x","port":8080,"remote_path":"/api/files/filename","local_dir":"~/Downloads/PDOS"}
/// Returns 200 immediately; progress tracked via /api/transfer-progress.
async fn handle_download_from_device(stream: &mut TcpStream, content_length: usize) -> anyhow::Result<()> {
    use tokio::io::AsyncReadExt;
    let mut body = vec![0u8; content_length];
    let mut read = 0;
    while read < content_length {
        let n = stream.read(&mut body[read..]).await?;
        if n == 0 { break; }
        read += n;
    }
    let req: serde_json::Value = match serde_json::from_slice(&body[..read]) {
        Ok(v) => v,
        Err(_) => {
            send_json_response(stream, 400, "{\"error\":\"invalid json\"}").await?;
            return Ok(());
        }
    };
    let host = req.get("host").and_then(|v| v.as_str()).unwrap_or("");
    let port = req.get("port").and_then(|v| v.as_u64()).unwrap_or(8080) as u16;
    let remote_path = req.get("remote_path").and_then(|v| v.as_str()).unwrap_or("");
    let local_dir = req.get("local_dir").and_then(|v| v.as_str()).unwrap_or("~/Downloads/PDOS");

    if host.is_empty() || remote_path.is_empty() {
        send_json_response(stream, 400, "{\"error\":\"missing host or remote_path\"}").await?;
        return Ok(());
    }

    let expanded_dir = if local_dir.starts_with("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        local_dir.replacen("~/", &format!("{}/", home), 1)
    } else {
        local_dir.to_string()
    };
    std::fs::create_dir_all(&expanded_dir).ok();

    let remote_filename = std::path::Path::new(remote_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("downloaded.bin");
    let output_path = format!("{}/{}", expanded_dir, remote_filename);

    if let Ok(mut prog) = crate::system_monitor::TRANSFER_PROGRESS.lock() {
        *prog = serde_json::json!({
            "active": true,
            "filename": remote_filename,
            "bytes_sent": 0,
            "total_bytes": 0,
            "progress_pct": 0.0,
            "speed_mbps": 0.0,
            "status": "downloading"
        });
    }

    let host_c = host.to_string();
    let path_c = remote_path.to_string();
    let out_c = output_path.clone();
    let fname = remote_filename.to_string();
    let fname_cb = fname.clone();

    tokio::spawn(async move {
        let cb = move |received: u64, total: u64| {
            if let Ok(mut prog) = crate::system_monitor::TRANSFER_PROGRESS.lock() {
                let pct = if total > 0 { (received as f64 / total as f64) * 100.0 } else { 0.0 };
                *prog = serde_json::json!({
                    "active": received < total,
                    "filename": fname_cb,
                    "bytes_sent": received,
                    "total_bytes": total,
                    "progress_pct": pct,
                    "speed_mbps": 0.0,
                    "status": if received >= total { "completed" } else { "downloading" }
                });
            }
        };
        match crate::http_transfer::http_download(&host_c, port, &path_c, &out_c, Some(std::sync::Arc::new(cb))).await {
            Ok(session) => {
                crate::system_monitor::log_op("info", &format!("HTTP download complete: {} ({})", session.filename, session.original_size));
                if let Ok(mut prog) = crate::system_monitor::TRANSFER_PROGRESS.lock() {
                    *prog = serde_json::json!({
                        "active": false,
                        "filename": session.filename,
                        "bytes_sent": session.original_size,
                        "total_bytes": session.original_size,
                        "progress_pct": 100.0,
                        "speed_mbps": session.average_speed_mbps,
                        "status": "completed"
                    });
                }
            }
            Err(e) => {
                crate::system_monitor::log_op("error", &format!("HTTP download failed: {}", e));
                if let Ok(mut prog) = crate::system_monitor::TRANSFER_PROGRESS.lock() {
                    *prog = serde_json::json!({
                        "active": false,
                        "filename": fname,
                        "bytes_sent": 0,
                        "total_bytes": 0,
                        "progress_pct": 0.0,
                        "speed_mbps": 0.0,
                        "status": "failed"
                    });
                }
            }
        }
    });

    send_json_response(stream, 200, "{\"success\":true,\"message\":\"Download started\"}").await?;
    Ok(())
}

fn list_directory_contents(dir: &str) -> anyhow::Result<Vec<serde_json::Value>> {
    let path = std::path::Path::new(dir);
    if !path.is_dir() {
        return Err(anyhow::anyhow!("Not a directory: {}", dir));
    }
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let metadata = entry.metadata()?;
        entries.push(serde_json::json!({
            "name": entry.file_name().to_string_lossy(),
            "path": entry.path().to_string_lossy(),
            "is_dir": ft.is_dir(),
            "is_file": ft.is_file(),
            "size": metadata.len(),
            "modified": metadata.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
        }));
    }
    entries.sort_by(|a, b| {
        let a_dir = a.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false);
        let b_dir = b.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false);
        b_dir.cmp(&a_dir).then_with(|| {
            a.get("name").and_then(|v| v.as_str()).unwrap_or("").cmp(
                b.get("name").and_then(|v| v.as_str()).unwrap_or("")
            )
        })
    });
    Ok(entries)
}

/// Send a simple HTTP POST to a URL with JSON body (fire-and-forget).
async fn send_http_post(url: &str, body: &str) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    let (host, port, path) = parse_url(url)?;
    let mut stream = tokio::net::TcpStream::connect(format!("{}:{}", host, port)).await?;
    let request = format!(
        "POST {} HTTP/1.1\r\n\
         Host: {}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        path, host, body.len(), body
    );
    stream.write_all(request.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

fn parse_url(url: &str) -> anyhow::Result<(String, u16, String)> {
    let url = url.strip_prefix("http://").unwrap_or(url);
    let (host_part, path) = if let Some(idx) = url.find('/') {
        (&url[..idx], &url[idx..])
    } else {
        (url, "/")
    };
    let (host, port) = if let Some(idx) = host_part.find(':') {
        let p: u16 = host_part[idx + 1..].parse().map_err(|_| anyhow::anyhow!("Invalid port"))?;
        (&host_part[..idx], p)
    } else {
        (host_part, 80u16)
    };
    Ok((host.to_string(), port, path.to_string()))
}

fn get_lan_ip() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

async fn serve_file_once(path: String, filename: String, port: u16) {
    let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await {
        Ok(l) => l,
        Err(_) => return,
    };

    // Wait for one connection with a 10-minute timeout
    let accept_result = tokio::time::timeout(
        tokio::time::Duration::from_secs(600),
        listener.accept()
    ).await;

    if let Ok(Ok((mut stream, _addr))) = accept_result {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut req_buf = vec![0u8; 4096];
        let _ = stream.read(&mut req_buf).await; // consume request

        match tokio::fs::File::open(&path).await {
            Ok(mut file) => {
                let file_size = file.metadata().await.map(|m| m.len()).unwrap_or(0);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"{}\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    filename,
                    file_size
                );
                let _ = stream.write_all(header.as_bytes()).await;
                let mut buf = vec![0u8; 65536];
                loop {
                    match file.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => { let _ = stream.write_all(&buf[..n]).await; }
                        Err(_) => break,
                    }
                }
            }
            Err(_) => {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
            }
        }
    }

    // Cleanup temp file
    let _ = tokio::fs::remove_file(&path).await;
}

fn get_dashboard_html() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
    <title>PDOS / HUB</title>
    <link href="https://fonts.googleapis.com/css2?family=Space+Mono:ital,wght@0,400;0,700;1,400&family=Inter:wght@400;500;600&display=swap" rel="stylesheet">
    <style>
        :root {
            --bg: #000000;
            --fg: #ffffff;
            --muted: #666666;
            --border: #333333;
            --hover-bg: #ffffff;
            --hover-fg: #000000;
        }

        * {
            box-sizing: border-box;
            margin: 0;
            padding: 0;
            -webkit-font-smoothing: antialiased;
        }

        body {
            font-family: "Space Mono", monospace;
            background-color: var(--bg);
            color: var(--fg);
            min-height: 100vh;
            display: flex;
            flex-direction: column;
            align-items: center;
            overflow-x: hidden;
            text-transform: uppercase;
        }

        /* Top Navigation */
        nav {
            width: 100%;
            padding: 24px 40px;
            display: flex;
            justify-content: space-between;
            align-items: center;
            border-bottom: 1px solid var(--border);
            z-index: 10;
        }

        .brand {
            font-size: 1.2rem;
            font-weight: 700;
            letter-spacing: 2px;
            display: flex;
            align-items: center;
            gap: 12px;
        }

        .version {
            font-size: 0.8rem;
            color: var(--muted);
            border: 1px solid var(--border);
            padding: 4px 12px;
        }

        /* Main Container */
        .container {
            width: 100%;
            max-width: 900px;
            margin-top: 40px;
            padding: 0 20px;
            display: flex;
            flex-direction: column;
            align-items: center;
            gap: 40px;
        }

        h1 {
            font-weight: 400;
            font-size: 2rem;
            letter-spacing: 4px;
            text-align: center;
        }

        .subtitle {
            color: var(--muted);
            font-size: 0.9rem;
            text-align: center;
            margin-top: -20px;
            letter-spacing: 1px;
        }

        /* Device Grid */
        .device-grid {
            display: flex;
            flex-wrap: wrap;
            justify-content: center;
            gap: 20px;
            width: 100%;
            min-height: 200px;
        }

        .device-card {
            display: flex;
            flex-direction: column;
            align-items: center;
            gap: 16px;
            cursor: pointer;
            padding: 24px;
            border: 1px solid var(--border);
            transition: all 0.2s ease;
            width: 100%;
            max-width: 200px;
        }

        .device-card:hover {
            background: var(--hover-bg);
            color: var(--hover-fg);
            border-color: var(--fg);
        }

        .device-card:hover .device-status {
            color: var(--hover-fg);
        }

        .device-icon {
            font-size: 32px;
            font-family: "Inter", sans-serif;
        }

        .device-name {
            font-size: 0.85rem;
            font-weight: 700;
            text-align: center;
            word-break: break-all;
        }
        
        .device-status {
            font-size: 0.75rem;
            color: var(--muted);
            display: flex;
            align-items: center;
            gap: 6px;
            transition: color 0.2s;
        }
        
        .status-dot {
            width: 6px;
            height: 6px;
            background: currentColor;
            border-radius: 50%;
        }

        /* Scan Button */
        .scan-btn {
            background: var(--bg);
            border: 1px solid var(--fg);
            color: var(--fg);
            padding: 16px 32px;
            font-size: 0.9rem;
            font-family: inherit;
            text-transform: uppercase;
            letter-spacing: 2px;
            cursor: pointer;
            transition: all 0.2s ease;
            width: 100%;
            max-width: 300px;
        }

        .scan-btn:hover {
            background: var(--fg);
            color: var(--bg);
        }

        /* Modal / Action Sheet */
        .modal-overlay {
            position: fixed;
            top: 0; left: 0; right: 0; bottom: 0;
            background: rgba(0,0,0,0.95);
            display: flex;
            justify-content: center;
            align-items: center;
            opacity: 0;
            pointer-events: none;
            transition: opacity 0.3s ease;
            z-index: 100;
            padding: 20px;
        }

        .modal-overlay.active {
            opacity: 1;
            pointer-events: auto;
        }

        .action-sheet {
            background: var(--bg);
            border: 1px solid var(--border);
            width: 100%;
            max-width: 600px;
            max-height: 90vh;
            overflow-y: auto;
            padding: 30px;
            display: flex;
            flex-direction: column;
            gap: 24px;
        }

        .sheet-header {
            display: flex;
            justify-content: space-between;
            align-items: flex-start;
            border-bottom: 1px solid var(--border);
            padding-bottom: 16px;
        }

        .sheet-header h2 {
            font-weight: 400;
            font-size: 1.1rem;
            letter-spacing: 2px;
        }

        .close-btn {
            background: none;
            border: none;
            color: var(--fg);
            font-family: inherit;
            font-size: 1rem;
            cursor: pointer;
            text-decoration: underline;
        }
        .close-btn:hover { color: var(--muted); }

        /* Action Grid */
        .action-grid {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 16px;
        }

        .action-btn {
            background: var(--bg);
            border: 1px solid var(--border);
            color: var(--fg);
            padding: 20px;
            cursor: pointer;
            transition: all 0.2s ease;
            display: flex;
            flex-direction: column;
            align-items: flex-start;
            gap: 12px;
            font-family: inherit;
            text-transform: uppercase;
        }

        .action-btn:hover {
            background: var(--fg);
            color: var(--bg);
            border-color: var(--fg);
        }

        .action-title {
            font-size: 0.9rem;
            font-weight: 700;
            letter-spacing: 0;
            text-align: left;
            text-transform: none;
        }

        .action-icon {
            font-size: 22px;
            text-transform: none;
        }

        .action-desc {
            font-size: 0.78rem;
            color: var(--muted);
            text-transform: none;
            font-family: "Inter", sans-serif;
            letter-spacing: 0;
            font-weight: 400;
            line-height: 1.4;
            text-align: left;
        }

        .action-btn:hover .action-desc {
            color: var(--bg);
            opacity: 0.7;
        }

        .view-desc {
            font-size: 0.88rem;
            color: var(--muted);
            text-transform: none;
            font-family: "Inter", sans-serif;
            letter-spacing: 0;
            line-height: 1.5;
            margin-bottom: -4px;
        }

        .drop-zone {
            border: 1px dashed var(--border);
            padding: 32px 20px;
            display: flex;
            justify-content: center;
            align-items: center;
            cursor: pointer;
            transition: all 0.2s ease;
            text-align: center;
        }
        .drop-zone:hover, .drop-zone.drag-over {
            border-color: var(--fg);
            background: rgba(255,255,255,0.03);
        }
        .drop-icon {
            font-size: 28px;
            display: block;
            margin-bottom: 8px;
            text-transform: none;
        }
        .drop-label {
            font-size: 0.9rem;
            font-weight: 600;
            color: var(--fg);
            text-transform: none;
            font-family: "Inter", sans-serif;
            letter-spacing: 0;
        }
        .drop-sub {
            font-size: 0.8rem;
            color: var(--muted);
            text-transform: none;
            font-family: "Inter", sans-serif;
            margin-top: 4px;
        }
        /* File Transfer Tabs */
        .file-tabs {
            display: flex;
            gap: 0;
            border: 1px solid var(--border);
        }
        .file-tab {
            flex: 1;
            background: none;
            border: none;
            border-right: 1px solid var(--border);
            color: var(--muted);
            padding: 12px 8px;
            cursor: pointer;
            font-family: "Inter", sans-serif;
            font-size: 0.82rem;
            font-weight: 600;
            text-transform: none;
            letter-spacing: 0;
            transition: all 0.15s;
        }
        .file-tab:last-child { border-right: none; }
        .file-tab.active { background: var(--fg); color: var(--bg); }
        .file-tab:hover:not(.active) { background: rgba(255,255,255,0.05); color: var(--fg); }
        .receive-list {
            min-height: 100px;
            max-height: 200px;
            overflow-y: auto;
            border: 1px solid var(--border);
            padding: 12px;
            display: flex;
            flex-direction: column;
            gap: 8px;
        }
        .receive-item {
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 12px;
            padding: 8px 0;
            border-bottom: 1px solid var(--border);
            font-family: "Inter", sans-serif;
            font-size: 0.82rem;
            text-transform: none;
            color: var(--fg);
        }
        .receive-item:last-child { border-bottom: none; }
        .receive-item-name { font-weight: 600; }
        .receive-item-size { color: var(--muted); font-size: 0.75rem; }
        .btn-open { background: none; border: 1px solid var(--border); color: var(--fg); padding: 4px 10px; cursor: pointer; font-size: 0.75rem; font-family: "Inter", sans-serif; }

        .btn-primary:disabled {
            opacity: 0.3;
            cursor: not-allowed;
        }
        .btn-primary:disabled:hover {
            background: var(--fg);
            color: var(--bg);
        }

        /* Form Layouts within Modal */
        .sub-view {
            display: none;
            flex-direction: column;
            gap: 20px;
        }
        
        .sub-view.active { display: flex; }
        .main-view.hidden { display: none; }

        input, textarea {
            background: var(--bg);
            border: 1px solid var(--border);
            padding: 16px;
            color: var(--fg);
            font-family: inherit;
            font-size: 0.9rem;
            width: 100%;
            border-radius: 0;
        }

        input:focus, textarea:focus {
            outline: none;
            border-color: var(--fg);
        }

        .btn-primary {
            background: var(--fg);
            color: var(--bg);
            border: 1px solid var(--fg);
            padding: 16px;
            font-size: 0.9rem;
            font-family: inherit;
            text-transform: uppercase;
            letter-spacing: 2px;
            cursor: pointer;
            width: 100%;
            transition: all 0.2s;
        }
        .btn-primary:hover { 
            background: var(--bg);
            color: var(--fg);
        }

        .btn-secondary {
            background: var(--bg);
            color: var(--fg);
            border: 1px solid var(--border);
            padding: 16px;
            font-size: 0.9rem;
            font-family: inherit;
            text-transform: uppercase;
            letter-spacing: 2px;
            cursor: pointer;
            width: 100%;
            transition: all 0.2s;
        }
        .btn-secondary:hover {
            border-color: var(--fg);
        }
        
        .btn-group {
            display: flex;
            gap: 12px;
            width: 100%;
        }

        .back-btn {
            background: none;
            border: none;
            color: var(--muted);
            font-family: inherit;
            font-size: 0.85rem;
            text-transform: uppercase;
            letter-spacing: 1px;
            cursor: pointer;
            text-align: left;
            margin-bottom: -10px;
        }
        .back-btn:hover { color: var(--fg); }

        /* Toast Notification */
        .toast-container {
            position: fixed;
            bottom: 20px;
            left: 20px;
            right: 20px;
            display: flex;
            flex-direction: column;
            gap: 12px;
            z-index: 1000;
            pointer-events: none;
        }

        .toast {
            background: var(--fg);
            color: var(--bg);
            border: 1px solid var(--fg);
            padding: 16px;
            font-size: 0.85rem;
            letter-spacing: 1px;
            display: flex;
            align-items: center;
            gap: 12px;
            box-shadow: 0 10px 30px rgba(0,0,0,0.5);
            transition: opacity 0.3s;
        }

        .toast.error { 
            background: var(--bg);
            color: var(--fg);
            border-color: var(--fg);
        }

        .terminal-output {
            background: var(--bg);
            border: 1px solid var(--border);
            font-family: inherit;
            padding: 16px;
            font-size: 0.85rem;
            height: 150px;
            overflow-y: auto;
            color: var(--fg);
            white-space: pre-wrap;
            text-transform: none;
        }
        
        /* Activity Log */
        .log-panel {
            width: 100%;
            max-width: 900px;
            margin: 0 20px 40px 20px;
            border: 1px solid var(--border);
        }
        .log-header {
            padding: 12px 16px;
            font-size: 0.8rem;
            color: var(--muted);
            border-bottom: 1px solid var(--border);
            letter-spacing: 1px;
        }
        .activity-log {
            padding: 12px 16px;
            height: 130px;
            overflow-y: auto;
            display: flex;
            flex-direction: column;
            gap: 4px;
        }
        .log-entry {
            font-size: 0.82rem;
            color: var(--muted);
            font-family: "Inter", sans-serif;
            text-transform: none;
            letter-spacing: 0;
            line-height: 1.5;
        }
        .log-entry.log-success { color: var(--fg); }
        .log-entry.log-error { color: #aaaaaa; text-decoration: line-through; }
        .log-time {
            font-family: "Space Mono", monospace;
            font-size: 0.75rem;
            color: #444;
            margin-right: 8px;
        }

        /* Responsive Media Queries */
        @media (max-width: 600px) {
            nav {
                padding: 20px;
            }
            
            h1 {
                font-size: 1.5rem;
                letter-spacing: 2px;
            }
            
            .action-grid {
                grid-template-columns: 1fr;
            }
            
            .action-sheet {
                padding: 20px;
            }
            
            .btn-group {
                flex-direction: column;
            }
            
            .device-card {
                max-width: 100%;
            }
            
            .toast-container {
                bottom: 20px;
                left: 20px;
                right: 20px;
                align-items: center;
            }
            .toast {
                width: 100%;
                justify-content: center;
                text-align: center;
            }
        }
    </style>
</head>
<body>

    <nav>
        <div class="brand">
            [ PDOS ]
        </div>
        <div style="display:flex;gap:16px;align-items:center;">
            <button class="nav-tab active" id="tabDevices" onclick="switchMainTab('devices')">Devices</button>
            <button class="nav-tab" id="tabObservability" onclick="switchMainTab('observability')">Observability</button>
            <div class="version">V0.3</div>
        </div>
    </nav>
    <style>
        .nav-tab {
            background: none;
            border: 1px solid var(--border);
            color: var(--muted);
            padding: 8px 16px;
            font-family: inherit;
            font-size: 0.78rem;
            cursor: pointer;
            letter-spacing: 1px;
            text-transform: uppercase;
            transition: all 0.2s;
        }
        .nav-tab:hover {
            border-color: var(--fg);
            color: var(--fg);
        }
        .nav-tab.active {
            background: var(--fg);
            color: var(--bg);
            border-color: var(--fg);
        }
    </style>

    <div id="pageDevices">
        <div class="container">
            <div>
                <h1>Nearby Devices</h1>
                <p class="subtitle">Tap a device to control it</p>
            </div>

            <div class="device-grid" id="deviceGrid">
                <!-- Populated via JS -->
            </div>
        </div>
    </div>

    <!-- ═══════════════════════════════════════════════════════════════ -->
    <!--  OBSERVABILITY PAGE                                           -->
    <!-- ═══════════════════════════════════════════════════════════════ -->
    <div id="pageObservability" style="display:none;">
    <div class="container" style="max-width:1100px;">

        <!-- ── Health Score ── -->
        <div class="obs-section">
            <div class="obs-section-header">System Health</div>
            <div class="obs-grid" style="grid-template-columns: 1fr 2fr;">
                <div class="obs-card">
                    <div id="overallHealthScore" class="health-score-large">--</div>
                    <div class="health-score-label">Overall Health</div>
                </div>
                <div class="obs-card">
                    <div class="health-bars">
                        <div class="health-bar-row"><span>CPU</span><div class="health-bar-track"><div id="healthCpu" class="health-bar-fill" style="width:0%"></div></div><span id="healthCpuLabel">0</span></div>
                        <div class="health-bar-row"><span>Network</span><div class="health-bar-track"><div id="healthNetwork" class="health-bar-fill" style="width:0%"></div></div><span id="healthNetworkLabel">0</span></div>
                        <div class="health-bar-row"><span>Disk</span><div class="health-bar-track"><div id="healthDisk" class="health-bar-fill" style="width:0%"></div></div><span id="healthDiskLabel">0</span></div>
                        <div class="health-bar-row"><span>Integrity</span><div class="health-bar-track"><div id="healthIntegrity" class="health-bar-fill" style="width:0%"></div></div><span id="healthIntegrityLabel">0</span></div>
                        <div class="health-bar-row"><span>Recovery</span><div class="health-bar-track"><div id="healthRecovery" class="health-bar-fill" style="width:0%"></div></div><span id="healthRecoveryLabel">0</span></div>
                    </div>
                </div>
            </div>
        </div>
        <style>
            .health-score-large { font-size:3.5rem; font-weight:700; text-align:center; letter-spacing:0; }
            .health-score-label { text-align:center; color:var(--muted); font-size:0.8rem; margin-top:4px; }
            .health-bars { display:flex; flex-direction:column; gap:10px; }
            .health-bar-row { display:flex; align-items:center; gap:10px; font-family:"Inter",sans-serif; font-size:0.82rem; text-transform:none; }
            .health-bar-track { flex:1; height:8px; background:#222; border-radius:4px; overflow:hidden; }
            .health-bar-fill { height:100%; background:var(--fg); border-radius:4px; transition:width 0.5s ease; }
        </style>

        <!-- ── Current Metrics ── -->
        <div class="obs-section">
            <div class="obs-section-header">Live Metrics</div>
            <div class="obs-grid" style="grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));">
                <div class="metric-card"><div class="metric-label">CPU</div><div id="liveCpu" class="metric-value">--</div></div>
                <div class="metric-card"><div class="metric-label">Memory</div><div id="liveMemory" class="metric-value">--</div></div>
                <div class="metric-card"><div class="metric-label">TX</div><div id="liveTx" class="metric-value">--</div></div>
                <div class="metric-card"><div class="metric-label">RX</div><div id="liveRx" class="metric-value">--</div></div>
                <div class="metric-card"><div class="metric-label">CPU Temp</div><div id="liveCpuTemp" class="metric-value">--</div></div>
                <div class="metric-card"><div class="metric-label">Fan</div><div id="liveFan" class="metric-value">--</div></div>
                <div class="metric-card"><div class="metric-label">Battery</div><div id="liveBattery" class="metric-value">--</div></div>
                <div class="metric-card"><div class="metric-label">Thermal State</div><div id="liveThermal" class="metric-value">--</div></div>
            </div>
        </div>

        <!-- ── Process-Level Metrics ── -->
        <div class="obs-section">
            <div class="obs-section-header">Process-Level Monitoring</div>
            <div class="obs-grid" style="grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));">
                <div class="metric-card"><div class="metric-label">Rust Daemon CPU</div><div id="procRustCpu" class="metric-value">--</div></div>
                <div class="metric-card"><div class="metric-label">Rust Daemon RAM</div><div id="procRustRam" class="metric-value">--</div></div>
                <div class="metric-card"><div class="metric-label">Hash Thread CPU</div><div id="procHashCpu" class="metric-value">--</div></div>
                <div class="metric-card"><div class="metric-label">Hash Thread RAM</div><div id="procHashRam" class="metric-value">--</div></div>
            </div>
        </div>

        <!-- ── Transfer History ── -->
        <div class="obs-section">
            <div class="obs-section-header">Transfer History</div>
            <div id="transferHistoryList"></div>
        </div>

        <!-- ── Transfer Detail Report ── -->
        <div id="transferDetail" style="display:none;">
            <button class="back-btn" onclick="document.getElementById('transferDetail').style.display='none';">&#8592; Back to History</button>
            <div class="obs-section">
                <div class="obs-section-header" id="detailTitle">Transfer Report</div>
                <div id="transferReportContent"></div>
            </div>
        </div>

        <!-- ── Waterfall Timeline ── -->
        <div id="waterfallContainer" style="display:none;">
            <div class="obs-section">
                <div class="obs-section-header">Transfer Waterfall</div>
                <div id="waterfallContent"></div>
            </div>
        </div>

        <!-- ─── Protocol Statistics ── -->
        <div class="obs-section">
            <div class="obs-section-header">Protocol Statistics</div>
            <div class="obs-grid" id="protocolStatsGrid" style="grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));"></div>
        </div>

        <!-- ── Compression Analytics ── -->
        <div class="obs-section">
            <div class="obs-section-header">Compression Analytics</div>
            <div id="compressionAnalyticsList"></div>
        </div>

        <!-- ── Storage Forecast ── -->
        <div class="obs-section">
            <div class="obs-section-header">Storage Forecast</div>
            <div id="storageForecastContent"></div>
        </div>

        <!-- ── Network Path Analyzer ── -->
        <div class="obs-section">
            <div class="obs-section-header">Network Path</div>
            <div id="networkPathContent"></div>
        </div>

        <!-- ── Bottleneck Detection ── -->
        <div class="obs-section">
            <div class="obs-section-header">Bottleneck Detection</div>
            <div id="bottleneckContent"></div>
        </div>

        <!-- ── Buffer Analysis ── -->
        <div class="obs-section">
            <div class="obs-section-header">Buffer Analysis</div>
            <div id="bufferAnalysisContent"></div>
        </div>

    </div>
    </div>

    <style>
        .obs-section { width:100%; margin-bottom:20px; }
        .obs-section-header { font-size:0.85rem; color:var(--muted); letter-spacing:1px; margin-bottom:12px; border-bottom:1px solid var(--border); padding-bottom:8px; }
        .obs-grid { display:grid; gap:12px; }
        .obs-card { border:1px solid var(--border); padding:20px; }
        .metric-card { border:1px solid var(--border); padding:16px; text-align:center; }
        .metric-label { font-size:0.72rem; color:var(--muted); letter-spacing:1px; margin-bottom:6px; }
        .metric-value { font-size:1.3rem; font-weight:700; letter-spacing:0; }
        .hist-item { border:1px solid var(--border); padding:12px; margin-bottom:8px; font-family:"Inter",sans-serif; text-transform:none; font-size:0.85rem; cursor:pointer; transition:all 0.15s; }
        .hist-item:hover { background:rgba(255,255,255,0.03); border-color:var(--fg); }
        .hist-item-header { display:flex; justify-content:space-between; align-items:center; }
        .hist-item-header .name { font-weight:600; color:var(--fg); }
        .hist-item-header .status { font-size:0.75rem; }
        .hist-item-details { display:flex; gap:16px; margin-top:6px; color:var(--muted); font-size:0.78rem; }
        .badge { display:inline-block; padding:2px 8px; font-size:0.7rem; font-family:"Space Mono",monospace; border:1px solid var(--border); }
        .badge.ok { border-color:#4CAF50; color:#4CAF50; }
        .badge.warn { border-color:#FF9800; color:#FF9800; }
        .badge.err { border-color:#f44336; color:#f44336; }
        .waterfall-bar { display:flex; align-items:center; gap:8px; margin-bottom:4px; font-family:"Inter",sans-serif; font-size:0.8rem; text-transform:none; }
        .waterfall-bar .label { width:90px; text-align:right; color:var(--muted); }
        .waterfall-bar .track { flex:1; height:20px; background:#111; position:relative; border-radius:3px; overflow:hidden; }
        .waterfall-bar .fill { height:100%; background:var(--fg); border-radius:3px; transition:width 0.3s; }
        .waterfall-bar .time { width:70px; color:var(--muted); font-size:0.72rem; }
        .phase-compression .fill { background:#FF9800; }
        .phase-authentication .fill { background:#2196F3; }
        .phase-discovery .fill { background:#9C27B0; }
        .phase-tls .fill { background:#4CAF50; }
        .phase-streaming .fill { background:var(--fg); }
        .phase-hash .fill { background:#607D8B; }
        .phase-archive .fill { background:#795548; }

        .report-section { margin-bottom:20px; }
        .report-section h4 { font-size:0.8rem; color:var(--muted); letter-spacing:1px; margin-bottom:8px; border-bottom:1px solid var(--border); padding-bottom:4px; }
        .report-grid { display:grid; grid-template-columns:1fr 1fr; gap:6px; font-family:"Inter",sans-serif; font-size:0.82rem; text-transform:none; }
        .report-grid .key { color:var(--muted); }
        .report-grid .val { text-align:right; color:var(--fg); font-weight:500; }

        .proto-stat { border:1px solid var(--border); padding:12px; text-align:center; }
        .proto-stat .num { font-size:1.5rem; font-weight:700; }
        .proto-stat .label { font-size:0.7rem; color:var(--muted); margin-top:4px; }
    </style>

    <!-- Action Sheet Modal -->
    <div class="modal-overlay" id="actionModal" onclick="closeModal(event)">
        <div class="action-sheet" onclick="event.stopPropagation()">
            
            <!-- Main Grid View -->
            <div id="viewMain" class="main-view">
                <div class="sheet-header">
                    <div>
                        <h2 id="modalDeviceName">Device</h2>
                        <span id="modalDeviceId" style="font-size: 0.75rem; color: var(--muted);">Connected</span>
                    </div>
                    <button class="close-btn" onclick="closeModal(true)">Done</button>
                </div>

                <div class="action-grid">
                    <button class="action-btn" onclick="openSubView('viewNotify')">
                        <span class="action-icon">&#128276;</span>
                        <span class="action-title">Send Alert</span>
                        <span class="action-desc">Pop a notification on this device</span>
                    </button>
                    <button class="action-btn" onclick="openSubView('viewClipboard')">
                        <span class="action-icon">&#128203;</span>
                        <span class="action-title">Clipboard</span>
                        <span class="action-desc">Share copied text between devices</span>
                    </button>
                    <button class="action-btn" onclick="openSubView('viewTerminal')">
                        <span class="action-icon">&#9881;&#65039;</span>
                        <span class="action-title">Run Command</span>
                        <span class="action-desc">Execute a shell command remotely</span>
                    </button>
                    <button class="action-btn" onclick="openSubView('viewFile')">
                        <span class="action-icon">&#128228;</span>
                        <span class="action-title">Transfer File</span>
                        <span class="action-desc">Send or receive files wirelessly</span>
                    </button>
                </div>
            </div>

            <!-- Sub View: Notification -->
            <div id="viewNotify" class="sub-view">
                <button class="back-btn" onclick="backToMain()">&#8592; Back</button>
                <div class="sheet-header"><h2>Send Alert</h2></div>
                <p class="view-desc">Send a notification that pops up on the selected device.</p>
                <input type="text" id="notifyTitle" placeholder="Title  (e.g. Reminder)">
                <input type="text" id="notifyBody" placeholder="Message  (e.g. Don't forget lunch!)">
                <button class="btn-primary" onclick="sendNotification()">Send Notification</button>
            </div>

            <!-- Sub View: Clipboard -->
            <div id="viewClipboard" class="sub-view">
                <button class="back-btn" onclick="backToMain()">&#8592; Back</button>
                <div class="sheet-header"><h2>Clipboard Sync</h2></div>
                <p class="view-desc">Share your clipboard between your Mac and this device instantly.</p>
                <textarea id="clipText" rows="4" placeholder="Text to send..." style="text-transform:none;"></textarea>
                <button class="btn-primary" onclick="setClipboard(false)">Send to Device</button>
                <button class="btn-secondary" onclick="setClipboard(true)">Send Mac Clipboard to Device</button>
                <button class="btn-secondary" onclick="getClipboard()">Copy from Device to Mac</button>
            </div>

            <!-- Sub View: Terminal -->
            <div id="viewTerminal" class="sub-view">
                <button class="back-btn" onclick="backToMain()">&#8592; Back</button>
                <div class="sheet-header"><h2>Run Command</h2></div>
                <p class="view-desc">Run a shell command on this device and see the output.</p>
                <div class="terminal-output" id="termOutput"></div>
                <div class="btn-group">
                    <input type="text" id="termCommand" placeholder="e.g. uname -a" style="text-transform:none;" onkeydown="if(event.key === 'Enter') runCommand()">
                    <button class="btn-primary" style="width: auto; padding: 0 32px;" onclick="runCommand()">Run</button>
                </div>
            </div>

            <!-- Sub View: File Transfer -->
            <div id="viewFile" class="sub-view">
                <button class="back-btn" onclick="backToMain()">&#8592; Back</button>
                <div class="sheet-header"><h2>Transfer File</h2></div>

                <!-- Tab switcher -->
                <div class="file-tabs">
                    <button class="file-tab active" id="tabSend" onclick="switchFileTab('send')">&#128228; Send to Device</button>
                    <button class="file-tab" id="tabReceive" onclick="switchFileTab('receive')">&#128229; Receive from Device</button>
                </div>

                <!-- SEND panel -->
                <div id="panelSend">
                    <p class="view-desc">Pick any file and send it at full Wi-Fi speed. No size limit.</p>
                    <div style="display:flex; gap:10px; margin-bottom: 10px;">
                        <button class="btn-secondary" style="flex:1;" onclick="document.getElementById('filePicker').click()">Select Files</button>
                        <button class="btn-secondary" style="flex:1;" onclick="document.getElementById('folderPicker').click()">Select Folder</button>
                    </div>
                    <div id="dropZone" class="drop-zone" onclick="document.getElementById('filePicker').click()">
                        <input type="file" id="filePicker" style="display:none;" multiple onchange="onFilePicked(this)">
                        <input type="file" id="folderPicker" style="display:none;" webkitdirectory multiple onchange="onFilePicked(this)">
                        <div id="dropZoneContent">
                            <span class="drop-icon">&#128228;</span>
                            <p class="drop-label">Click or drag & drop files/folders here</p>
                            <p class="drop-sub">Send multiple items or entire folders at once</p>
                        </div>
                    </div>
                    <div id="fileResult" style="display:none; font-size:0.85rem; color:var(--muted); font-family:'Inter',sans-serif; text-transform:none; padding:4px 0;"></div>
                    <button class="btn-primary" id="btnSendFile" onclick="sendFile()" disabled>Send to Device</button>
                </div>

                <!-- RECEIVE panel -->
                <div id="panelReceive" style="display:none;">
                    <p class="view-desc">Files shared from the Android device appear here automatically. Share any file using the Android share sheet and select PDOS.</p>
                    <div class="receive-list" id="receiveList">
                        <p style="color:var(--muted); font-size:0.85rem; text-transform:none; font-family:'Inter',sans-serif;">No files received yet. Share a file from your Android device.</p>
                    </div>
                    <button class="btn-secondary" onclick="pollReceived()">Refresh</button>
                </div>
            </div>

        </div>
    </div>

    <!-- Activity Log Panel -->
    <div class="log-panel">
        <div class="log-header">Activity Log</div>
        <div class="activity-log" id="activityLog"></div>
    </div>

    <div class="toast-container" id="toastContainer"></div>

    <script>
        let selectedNodeId = null;

        function showToast(msg, type = 'success') {
            const container = document.getElementById('toastContainer');
            const toast = document.createElement('div');
            toast.className = 'toast ' + type;
            toast.innerHTML = type === 'success' ? '[OK] ' + msg : '[ERR] ' + msg;
            container.appendChild(toast);
            
            setTimeout(() => {
                toast.style.opacity = '0';
                setTimeout(() => toast.remove(), 300);
            }, 3000);
        }

        async function scanDevices() {
            const btn = document.getElementById('btnScan');
            btn.innerHTML = '[ SCANNING... ]';
            
            try {
                const response = await fetch('/api/devices');
                if (!response.ok) throw new Error("Connection failed");
                const devices = await response.json();
                
                const grid = document.getElementById('deviceGrid');
                grid.innerHTML = "";

                if (devices.length === 0) {
                    grid.innerHTML = '<div style="color:var(--muted); padding:40px;">[ NO NODES DETECTED ]</div>';
                } else {
                    devices.forEach((d) => {
                        const platformStr = typeof d.platform === 'object' ? 'unknown' : d.platform.toLowerCase();
                        let icon = "PC";
                        if (platformStr === 'mac') icon = "MAC";
                        if (platformStr === 'android') icon = "AND";
                        
                        const card = document.createElement('div');
                        card.className = 'device-card';
                        card.onclick = () => openModal(d.node_id, d.name);
                        
                        card.innerHTML = '<div class="device-icon">' + icon + '</div><div class="device-name">' + d.name + '</div><div class="device-status"><span class="status-dot"></span> ONLINE</div>';
                        grid.appendChild(card);
                    });
                    if (devices.length > 0) showToast('FOUND ' + devices.length + ' NODE(S)');
                }
            } catch (err) {
                showToast(err.message, 'error');
            } finally {
                btn.innerHTML = '[ SCAN NETWORK ]';
            }
        }

        function openModal(id, name) {
            selectedNodeId = id;
            document.getElementById('modalDeviceName').innerText = name.toUpperCase();
            document.getElementById('modalDeviceId').innerText = id.split('-')[0].toUpperCase();
            
            backToMain();
            document.getElementById('actionModal').classList.add('active');
        }

        function closeModal(event) {
            if (event === true || event.target.id === 'actionModal') {
                document.getElementById('actionModal').classList.remove('active');
                setTimeout(backToMain, 300);
            }
        }

        function openSubView(id) {
            document.getElementById('viewMain').classList.add('hidden');
            document.getElementById(id).classList.add('active');
        }

        function backToMain() {
            document.querySelectorAll('.sub-view').forEach(el => el.classList.remove('active'));
            document.getElementById('viewMain').classList.remove('hidden');
        }

        // Action Implementations
        async function sendNotification() {
            const title = document.getElementById('notifyTitle').value;
            const body = document.getElementById('notifyBody').value;
            try {
                const res = await fetch('/api/notify', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ node_id: selectedNodeId, title, body })
                });
                if (res.ok) showToast("NOTIFICATION DISPATCHED");
                else showToast("DISPATCH FAILED", "error");
            } catch (err) { showToast(err.message, "error"); }
        }

        async function getClipboard() {
            try {
                const res = await fetch('/api/clipboard/get', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ node_id: selectedNodeId })
                });
                const data = await res.json();
                if (res.ok) {
                    document.getElementById('clipText').value = data.content;
                    showToast("FETCHED TO LOCAL CLIPBOARD");
                }
                else showToast("FETCH FAILED", "error");
            } catch (err) { showToast(err.message, "error"); }
        }

        async function setClipboard(useLocal) {
            let content = useLocal ? "" : document.getElementById('clipText').value;
            try {
                const res = await fetch('/api/clipboard/set', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ node_id: selectedNodeId, content, use_local: useLocal })
                });
                if (res.ok) showToast(useLocal ? "MAC CLIPBOARD SYNCED" : "TEXT PUSHED");
                else showToast("SYNC FAILED", "error");
            } catch (err) { showToast(err.message, "error"); }
        }

        async function runCommand() {
            const cmd = document.getElementById('termCommand').value;
            const out = document.getElementById('termOutput');
            out.innerHTML += '\n> ' + cmd;
            try {
                const res = await fetch('/api/exec', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ node_id: selectedNodeId, command: cmd })
                });
                const data = await res.json();
                if (res.ok) out.innerHTML += '\n' + data.output;
                else out.innerHTML += '\n[ERR] ' + data.error;
            } catch (err) { out.innerHTML += '\n[ERR] ' + err.message; }
            out.scrollTop = out.scrollHeight;
            document.getElementById('termCommand').value = '';
        }

        let pickedFiles = [];

        function onFilePicked(input) {
            if (!input.files || input.files.length === 0) return;
            pickedFiles = Array.from(input.files);
            const zone = document.getElementById('dropZoneContent');
            if (pickedFiles.length === 1) {
                zone.innerHTML = '<span class="drop-icon">&#128196;</span><p class="drop-label">' + pickedFiles[0].name + '</p><p class="drop-sub">' + (pickedFiles[0].size / 1024).toFixed(1) + ' KB &mdash; click to change</p>';
            } else {
                const totalSize = pickedFiles.reduce((acc, f) => acc + f.size, 0);
                zone.innerHTML = '<span class="drop-icon">&#128196;</span><p class="drop-label">' + pickedFiles.length + ' files selected</p><p class="drop-sub">' + (totalSize / (1024*1024)).toFixed(1) + ' MB total &mdash; click to change</p>';
            }
            document.getElementById('btnSendFile').disabled = false;
            document.getElementById('fileResult').style.display = 'none';
        }

        // Drag and drop support
        document.addEventListener('DOMContentLoaded', () => {
            const zone = document.getElementById('dropZone');
            if (!zone) return;
            zone.addEventListener('dragover', e => { e.preventDefault(); zone.classList.add('drag-over'); });
            zone.addEventListener('dragleave', () => zone.classList.remove('drag-over'));
            zone.addEventListener('drop', e => {
                e.preventDefault();
                zone.classList.remove('drag-over');
                const files = e.dataTransfer.files;
                if (files && files.length > 0) {
                    pickedFiles = Array.from(files);
                    const inner = document.getElementById('dropZoneContent');
                    if (pickedFiles.length === 1) {
                        inner.innerHTML = '<span class="drop-icon">&#128196;</span><p class="drop-label">' + pickedFiles[0].name + '</p><p class="drop-sub">' + (pickedFiles[0].size / 1024).toFixed(1) + ' KB &mdash; click to change</p>';
                    } else {
                        const totalSize = pickedFiles.reduce((acc, f) => acc + f.size, 0);
                        inner.innerHTML = '<span class="drop-icon">&#128196;</span><p class="drop-label">' + pickedFiles.length + ' files selected</p><p class="drop-sub">' + (totalSize / (1024*1024)).toFixed(1) + ' MB total &mdash; click to change</p>';
                    }
                    document.getElementById('btnSendFile').disabled = false;
                }
            });
        });

        function switchFileTab(tab) {
            document.getElementById('panelSend').style.display = tab === 'send' ? 'flex' : 'none';
            document.getElementById('panelReceive').style.display = tab === 'receive' ? 'flex' : 'none';
            document.getElementById('panelSend').style.flexDirection = 'column';
            document.getElementById('tabSend').classList.toggle('active', tab === 'send');
            document.getElementById('tabReceive').classList.toggle('active', tab === 'receive');
            if (tab === 'receive') pollReceived();
        }

        async function pollReceived() {
            try {
                const res = await fetch('/api/pending-transfers');
                const items = await res.json();
                const list = document.getElementById('receiveList');
                if (!items || items.length === 0) {
                    list.innerHTML = '<p style="color:var(--muted); font-size:0.85rem; text-transform:none; font-family:Inter,sans-serif;">No files received yet. Share a file from your Android device.</p>';
                    return;
                }
                list.innerHTML = items.map(item => {
                    const sizeMB = (item.size / (1024*1024)).toFixed(2);
                    const date = new Date(item.received_at * 1000).toLocaleTimeString();
                    return '<div class="receive-item"><div><div class="receive-item-name">' + item.filename + '</div><div class="receive-item-size">' + sizeMB + ' MB &mdash; received at ' + date + '</div></div></div>';
                }).join('');
            } catch(e) { console.error(e); }
        }

        // Phase 4A: Poll Sender-Side Progress
        let progressInterval = null;
        async function pollProgress() {
            try {
                const res = await fetch('/api/transfer-status');
                const items = await res.json();
                const container = document.getElementById('fileResult');
                if (!items || items.length === 0) return;
                
                let activeTransfers = false;
                let html = '<div style="display:flex; flex-direction:column; gap:8px;">';
                for (const item of items) {
                    const pct = item.total > 0 ? Math.round((item.sent / item.total) * 100) : 0;
                    const sizeMB = (item.total / (1024*1024)).toFixed(1);
                    const sentMB = (item.sent / (1024*1024)).toFixed(1);
                    const statusText = item.status === 'completed' ? 'Done' : 'Transferring...';
                    if (item.status !== 'completed') activeTransfers = true;
                    
                    html += `
                        <div style="background:#222; border:1px solid #333; padding:8px; border-radius:6px;">
                            <div style="display:flex; justify-content:space-between; margin-bottom:4px;">
                                <span style="color:#ddd; font-weight:500;">${item.filename}</span>
                                <span style="color:#aaa;">${sentMB} / ${sizeMB} MB</span>
                            </div>
                            <div style="width:100%; height:6px; background:#111; border-radius:3px; overflow:hidden;">
                                <div style="width:${pct}%; height:100%; background:var(--primary); transition:width 0.3s;"></div>
                            </div>
                            <div style="text-align:right; margin-top:4px; font-size:0.75rem; color:${item.status === 'completed' ? '#4CAF50' : '#888'};">
                                ${statusText} (${pct}%)
                            </div>
                        </div>
                    `;
                }
                html += '</div>';
                container.innerHTML = html;
                
                if (!activeTransfers && progressInterval) {
                    clearInterval(progressInterval);
                    progressInterval = null;
                    document.getElementById('btnSendFile').innerHTML = 'Send to Device';
                    document.getElementById('btnSendFile').disabled = false;
                }
            } catch (e) { console.error(e); }
        }

        async function sendFile() {
            if (!pickedFiles || pickedFiles.length === 0) { showToast('Please choose a file first', 'error'); return; }
            const btn = document.getElementById('btnSendFile');
            btn.innerHTML = 'Preparing...';
            btn.disabled = true;
            const r = document.getElementById('fileResult');
            r.style.display = 'block';
            r.innerHTML = 'Starting transfer...';

            if (!progressInterval) progressInterval = setInterval(pollProgress, 500);

            let successCount = 0;
            let failCount = 0;
            
            for (let i = 0; i < pickedFiles.length; i++) {
                const file = pickedFiles[i];
                const relativePath = file.webkitRelativePath || file.name;
                
                try {
                    const encodedName = encodeURIComponent(relativePath);
                    const url = '/api/stream-upload?node_id=' + selectedNodeId + '&filename=' + encodedName;
                    
                    // The fetch resolves quickly because it streams to the Mac's /tmp dir at local disk speed
                    const res = await fetch(url, {
                        method: 'POST',
                        headers: {
                            'Content-Type': 'application/octet-stream',
                            'Content-Length': file.size.toString()
                        },
                        body: file
                    });
                    const data = await res.json();
                    if (res.ok) {
                        successCount++;
                    } else {
                        addLog('Failed: ' + (data.error || 'unknown'), 'error');
                        failCount++;
                    }
                } catch (err) {
                    addLog('Error: ' + err.message, 'error');
                    failCount++;
                }
            }
            btn.innerHTML = 'Waiting for Android...';
            // Button is re-enabled by pollProgress when all items are 'completed'
        }

        async function getFile() {}

        function addLog(msg, type) {
            const log = document.getElementById('activityLog');
            if (!log) return;
            const entry = document.createElement('div');
            entry.className = 'log-entry' + (type ? ' log-' + type : '');
            const time = new Date().toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });
            entry.innerHTML = '<span class="log-time">' + time + '</span> ' + msg;
            log.appendChild(entry);
            log.scrollTop = log.scrollHeight;
        }

        // ── Observability Tab Switching ──
        function switchMainTab(tab) {
            document.getElementById('pageDevices').style.display = tab === 'devices' ? 'block' : 'none';
            document.getElementById('pageObservability').style.display = tab === 'observability' ? 'block' : 'none';
            document.getElementById('tabDevices').classList.toggle('active', tab === 'devices');
            document.getElementById('tabObservability').classList.toggle('active', tab === 'observability');
            if (tab === 'observability') {
                loadObservability();
            }
        }

        let obsInterval = null;

        function loadObservability() {
            if (obsInterval) clearInterval(obsInterval);
            loadHealthScore();
            loadLiveMetrics();
            loadProcessMetrics();
            loadTransferHistory();
            loadProtocolStats();
            loadCompressionAnalytics();
            loadStorageForecast();
            loadNetworkPath();
            loadBottleneck();
            loadBufferAnalysis();
            obsInterval = setInterval(() => {
                loadHealthScore();
                loadLiveMetrics();
                loadProcessMetrics();
            }, 3000);
        }

        async function loadHealthScore() {
            try {
                const res = await fetch('/api/health-score');
                const d = await res.json();
                document.getElementById('overallHealthScore').innerText = Math.round(d.overall || 0);
                document.getElementById('healthCpu').style.width = (d.cpu || 0) + '%';
                document.getElementById('healthCpuLabel').innerText = Math.round(d.cpu || 0);
                document.getElementById('healthNetwork').style.width = (d.network || 0) + '%';
                document.getElementById('healthNetworkLabel').innerText = Math.round(d.network || 0);
                document.getElementById('healthDisk').style.width = (d.disk || 0) + '%';
                document.getElementById('healthDiskLabel').innerText = Math.round(d.disk || 0);
                document.getElementById('healthIntegrity').style.width = (d.integrity || 0) + '%';
                document.getElementById('healthIntegrityLabel').innerText = Math.round(d.integrity || 0);
                document.getElementById('healthRecovery').style.width = (d.recovery || 0) + '%';
                document.getElementById('healthRecoveryLabel').innerText = Math.round(d.recovery || 0);
            } catch(e) {}
        }

        async function loadLiveMetrics() {
            try {
                const res = await fetch('/api/system-metrics');
                const m = await res.json();
                document.getElementById('liveCpu').innerText = m.cpu_usage + '%';
                document.getElementById('liveMemory').innerText = m.memory_mb + ' MB';
                document.getElementById('liveTx').innerText = m.network_tx_mbps + ' Mbps';
                document.getElementById('liveRx').innerText = m.network_rx_mbps + ' Mbps';
            } catch(e) {}
            try {
                const res = await fetch('/api/thermal');
                const t = await res.json();
                document.getElementById('liveCpuTemp').innerText = t.cpu_temp_c ? t.cpu_temp_c + '°C' : '--';
                document.getElementById('liveFan').innerText = t.fan_rpm ? t.fan_rpm + ' RPM' : '--';
                document.getElementById('liveBattery').innerText = t.battery_pct ? t.battery_pct + '%' : '--';
                document.getElementById('liveThermal').innerText = t.thermal_state || '--';
            } catch(e) {}
        }

        async function loadProcessMetrics() {
            try {
                const res = await fetch('/api/process-metrics');
                const p = await res.json();
                const procs = p.processes || {};
                document.getElementById('procRustCpu').innerText = (procs.rust_daemon && procs.rust_daemon.cpu != null) ? procs.rust_daemon.cpu.toFixed(1) + '%' : '--';
                document.getElementById('procRustRam').innerText = (procs.rust_daemon && procs.rust_daemon.ram_mb != null) ? procs.rust_daemon.ram_mb.toFixed(1) + ' MB' : '--';
                document.getElementById('procHashCpu').innerText = (procs.hash_thread && procs.hash_thread.cpu != null) ? procs.hash_thread.cpu.toFixed(1) + '%' : '--';
                document.getElementById('procHashRam').innerText = (procs.hash_thread && procs.hash_thread.ram_mb != null) ? procs.hash_thread.ram_mb.toFixed(1) + ' MB' : '--';
            } catch(e) {}
        }

        async function loadTransferHistory() {
            try {
                const res = await fetch('/api/transfer-history');
                const items = await res.json();
                const container = document.getElementById('transferHistoryList');
                if (!items || items.length === 0) {
                    container.innerHTML = '<div style="color:var(--muted);padding:20px;text-align:center;font-size:0.85rem;font-family:Inter,sans-serif;text-transform:none;">No transfer history yet.</div>';
                    return;
                }
                let html = '';
                for (const item of items) {
                    const sizeMB = (item.size / 1_048_576).toFixed(1);
                    const speed = item.average_speed_mbps ? item.average_speed_mbps.toFixed(1) : '?';
                    const score = item.health_score != null ? Math.round(item.health_score) : '--';
                    const statusClass = item.completed ? 'ok' : 'err';
                    const statusLabel = item.completed ? 'Done' : 'Failed';
                    html += '<div class="hist-item" onclick="showTransferDetail(\'' + item.id + '\')">';
                    html += '<div class="hist-item-header"><span class="name">' + item.filename + '</span><span class="badge ' + statusClass + '">' + statusLabel + '</span></div>';
                    html += '<div class="hist-item-details"><span>' + sizeMB + ' MB</span><span>' + speed + ' Mbps avg</span><span>Score: ' + score + '/100</span>';
                    if (item.compression_ratio) html += '<span>Ratio: ' + item.compression_ratio.toFixed(2) + '</span>';
                    if (item.reconnects > 0) html += '<span>Reconnects: ' + item.reconnects + '</span>';
                    html += '</div></div>';
                }
                container.innerHTML = html;
            } catch(e) {}
        }

        async function showTransferDetail(id) {
            document.getElementById('transferDetail').style.display = 'block';
            document.getElementById('waterfallContainer').style.display = 'block';
            try {
                const res = await fetch('/api/transfer-report?id=' + encodeURIComponent(id));
                const s = await res.json();
                const t = s.transfer_summary || {};
                const f = s.file || {};
                const xfer = s.transfer || {};
                const net = s.network || {};
                const res_ = s.resources || {};
                const result = s.result || {};
                const health = s.health || {};

                const html = `
                    <div class="report-section">
                        <h4>Transfer Summary</h4>
                        <div class="report-grid">
                            <span class="key">Transfer ID</span><span class="val">${t.transfer_id || '--'}</span>
                            <span class="key">Start Time</span><span class="val">${t.start_time || '--'}</span>
                            <span class="key">End Time</span><span class="val">${t.end_time || '--'}</span>
                            <span class="key">Duration</span><span class="val">${t.duration_secs ? t.duration_secs.toFixed(1) + 's' : '--'}</span>
                        </div>
                    </div>
                    <div class="report-section">
                        <h4>File</h4>
                        <div class="report-grid">
                            <span class="key">Name</span><span class="val">${f.name || '--'}</span>
                            <span class="key">Type</span><span class="val">${f.type || '--'}</span>
                            <span class="key">Extension</span><span class="val">${f.extension || '--'}</span>
                            <span class="key">SHA256</span><span class="val" style="font-size:0.7rem;">${f.sha256 ? f.sha256.substring(0, 16) + '...' : '--'}</span>
                            <span class="key">Original Size</span><span class="val">${f.original_size ? (f.original_size / 1_048_576).toFixed(1) + ' MB' : '--'}</span>
                            <span class="key">Compressed Size</span><span class="val">${f.compressed_size ? (f.compressed_size / 1_048_576).toFixed(1) + ' MB' : 'N/A'}</span>
                            <span class="key">Compression Ratio</span><span class="val">${f.compression_ratio != null ? f.compression_ratio.toFixed(3) : 'N/A'}</span>
                        </div>
                    </div>
                    <div class="report-section">
                        <h4>Transfer Speed</h4>
                        <div class="report-grid">
                            <span class="key">Average Speed</span><span class="val">${xfer.average_speed_mbps ? xfer.average_speed_mbps.toFixed(1) + ' Mbps' : '--'}</span>
                            <span class="key">Peak Speed</span><span class="val">${xfer.peak_speed_mbps ? xfer.peak_speed_mbps.toFixed(1) + ' Mbps' : '--'}</span>
                            <span class="key">Minimum Speed</span><span class="val">${xfer.min_speed_mbps ? xfer.min_speed_mbps.toFixed(1) + ' Mbps' : '--'}</span>
                            <span class="key">Median Speed</span><span class="val">${xfer.median_speed_mbps ? xfer.median_speed_mbps.toFixed(1) + ' Mbps' : '--'}</span>
                            <span class="key">95th Percentile</span><span class="val">${xfer.p95_speed_mbps ? xfer.p95_speed_mbps.toFixed(1) + ' Mbps' : '--'}</span>
                        </div>
                    </div>
                    <div class="report-section">
                        <h4>Network</h4>
                        <div class="report-grid">
                            <span class="key">Average RTT</span><span class="val">${net.average_rtt_ms ? net.average_rtt_ms.toFixed(1) + ' ms' : '--'}</span>
                            <span class="key">Peak RTT</span><span class="val">${net.peak_rtt_ms ? net.peak_rtt_ms.toFixed(1) + ' ms' : '--'}</span>
                            <span class="key">Packet Loss</span><span class="val">${net.packet_loss_pct ? net.packet_loss_pct.toFixed(1) + '%' : '0%'}</span>
                            <span class="key">Retransmissions</span><span class="val">${net.retransmissions || 0}</span>
                            <span class="key">Reconnects</span><span class="val">${net.reconnects || 0}</span>
                        </div>
                    </div>
                    <div class="report-section">
                        <h4>Resources</h4>
                        <div class="report-grid">
                            <span class="key">Average CPU</span><span class="val">${res_.average_cpu_pct ? res_.average_cpu_pct.toFixed(1) + '%' : '--'}</span>
                            <span class="key">Peak CPU</span><span class="val">${res_.peak_cpu_pct ? res_.peak_cpu_pct.toFixed(1) + '%' : '--'}</span>
                            <span class="key">Average RAM</span><span class="val">${res_.average_ram_mb ? res_.average_ram_mb.toFixed(1) + ' MB' : '--'}</span>
                            <span class="key">Peak RAM</span><span class="val">${res_.peak_ram_mb ? res_.peak_ram_mb.toFixed(1) + ' MB' : '--'}</span>
                            <span class="key">Disk Read</span><span class="val">${res_.disk_read_mbps ? res_.disk_read_mbps.toFixed(1) + ' MB/s' : '--'}</span>
                            <span class="key">Disk Write</span><span class="val">${res_.disk_write_mbps ? res_.disk_write_mbps.toFixed(1) + ' MB/s' : '--'}</span>
                        </div>
                    </div>
                    <div class="report-section">
                        <h4>Result</h4>
                        <div class="report-grid">
                            <span class="key">Completed</span><span class="val">${result.completed ? 'Yes' : 'No'}</span>
                            <span class="key">Verified</span><span class="val">${result.verified ? 'Yes' : 'No'}</span>
                            <span class="key">Resumed</span><span class="val">${result.resumed ? 'Yes' : 'No'}</span>
                            <span class="key">Interrupted</span><span class="val">${result.interrupted ? 'Yes' : 'No'}</span>
                            <span class="key">Error</span><span class="val">${result.error || 'None'}</span>
                        </div>
                    </div>
                    <div class="report-section">
                        <h4>Health</h4>
                        <div class="report-grid">
                            <span class="key">Health Score</span><span class="val" style="font-size:1.3rem;font-weight:700;">${health.health_score != null ? Math.round(health.health_score) + '/100' : '--'}</span>
                            <span class="key">Bottleneck</span><span class="val">${health.bottleneck || '--'}</span>
                            <span class="key">Recommendation</span><span class="val">${health.recommendation || '--'}</span>
                        </div>
                    </div>
                `;
                document.getElementById('transferReportContent').innerHTML = html;
                document.getElementById('detailTitle').innerText = 'Transfer Report: ' + (f.name || id);
                document.getElementById('transferDetail').style.display = 'block';
                document.title = 'PDOS - ' + (f.name || 'Report');

                // Load waterfall
                loadWaterfall(id);
            } catch(e) {
                document.getElementById('transferReportContent').innerHTML = '<div style="color:var(--muted);">Error loading report.</div>';
            }
        }

        async function loadWaterfall(id) {
            try {
                const res = await fetch('/api/transfer-waterfall?id=' + encodeURIComponent(id));
                const phases = await res.json();
                const container = document.getElementById('waterfallContent');
                if (!phases || phases.length === 0) {
                    container.innerHTML = '<div style="color:var(--muted);font-size:0.85rem;font-family:Inter,sans-serif;text-transform:none;">No phase data.</div>';
                    return;
                }
                const totalMs = phases.reduce((acc, p) => acc + (p.duration_ms || 0), 0) || 1;
                let html = '';
                const phaseColors = {
                    'Discovery': 'phase-discovery',
                    'Authentication': 'phase-authentication',
                    'Compression': 'phase-compression',
                    'TLS': 'phase-tls',
                    'Streaming': 'phase-streaming',
                    'Hash': 'phase-hash',
                    'Archive': 'phase-archive',
                };
                for (const p of phases) {
                    const pct = Math.max(2, ((p.duration_ms || 0) / totalMs) * 100);
                    const colorClass = phaseColors[p.name] || '';
                    html += '<div class="waterfall-bar"><span class="label">' + p.name + '</span>' +
                        '<div class="track"><div class="fill ' + colorClass + '" style="width:' + pct + '%"></div></div>' +
                        '<span class="time">' + (p.duration_ms ? p.duration_ms.toFixed(0) + 'ms' : '--') + '</span></div>';
                }
                container.innerHTML = html;
            } catch(e) {
                document.getElementById('waterfallContent').innerHTML = '<div style="color:var(--muted);">Error loading waterfall.</div>';
            }
        }

        async function loadProtocolStats() {
            try {
                const res = await fetch('/api/protocol-stats');
                const stats = await res.json();
                const grid = document.getElementById('protocolStatsGrid');
                const entries = [
                    { key: 'Discovery', val: stats.discovery_packets || 0 },
                    { key: 'Auth', val: stats.auth_requests || 0 },
                    { key: 'Transfers', val: stats.transfer_requests || 0 },
                    { key: 'Resume', val: stats.resume_requests || 0 },
                    { key: 'Cancelled', val: stats.cancelled_transfers || 0 },
                    { key: 'Completed', val: stats.completed_transfers || 0 },
                    { key: 'Failed', val: stats.failed_transfers || 0 },
                    { key: 'TLS', val: stats.tls_handshakes || 0 },
                    { key: 'Range', val: stats.range_requests || 0 },
                ];
                grid.innerHTML = entries.map(e =>
                    '<div class="proto-stat"><div class="num">' + e.val + '</div><div class="label">' + e.key + '</div></div>'
                ).join('');
            } catch(e) {}
        }

        async function loadCompressionAnalytics() {
            try {
                const res = await fetch('/api/compression-analytics');
                const items = await res.json();
                const container = document.getElementById('compressionAnalyticsList');
                if (!items || items.length === 0) {
                    container.innerHTML = '<div style="color:var(--muted);font-size:0.85rem;font-family:Inter,sans-serif;text-transform:none;">No compressed transfers yet.</div>';
                    return;
                }
                let html = '';
                for (const item of items) {
                    const origMB = (item.original_size / 1_048_576).toFixed(1);
                    const compMB = (item.compressed_size / 1_048_576).toFixed(1);
                    const savedMB = item.bandwidth_saved ? (item.bandwidth_saved / 1_048_576).toFixed(1) : '0';
                    html += '<div class="hist-item"><div class="hist-item-header"><span class="name">' + item.filename + '</span><span class="badge ok">Ratio: ' + (item.compression_ratio ? item.compression_ratio.toFixed(2) : '?') + '</span></div>' +
                        '<div class="hist-item-details"><span>Original: ' + origMB + ' MB</span><span>Compressed: ' + compMB + ' MB</span>' +
                        '<span>Saved: ' + savedMB + ' MB</span><span>Time: ' + (item.compression_time_ms || '?') + ' ms</span></div></div>';
                }
                container.innerHTML = html;
            } catch(e) {}
        }

        async function loadStorageForecast() {
            try {
                const res = await fetch('/api/storage-forecast?path=' + encodeURIComponent('/tmp') + '&size=0');
                const s = await res.json();
                const html = '<div class="obs-grid" style="grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));">' +
                    '<div class="metric-card"><div class="metric-label">Total</div><div class="metric-value">' + (s.total_gb || 0).toFixed(1) + ' GB</div></div>' +
                    '<div class="metric-card"><div class="metric-label">Free</div><div class="metric-value">' + (s.free_gb || 0).toFixed(1) + ' GB</div></div>' +
                    '<div class="metric-card"><div class="metric-label">File Size</div><div class="metric-value">' + (s.file_size_gb || 0).toFixed(2) + ' GB</div></div>' +
                    '<div class="metric-card"><div class="metric-label">Remaining</div><div class="metric-value">' + (s.remaining_gb || 0).toFixed(1) + ' GB</div></div>' +
                    '<div class="metric-card"><div class="metric-label">Status</div><div class="metric-value" style="font-size:1rem;color:' + (s.enough_space ? '#4CAF50' : '#f44336') + ';">' + (s.enough_space ? 'Enough Space' : 'Low Space') + '</div></div>' +
                    '</div>';
                document.getElementById('storageForecastContent').innerHTML = html;
            } catch(e) {}
        }

        async function loadNetworkPath() {
            try {
                const res = await fetch('/api/network-path');
                const items = await res.json();
                const container = document.getElementById('networkPathContent');
                if (!items || items.length === 0) {
                    container.innerHTML = '<div style="color:var(--muted);font-size:0.85rem;font-family:Inter,sans-serif;text-transform:none;">No network data yet.</div>';
                    return;
                }
                let html = '<div style="font-family:Inter,sans-serif;text-transform:none;">';
                for (const n of items.slice(-20)) {
                    const eventClass = n.event === 'Disconnected' ? 'err' : n.event === 'stable' ? 'ok' : 'warn';
                    html += '<div class="hist-item"><div class="hist-item-header"><span class="name">' + (n.interface || '?') + '</span><span class="badge ' + eventClass + '">' + (n.event || 'stable') + '</span></div>' +
                        '<div class="hist-item-details"><span>' + (n.ip || '--') + '</span><span>' + (n.rssi || '--') + ' dBm</span><span>' + (n.link_speed || '--') + ' Mbps</span><span style="color:#666;">' + (n.time || '') + '</span></div></div>';
                }
                html += '</div>';
                container.innerHTML = html;
            } catch(e) {}
        }

        async function loadBottleneck() {
            try {
                const res = await fetch('/api/bottleneck');
                const b = await res.json();
                const html = '<div class="obs-grid">' +
                    '<div class="obs-card"><div style="font-size:2rem;font-weight:700;text-align:center;">' + (b.bottleneck || 'Idle') + '</div><div style="text-align:center;color:var(--muted);font-size:0.8rem;margin-top:4px;">Current Bottleneck</div></div>' +
                    '<div class="obs-card"><div style="font-size:1rem;font-family:Inter,sans-serif;text-transform:none;color:var(--muted);">' + (b.recommendation || 'No transfers yet.') + '</div><div style="margin-top:8px;color:var(--muted);font-size:0.8rem;">Health Score: ' + (b.health_score != null ? Math.round(b.health_score) + '/100' : '--') + '</div></div>' +
                    '</div>';
                document.getElementById('bottleneckContent').innerHTML = html;
            } catch(e) {}
        }

        async function loadBufferAnalysis() {
            try {
                const res = await fetch('/api/buffer-analysis');
                const b = await res.json();
                const html = '<div class="obs-grid" style="grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));">' +
                    '<div class="metric-card"><div class="metric-label">Read Buffer</div><div class="metric-value">' + (b.read_buffer_kb || 64) + ' KB</div></div>' +
                    '<div class="metric-card"><div class="metric-label">Write Buffer</div><div class="metric-value">' + (b.write_buffer_kb || 64) + ' KB</div></div>' +
                    '<div class="metric-card"><div class="metric-label">Avg Queue</div><div class="metric-value">' + (b.average_queue_depth || 0).toFixed(1) + '</div></div>' +
                    '<div class="metric-card"><div class="metric-label">Max Queue</div><div class="metric-value">' + (b.max_queue_depth || 0) + '</div></div>' +
                    '<div class="metric-card"><div class="metric-label">Backpressure</div><div class="metric-value">' + (b.backpressure_events || 0) + '</div></div>' +
                    '</div>';
                document.getElementById('bufferAnalysisContent').innerHTML = html;
            } catch(e) {}
        }

        // Initial scan with log
        window.onload = () => {
            addLog('Control Hub loaded. Scanning for devices...');
            setTimeout(scanDevices, 500);
        };
    </script>
</body>
</html>
"#
        .to_string()
}
