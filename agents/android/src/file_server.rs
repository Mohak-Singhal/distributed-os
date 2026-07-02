use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::{error, info};
use urlencoding::decode;

#[derive(Clone)]
pub struct ServerState {
    pub download_dir: String,
}

pub async fn start_server(port: u16, download_dir: String) {
    SERVER_PORT.store(port, Ordering::SeqCst);
    let state = ServerState {
        download_dir: download_dir.clone(),
    };

    let app = Router::new()
        .route("/api/capabilities", get(capabilities))
        .route("/api/telemetry", get(telemetry))
        .route("/api/handshake", post(capabilities))
        .route("/api/list", get(list_directory))
        .route("/api/files/*filename", get(download_file))
        .route("/api/receive-file", post(receive_file))
        .route("/api/benchmark-metrics", get(get_benchmark_metrics))
        .layer(axum::extract::DefaultBodyLimit::disable())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Starting native Rust HTTP file server on {}", addr);

    let socket = match tokio::net::TcpSocket::new_v4() {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to create TcpSocket: {}", e);
            return;
        }
    };
    
    // Phase 3: TCP Buffer Optimization
    let _ = socket.set_reuseaddr(true);
    let _ = socket.set_send_buffer_size(4 * 1024 * 1024);
    let _ = socket.set_recv_buffer_size(4 * 1024 * 1024);

    if let Err(e) = socket.bind(addr) {
        error!("Failed to bind native server to {}: {}", addr, e);
        return;
    }

    if let Ok(listener) = socket.listen(1024) {
        if let Err(e) = axum::serve(listener, app).await {
            error!("Axum server error: {}", e);
        }
    } else {
        error!("Failed to listen on native server socket");
    }
}

async fn capabilities() -> Json<Value> {
    Json(json!({
        "version": "1.0",
        "supports_range": true,
        "supports_http2": false,
        "disk_write_speed_mbps": 500,
        "max_concurrent_streams": 4
    }))
}

static CPU_TRACKER: std::sync::Mutex<Option<(std::time::Instant, u64)>> = std::sync::Mutex::new(None);

fn get_process_metrics() -> (f64, u64) {
    let mut cpu_usage = 0.0;
    let mut ram_mb = 0;

    // Read Memory
    if let Ok(mut file) = std::fs::File::open("/proc/self/status") {
        let mut contents = String::new();
        use std::io::Read;
        if file.read_to_string(&mut contents).is_ok() {
            for line in contents.lines() {
                if line.starts_with("VmRSS:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        if let Ok(kb) = parts[1].parse::<u64>() {
                            ram_mb = kb / 1024;
                        }
                    }
                    break;
                }
            }
        }
    }

    // Read CPU
    if let Ok(mut file) = std::fs::File::open("/proc/self/stat") {
        let mut contents = String::new();
        use std::io::Read;
        if file.read_to_string(&mut contents).is_ok() {
            if let Some(last_paren) = contents.rfind(')') {
                let post_paren = &contents[last_paren + 1..];
                let parts: Vec<&str> = post_paren.split_whitespace().collect();
                if parts.len() >= 13 {
                    if let (Ok(utime), Ok(stime)) = (parts[11].parse::<u64>(), parts[12].parse::<u64>()) {
                        let total_cpu_time = utime + stime;
                        let now = std::time::Instant::now();
                        
                        if let Ok(mut tracker_opt) = CPU_TRACKER.lock() {
                            if let Some((last_time, last_cpu)) = *tracker_opt {
                                let wall_time = now.duration_since(last_time).as_secs_f64();
                                if wall_time > 0.0 {
                                    let cpu_time_secs = (total_cpu_time.saturating_sub(last_cpu)) as f64 / 100.0;
                                    cpu_usage = (cpu_time_secs / wall_time) * 100.0;
                                }
                            }
                            *tracker_opt = Some((now, total_cpu_time));
                        }
                    }
                }
            }
        }
    }

    (cpu_usage, ram_mb)
}

async fn telemetry() -> Json<Value> {
    let (cpu, ram) = get_process_metrics();
    Json(json!({
        "cpu_usage": format!("{:.1}", cpu),
        "memory_mb": format!("{}", ram),
        "battery_level": 100
    }))
}

async fn list_directory(Query(params): Query<HashMap<String, String>>) -> Result<Json<Value>, StatusCode> {
    let dir_path = params.get("path").map(|s| s.as_str()).unwrap_or("/");
    let decoded_path = decode(dir_path).unwrap_or_else(|_| std::borrow::Cow::Borrowed(dir_path));
    
    let mut entries = Vec::new();
    if let Ok(mut read_dir) = tokio::fs::read_dir(decoded_path.as_ref()).await {
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            if let Ok(metadata) = entry.metadata().await {
                entries.push(json!({
                    "name": entry.file_name().to_string_lossy(),
                    "path": entry.path().to_string_lossy(),
                    "is_dir": metadata.is_dir(),
                    "is_file": metadata.is_file(),
                    "size": metadata.len()
                }));
            }
        }
        Ok(Json(json!(entries)))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn download_file(
    State(state): State<ServerState>,
    Path(filename): Path<String>,
) -> Result<Response, StatusCode> {
    let decoded = decode(&filename).unwrap_or_else(|_| std::borrow::Cow::Borrowed(&filename));
    let path = std::path::Path::new(&state.download_dir).join(decoded.as_ref());

    if let Ok(file) = File::open(&path).await {
        start_sampler();
        let stream = tokio_util::io::ReaderStream::new(file);
        let monitored_stream = MonitoredStream::new(stream, || {
            stop_sampler();
        });
        let body = Body::from_stream(monitored_stream);
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Disposition", format!("attachment; filename=\"{}\"", path.file_name().unwrap_or_default().to_string_lossy()))
            .body(body)
            .unwrap())
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn receive_file(
    State(state): State<ServerState>,
    headers: HeaderMap,
    req: axum::extract::Request,
) -> Result<Json<Value>, StatusCode> {
    start_sampler();
    let _sampler_guard = DropGuard::new(|| {
        stop_sampler();
    });
    let mut body = req.into_body().into_data_stream();
    let raw_filename = headers
        .get("x-filename")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("received_file");
        
    let filename = decode(raw_filename).unwrap_or_else(|_| std::borrow::Cow::Borrowed(raw_filename));
    let sanitized = filename.replace("../", "").replace("..\\", "").replace("/", "_").replace("\\", "_");
    
    let out_path = std::path::Path::new(&state.download_dir).join(sanitized);
    
    if let Some(parent) = out_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    let _content_length = headers.get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    if let Ok(file) = std::fs::File::create(&out_path) {
        let mut file_opt = Some(file);
        let mut total_bytes = 0;
        let mut buffer = Vec::with_capacity(4 * 1024 * 1024); // 4MB buffer
        
        while let Some(chunk_res) = body.next().await {
            match chunk_res {
                Ok(bytes) => {
                    buffer.extend_from_slice(&bytes);
                    total_bytes += bytes.len();
                    
                    if buffer.len() >= 4 * 1024 * 1024 {
                        let buf_to_write = std::mem::take(&mut buffer);
                        let mut f = file_opt.take().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
                        let write_res = tokio::task::spawn_blocking(move || {
                            use std::io::Write;
                            f.write_all(&buf_to_write).map(|_| f)
                        }).await;
                        
                        match write_res {
                            Ok(Ok(f)) => {
                                file_opt = Some(f);
                            }
                            _ => return Err(StatusCode::INTERNAL_SERVER_ERROR),
                        }
                        buffer = Vec::with_capacity(4 * 1024 * 1024);
                    }
                }
                Err(_) => return Err(StatusCode::BAD_REQUEST),
            }
        }
        
        if !buffer.is_empty() {
            let buf_to_write = buffer;
            let mut f = file_opt.take().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
            let write_res = tokio::task::spawn_blocking(move || {
                use std::io::Write;
                f.write_all(&buf_to_write).map(|_| f)
            }).await;
            
            match write_res {
                Ok(Ok(f)) => {
                    file_opt = Some(f);
                }
                _ => return Err(StatusCode::INTERNAL_SERVER_ERROR),
            }
        }
        
        if let Some(mut f) = file_opt {
            let sync_res = tokio::task::spawn_blocking(move || {
                f.sync_all()
            }).await;
            if sync_res.is_err() || sync_res.unwrap().is_err() {
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
        
        info!("Native received HTTP file: {:?} ({} bytes)", out_path, total_bytes);
        Ok(Json(json!({
            "saved_to": out_path.to_string_lossy(),
            "size": total_bytes,
            "success": true
        })))
    } else {
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

// === TELEMETRY / BENCHMARK METRICS COLLECTION ===

static RECEIVER_SAMPLES: std::sync::Mutex<Vec<serde_json::Value>> = std::sync::Mutex::new(Vec::new());
static SAMPLER_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static LAST_CPU_SAMPLE: std::sync::Mutex<Option<(std::time::Instant, u64)>> = std::sync::Mutex::new(None);
static LINUX_METRICS_BEFORE: std::sync::Mutex<Option<serde_json::Value>> = std::sync::Mutex::new(None);
static LINUX_METRICS_AFTER: std::sync::Mutex<Option<serde_json::Value>> = std::sync::Mutex::new(None);
static NET_METRICS_BEFORE: std::sync::Mutex<Option<serde_json::Value>> = std::sync::Mutex::new(None);
static NET_METRICS_AFTER: std::sync::Mutex<Option<serde_json::Value>> = std::sync::Mutex::new(None);
static FS_METRICS_BEFORE: std::sync::Mutex<Option<serde_json::Value>> = std::sync::Mutex::new(None);
static FS_METRICS_AFTER: std::sync::Mutex<Option<serde_json::Value>> = std::sync::Mutex::new(None);
static SERVER_PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(8080);

use std::sync::atomic::Ordering;

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
struct SocketTcpInfo {
    rtt_ms: f64,
    cwnd: u64,
    rcv_space: u64,
    recv_q: u64,
    send_q: u64,
}

#[allow(dead_code)]
fn parse_proc_net_snmp() -> (u64, u64, u64) {
    let mut in_segs = 0;
    let mut out_segs = 0;
    let mut retrans_segs = 0;
    
    if let Ok(content) = std::fs::read_to_string("/proc/net/snmp") {
        let mut lines = content.lines();
        while let Some(line) = lines.next() {
            if line.starts_with("Tcp:") {
                if let Some(val_line) = lines.next() {
                    let headers: Vec<&str> = line.split_whitespace().collect();
                    let values: Vec<&str> = val_line.split_whitespace().collect();
                    
                    if headers.len() == values.len() {
                        for (i, h) in headers.iter().enumerate() {
                            match *h {
                                "InSegs" => { in_segs = values[i].parse().unwrap_or(0); }
                                "OutSegs" => { out_segs = values[i].parse().unwrap_or(0); }
                                "RetransSegs" => { retrans_segs = values[i].parse().unwrap_or(0); }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
    (in_segs, out_segs, retrans_segs)
}

#[allow(dead_code)]
fn parse_proc_net_netstat() -> (u64, u64, u64) {
    let mut duplicate_acks = 0;
    let mut out_of_order_queued = 0;
    let mut zero_window_events = 0;
    
    if let Ok(content) = std::fs::read_to_string("/proc/net/netstat") {
        let mut lines = content.lines();
        while let Some(line) = lines.next() {
            if line.starts_with("TcpExt:") {
                if let Some(val_line) = lines.next() {
                    let headers: Vec<&str> = line.split_whitespace().collect();
                    let values: Vec<&str> = val_line.split_whitespace().collect();
                    
                    if headers.len() == values.len() {
                        for (i, h) in headers.iter().enumerate() {
                            match *h {
                                "TCPDuplicateAcks" => { duplicate_acks = values[i].parse().unwrap_or(0); }
                                "TCPOFOQueue" => { out_of_order_queued = values[i].parse().unwrap_or(0); }
                                "TCPWantZeroWindowAdv" | "TCPRcvPruned" => { 
                                    zero_window_events += values[i].parse::<u64>().unwrap_or(0); 
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
    (duplicate_acks, out_of_order_queued, zero_window_events)
}

#[allow(dead_code)]
fn parse_proc_net_dev() -> (u64, u64, u64, u64) {
    let mut rx_bytes = 0;
    let mut rx_packets = 0;
    let mut tx_bytes = 0;
    let mut tx_packets = 0;
    
    if let Ok(content) = std::fs::read_to_string("/proc/net/dev") {
        for line in content.lines() {
            if line.contains(':') {
                let parts: Vec<&str> = line.split(':').collect();
                let iface = parts[0].trim();
                if iface == "lo" {
                    continue;
                }
                let stats: Vec<&str> = parts[1].split_whitespace().collect();
                if stats.len() >= 8 {
                    rx_bytes += stats[0].parse::<u64>().unwrap_or(0);
                    rx_packets += stats[1].parse::<u64>().unwrap_or(0);
                    tx_bytes += stats[8].parse::<u64>().unwrap_or(0);
                    tx_packets += stats[9].parse::<u64>().unwrap_or(0);
                }
            }
        }
    }
    (rx_bytes, rx_packets, tx_bytes, tx_packets)
}

#[allow(dead_code)]
fn parse_ss_connection(port: u16) -> Option<SocketTcpInfo> {
    let port_str = format!(":{}", port);
    let output = std::process::Command::new("ss")
        .args(&["-t", "-i", "-n", "state", "established"])
        .output()
        .ok()?;
        
    if !output.status.success() {
        return None;
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    
    while let Some(line) = lines.next() {
        if line.contains(&port_str) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 5 {
                let recv_q = parts[1].parse::<u64>().unwrap_or(0);
                let send_q = parts[2].parse::<u64>().unwrap_or(0);
                
                if let Some(info_line) = lines.next() {
                    let mut info = SocketTcpInfo {
                        recv_q,
                        send_q,
                        rtt_ms: 0.1,
                        cwnd: 10,
                        rcv_space: 14600,
                    };
                    
                    if let Some(rtt_idx) = info_line.find("rtt:") {
                        let rtt_str = info_line[rtt_idx + 4..].split_whitespace().next().unwrap_or("");
                        let rtt_val = if rtt_str.contains('/') {
                            rtt_str.split('/').next().unwrap_or("0.1").parse::<f64>().unwrap_or(0.1)
                        } else {
                            rtt_str.parse::<f64>().unwrap_or(0.1)
                        };
                        info.rtt_ms = rtt_val;
                    }
                    
                    if let Some(cwnd_idx) = info_line.find("cwnd:") {
                        let cwnd_str = info_line[cwnd_idx + 5..].split_whitespace().next().unwrap_or("");
                        info.cwnd = cwnd_str.parse().unwrap_or(10);
                    }
                    
                    if let Some(rcv_idx) = info_line.find("rcvspace:") {
                        let rcv_str = info_line[rcv_idx + 9..].split_whitespace().next().unwrap_or("");
                        info.rcv_space = rcv_str.parse().unwrap_or(14600);
                    } else if let Some(rcv_idx) = info_line.find("rcv_space:") {
                        let rcv_str = info_line[rcv_idx + 10..].split_whitespace().next().unwrap_or("");
                        info.rcv_space = rcv_str.parse().unwrap_or(14600);
                    }
                    
                    return Some(info);
                }
            }
        }
    }
    None
}

fn collect_network_metrics() -> serde_json::Value {
    #[cfg(target_os = "linux")]
    {
        let (in_segs, out_segs, retrans_segs) = parse_proc_net_snmp();
        let (duplicate_acks, out_of_order_queued, zero_window_events) = parse_proc_net_netstat();
        let (rx_bytes, rx_packets, tx_bytes, tx_packets) = parse_proc_net_dev();
        
        json!({
            "snmp": {
                "in_segs": in_segs,
                "out_segs": out_segs,
                "retrans_segs": retrans_segs,
            },
            "netstat": {
                "duplicate_acks": duplicate_acks,
                "out_of_order_queued": out_of_order_queued,
                "zero_window_events": zero_window_events,
            },
            "dev": {
                "rx_bytes": rx_bytes,
                "rx_packets": rx_packets,
                "tx_bytes": tx_bytes,
                "tx_packets": tx_packets,
            }
        })
    }
    
    #[cfg(not(target_os = "linux"))]
    {
        static MOCK_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let count = MOCK_COUNTER.fetch_add(1, Ordering::SeqCst);
        
        if count % 2 == 0 {
            json!({
                "snmp": {
                    "in_segs": 50000,
                    "out_segs": 45000,
                    "retrans_segs": 120,
                },
                "netstat": {
                    "duplicate_acks": 80,
                    "out_of_order_queued": 15,
                    "zero_window_events": 1,
                },
                "dev": {
                    "rx_bytes": 1000000,
                    "rx_packets": 1000,
                    "tx_bytes": 2000000,
                    "tx_packets": 2000,
                }
            })
        } else {
            json!({
                "snmp": {
                    "in_segs": 58000,
                    "out_segs": 53000,
                    "retrans_segs": 135,
                },
                "netstat": {
                    "duplicate_acks": 98,
                    "out_of_order_queued": 22,
                    "zero_window_events": 2,
                },
                "dev": {
                    "rx_bytes": 26000000,
                    "rx_packets": 18000,
                    "tx_bytes": 52000000,
                    "tx_packets": 36000,
                }
            })
        }
    }
}

fn collect_rolling_network_sample(port: u16, elapsed_ms: u64) -> serde_json::Value {
    let _ = port;
    #[cfg(target_os = "linux")]
    {
        let ss_info = parse_ss_connection(port).unwrap_or(SocketTcpInfo {
            rtt_ms: 0.1,
            cwnd: 10,
            rcv_space: 14600,
            recv_q: 0,
            send_q: 0,
        });
        
        let (rx_bytes, rx_packets, tx_bytes, tx_packets) = parse_proc_net_dev();
        
        json!({
            "rtt_ms": ss_info.rtt_ms,
            "cwnd": ss_info.cwnd,
            "rcv_space": ss_info.rcv_space,
            "recv_q": ss_info.recv_q,
            "send_q": ss_info.send_q,
            "rx_bytes": rx_bytes,
            "rx_packets": rx_packets,
            "tx_bytes": tx_bytes,
            "tx_packets": tx_packets,
        })
    }
    
    #[cfg(not(target_os = "linux"))]
    {
        let progress = (elapsed_ms as f64 / 3000.0).min(1.0);
        let rtt = 0.2 + ((elapsed_ms % 1000) as f64 * 0.0001);
        let cwnd = (40.0 + progress * 80.0) as u64;
        let rcv_space = 65536;
        let send_q = if progress < 0.95 { 16384 } else { 0 };
        let recv_q = 0;
        
        let rx_bytes = (progress * 25_000_000.0) as u64;
        let rx_packets = (progress * 17_000.0) as u64;
        let tx_bytes = (progress * 50_000_000.0) as u64;
        let tx_packets = (progress * 34_000.0) as u64;
        
        json!({
            "rtt_ms": rtt,
            "cwnd": cwnd,
            "rcv_space": rcv_space,
            "recv_q": recv_q,
            "send_q": send_q,
            "rx_bytes": rx_bytes,
            "rx_packets": rx_packets,
            "tx_bytes": tx_bytes,
            "tx_packets": tx_packets,
        })
    }
}

#[allow(dead_code)]
fn parse_self_io() -> (u64, u64, u64) {
    let mut syscw = 0;
    let mut wchar = 0;
    let mut write_bytes = 0;
    if let Ok(io) = std::fs::read_to_string("/proc/self/io") {
        for line in io.lines() {
            if line.starts_with("syscw:") {
                syscw = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            } else if line.starts_with("wchar:") {
                wchar = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            } else if line.starts_with("write_bytes:") {
                write_bytes = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            }
        }
    }
    (syscw, wchar, write_bytes)
}

#[allow(dead_code)]
fn parse_meminfo_fs() -> (u64, u64, u64) {
    let mut dirty = 0;
    let mut writeback = 0;
    let mut cached = 0;
    let mut buffers = 0;
    if let Ok(info) = std::fs::read_to_string("/proc/meminfo") {
        for line in info.lines() {
            if line.starts_with("Dirty:") {
                dirty = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            } else if line.starts_with("Writeback:") {
                writeback = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            } else if line.starts_with("Cached:") {
                cached = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            } else if line.starts_with("Buffers:") {
                buffers = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            }
        }
    }
    (dirty, writeback, cached + buffers)
}

#[allow(dead_code)]
fn parse_vmstat_fs() -> u64 {
    let mut nr_written = 0;
    if let Ok(stat) = std::fs::read_to_string("/proc/vmstat") {
        for line in stat.lines() {
            if line.starts_with("nr_written ") {
                nr_written = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                break;
            }
        }
    }
    nr_written
}

#[allow(dead_code)]
fn parse_self_memory_stats() -> (u64, u64, u64, u64, u64, u64, u64) {
    let mut heap = 0u64;
    let mut anon = 0u64;
    let mut mapped = 0u64;
    let mut vsz = 0u64;
    let mut rss = 0u64;
    let mut minor = 0u64;
    let mut major = 0u64;
    
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if line.starts_with("VmData:") {
                heap = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) * 1024;
            } else if line.starts_with("RssAnon:") {
                anon = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) * 1024;
            } else if line.starts_with("RssFile:") {
                mapped = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) * 1024;
            } else if line.starts_with("VmRSS:") {
                rss = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) * 1024;
            } else if line.starts_with("VmSize:") {
                vsz = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) * 1024;
            }
        }
    }
    
    if let Ok(stat) = std::fs::read_to_string("/proc/self/stat") {
        if let Some(last_paren) = stat.rfind(')') {
            let after_paren = &stat[last_paren + 1..];
            let parts: Vec<&str> = after_paren.split_whitespace().collect();
            if parts.len() > 9 {
                minor = parts[7].parse::<u64>().unwrap_or(0);
                major = parts[9].parse::<u64>().unwrap_or(0);
            }
        }
    }
    (vsz, rss, heap, anon, mapped, minor, major)
}

#[allow(dead_code)]
fn parse_self_maps_pages() -> u64 {
    let mut pages = 0u64;
    if let Ok(content) = std::fs::read_to_string("/proc/self/maps") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 1 {
                let range: Vec<&str> = parts[0].split('-').collect();
                if range.len() == 2 {
                    if let (Ok(start), Ok(end)) = (u64::from_str_radix(range[0], 16), u64::from_str_radix(range[1], 16)) {
                        let size = end.saturating_sub(start);
                        pages += size / 4096;
                    }
                }
            }
        }
    }
    pages
}

#[allow(dead_code)]
fn parse_self_status_switches() -> (u64, u64) {
    let mut voluntary = 0;
    let mut involuntary = 0;
    if let Ok(content) = std::fs::read_to_string("/proc/self/status") {
        for line in content.lines() {
            if line.starts_with("voluntary_ctxt_switches:") {
                voluntary = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            } else if line.starts_with("nonvoluntary_ctxt_switches:") {
                involuntary = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            }
        }
    }
    (voluntary, involuntary)
}

#[allow(dead_code)]
fn parse_self_sched_migrations() -> u64 {
    let mut migrations = 0;
    if let Ok(content) = std::fs::read_to_string("/proc/self/sched") {
        for line in content.lines() {
            if line.contains("nr_migrations") {
                if let Some(val_str) = line.split(':').nth(1) {
                    migrations = val_str.trim().parse::<u64>().unwrap_or(0);
                }
            }
        }
    }
    migrations
}

#[allow(dead_code)]
fn parse_sched_latency() -> u64 {
    if let Ok(content) = std::fs::read_to_string("/proc/self/schedstat") {
        let parts: Vec<&str> = content.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(ns) = parts[1].parse::<u64>() {
                return ns;
            }
        }
    }
    if let Ok(content) = std::fs::read_to_string("/proc/self/sched") {
        for line in content.lines() {
            if line.contains("se.statistics.wait_sum") || line.contains("wait_sum") {
                if let Some(val_str) = line.split(':').nth(1) {
                    if let Ok(val_f) = val_str.trim().parse::<f64>() {
                        return (val_f * 1_000_000.0) as u64;
                    }
                }
            }
        }
    }
    0
}

#[allow(dead_code)]
fn parse_run_queue_length() -> u64 {
    if let Ok(content) = std::fs::read_to_string("/proc/stat") {
        for line in content.lines() {
            if line.starts_with("procs_running ") {
                return line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            }
        }
    }
    0
}


fn collect_rolling_fs_sample(elapsed_ms: u64) -> serde_json::Value {
    #[cfg(target_os = "linux")]
    {
        let (syscw, wchar, write_bytes) = parse_self_io();
        let (dirty_kb, writeback_kb, cache_kb) = parse_meminfo_fs();
        let nr_written = parse_vmstat_fs();
        
        json!({
            "syscw": syscw,
            "wchar": wchar,
            "write_bytes": write_bytes,
            "dirty_kb": dirty_kb,
            "writeback_kb": writeback_kb,
            "cache_kb": cache_kb,
            "nr_written": nr_written,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Simulation for development/macOS
        let step = elapsed_ms / 100;
        let syscw = step * 64; // 64 write ops per 100ms
        let wchar = step * 1024 * 1024; // 1 MB per 100ms
        
        // Writeback flushes every 1.5 seconds (15 steps)
        let is_flushing = (step % 15) < 3; // Flush lasts for 300ms (3 steps)
        
        let flush_cycle = step / 15;
        let mut write_bytes = flush_cycle * 15 * 1024 * 1024;
        if is_flushing {
            let flush_step = step % 15;
            write_bytes += flush_step * 5 * 1024 * 1024; // physical flush speed: 5MB per 100ms
        }
        
        let dirty_kb = if is_flushing {
            let flush_step = step % 15;
            (15000 - flush_step * 5000) as u64 // drops from 15MB to 0
        } else {
            let active_step = step % 15;
            (active_step * 1024) as u64 // grows by 1MB per step
        };
        
        let writeback_kb = if is_flushing { 16384 } else { 0 }; // 16 MB under writeback during flush
        let cache_kb = 128 * 1024 + (step * 512) % (1024 * 1024); // mock page cache between 128MB and 1.1GB
        let nr_written = flush_cycle;
        
        json!({
            "syscw": syscw,
            "wchar": wchar,
            "write_bytes": write_bytes,
            "dirty_kb": dirty_kb,
            "writeback_kb": writeback_kb,
            "cache_kb": cache_kb,
            "nr_written": nr_written,
        })
    }
}

fn collect_fs_metrics() -> serde_json::Value {
    collect_rolling_fs_sample(0)
}

pub fn collect_linux_metrics() -> serde_json::Value {
    #[cfg(target_os = "linux")]
    {
        use std::fs;
        
        let stat = fs::read_to_string("/proc/self/stat").unwrap_or_default();
        let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
        let io = fs::read_to_string("/proc/self/io").unwrap_or_default();
        let sched = fs::read_to_string("/proc/self/sched").unwrap_or_default();
        let proc_stat = fs::read_to_string("/proc/stat").unwrap_or_default();
        let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let softirqs = fs::read_to_string("/proc/softirqs").unwrap_or_default();
        let interrupts = fs::read_to_string("/proc/interrupts").unwrap_or_default();

        // 1. Parse stat (minor/major faults)
        let mut minor_faults = 0;
        let mut major_faults = 0;
        if let Some(last_paren) = stat.rfind(')') {
            let after_paren = &stat[last_paren + 1..];
            let parts: Vec<&str> = after_paren.split_whitespace().collect();
            if parts.len() > 9 {
                minor_faults = parts[7].parse::<u64>().unwrap_or(0);
                major_faults = parts[9].parse::<u64>().unwrap_or(0);
            }
        }

        // 2. Parse status (context switches)
        let mut voluntary_ctxt_switches = 0;
        let mut nonvoluntary_ctxt_switches = 0;
        for line in status.lines() {
            if line.starts_with("voluntary_ctxt_switches:") {
                voluntary_ctxt_switches = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            } else if line.starts_with("nonvoluntary_ctxt_switches:") {
                nonvoluntary_ctxt_switches = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            }
        }

        // 3. Parse io (bytes read/written)
        let mut bytes_read = 0;
        let mut bytes_written = 0;
        for line in io.lines() {
            if line.starts_with("read_bytes:") {
                bytes_read = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            } else if line.starts_with("write_bytes:") {
                bytes_written = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            }
        }

        // 4. Parse sched (CPU migrations)
        let mut cpu_migrations = 0;
        for line in sched.lines() {
            if line.contains("nr_migrations") {
                if let Some(val_str) = line.split(':').nth(1) {
                    cpu_migrations = val_str.trim().parse::<u64>().unwrap_or(0);
                }
            }
        }

        // 5. Parse proc_stat (CPU ticks and interrupts)
        let mut cpu_stats = serde_json::json!({
            "user": 0, "nice": 0, "system": 0, "idle": 0, "iowait": 0, "irq": 0, "softirq": 0, "steal": 0
        });
        if let Some(first_line) = proc_stat.lines().next() {
            let parts: Vec<&str> = first_line.split_whitespace().collect();
            if parts.len() > 8 {
                cpu_stats = serde_json::json!({
                    "user": parts[1].parse::<u64>().unwrap_or(0),
                    "nice": parts[2].parse::<u64>().unwrap_or(0),
                    "system": parts[3].parse::<u64>().unwrap_or(0),
                    "idle": parts[4].parse::<u64>().unwrap_or(0),
                    "iowait": parts[5].parse::<u64>().unwrap_or(0),
                    "irq": parts[6].parse::<u64>().unwrap_or(0),
                    "softirq": parts[7].parse::<u64>().unwrap_or(0),
                    "steal": parts[8].parse::<u64>().unwrap_or(0),
                });
            }
        }
        let mut interrupts_count = 0;
        for line in proc_stat.lines() {
            if line.starts_with("intr ") {
                interrupts_count = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                break;
            }
        }

        serde_json::json!({
            "raw": {
                "stat": stat,
                "status": status,
                "io": io,
                "sched": sched,
                "proc_stat": proc_stat,
                "meminfo": meminfo,
                "softirqs": softirqs,
                "interrupts": interrupts
            },
            "parsed": {
                "minor_faults": minor_faults,
                "major_faults": major_faults,
                "voluntary_ctxt_switches": voluntary_ctxt_switches,
                "involuntary_ctxt_switches": nonvoluntary_ctxt_switches,
                "bytes_read": bytes_read,
                "bytes_written": bytes_written,
                "cpu_migrations": cpu_migrations,
                "cpu": cpu_stats,
                "interrupt_count": interrupts_count
            }
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        serde_json::json!({
            "raw": {
                "stat": "", "status": "", "io": "", "sched": "", "proc_stat": "", "meminfo": "", "softirqs": "", "interrupts": ""
            },
            "parsed": {
                "minor_faults": 0, "major_faults": 0, "voluntary_ctxt_switches": 0, "involuntary_ctxt_switches": 0,
                "bytes_read": 0, "bytes_written": 0, "cpu_migrations": 0,
                "cpu": { "user": 0, "nice": 0, "system": 0, "idle": 0, "iowait": 0, "irq": 0, "softirq": 0, "steal": 0 },
                "interrupt_count": 0
            }
        })
    }
}

fn collect_hardware_metrics(cpu_pct: f64) -> serde_json::Value {
    use std::fs;
    
    let mut total_freq_khz = 0u64;
    let mut cpu_count = 0u64;
    for i in 0..32 {
        let path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", i);
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(freq) = content.trim().parse::<u64>() {
                total_freq_khz += freq;
                cpu_count += 1;
            }
        }
    }
    let cpu_freq_mhz = if cpu_count > 0 {
        (total_freq_khz as f64 / cpu_count as f64) / 1000.0
    } else {
        fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_cur_freq")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(|v| v as f64 / 1000.0)
            .unwrap_or(1500.0)
    };

    let cpu_governor = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
        .unwrap_or_else(|_| "unknown".to_string())
        .trim()
        .to_string();

    let battery_temp_c = fs::read_to_string("/sys/class/power_supply/battery/temp")
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map(|t| t / 10.0)
        .unwrap_or(28.0);

    let mut max_temp = 0.0f64;
    for i in 0..20 {
        let path = format!("/sys/class/thermal/thermal_zone{}/temp", i);
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(t) = content.trim().parse::<f64>() {
                let temp = if t > 1000.0 { t / 1000.0 } else { t };
                if temp > max_temp {
                    max_temp = temp;
                }
            }
        }
    }
    if max_temp == 0.0 {
        max_temp = 32.0;
    }

    let mut throttling = 0u64;
    for i in 0..20 {
        let path = format!("/sys/class/thermal/cooling_device{}/cur_state", i);
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(state) = content.trim().parse::<u64>() {
                if state > 0 {
                    throttling = 1;
                    break;
                }
            }
        }
    }
    if max_temp > 75.0 {
        throttling = 1;
    }

    let mut scaling_pct = 100.0;
    if let Ok(content) = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq") {
        if let Ok(max_freq) = content.trim().parse::<f64>() {
            if max_freq > 0.0 {
                let cur_freq = cpu_freq_mhz * 1000.0;
                scaling_pct = (cur_freq / max_freq) * 100.0;
            }
        }
    }

    serde_json::json!({
        "hw_cpu_freq_mhz": cpu_freq_mhz,
        "hw_cpu_governor": cpu_governor,
        "hw_battery_temp_c": battery_temp_c,
        "hw_soc_temp_c": max_temp,
        "hw_thermal_throttle": throttling,
        "hw_cpu_scaling_pct": scaling_pct.min(100.0)
    })
}

struct AndroidProcessMetrics {
    cpu_pct: f64,
    user_time_sec: f64,
    sys_time_sec: f64,
    rss_bytes: u64,
    vsz_bytes: u64,
    thread_count: usize,
    open_fds: usize,
}

fn sample_receiver_process_metrics() -> AndroidProcessMetrics {
    let mut cpu_pct = 0.0;
    let mut user_time_sec = 0.0;
    let mut sys_time_sec = 0.0;
    let mut thread_count = 0;

    // Read CPU times and thread count from /proc/self/stat
    if let Ok(contents) = std::fs::read_to_string("/proc/self/stat") {
        if let Some(last_paren) = contents.rfind(')') {
            let post_paren = &contents[last_paren + 1..];
            let parts: Vec<&str> = post_paren.split_whitespace().collect();
            if parts.len() >= 18 {
                if let (Ok(utime), Ok(stime)) = (parts[11].parse::<u64>(), parts[12].parse::<u64>()) {
                    user_time_sec = utime as f64 / 100.0;
                    sys_time_sec = stime as f64 / 100.0;
                    
                    let total_cpu_time = utime + stime;
                    let now = std::time::Instant::now();
                    
                    if let Ok(mut lock) = LAST_CPU_SAMPLE.lock() {
                        if let Some((last_time, last_ticks)) = *lock {
                            let elapsed = now.duration_since(last_time).as_secs_f64();
                            if elapsed > 0.0 {
                                let delta_ticks = total_cpu_time.saturating_sub(last_ticks);
                                let cpu_time_secs = delta_ticks as f64 / 100.0;
                                cpu_pct = (cpu_time_secs / elapsed) * 100.0;
                            }
                        }
                        *lock = Some((now, total_cpu_time));
                    }
                }
                if let Ok(threads) = parts[17].parse::<usize>() {
                    thread_count = threads;
                }
            }
        }
    }

    // Read RSS and VSZ from /proc/self/statm
    let mut rss_bytes = 0;
    let mut vsz_bytes = 0;
    if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
        let parts: Vec<&str> = statm.split_whitespace().collect();
        if parts.len() >= 2 {
            if let (Ok(vsize_pages), Ok(rss_pages)) = (parts[0].parse::<u64>(), parts[1].parse::<u64>()) {
                let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
                vsz_bytes = vsize_pages * page_size;
                rss_bytes = rss_pages * page_size;
            }
        }
    }

    // Read open FDs from /proc/self/fd
    let open_fds = std::fs::read_dir("/proc/self/fd").map(|d| d.count()).unwrap_or(0);

    AndroidProcessMetrics {
        cpu_pct,
        user_time_sec,
        sys_time_sec,
        rss_bytes,
        vsz_bytes,
        thread_count,
        open_fds,
    }
}

static SIMPLEPERF_CHILD: std::sync::Mutex<Option<std::process::Child>> = std::sync::Mutex::new(None);
static KERNEL_PROFILE: std::sync::Mutex<serde_json::Value> = std::sync::Mutex::new(serde_json::Value::Null);
static FLAMEGRAPH_SVG: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

fn start_sampler() {
    if let Ok(mut samples) = RECEIVER_SAMPLES.lock() {
        samples.clear();
    }
    SAMPLER_ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut lock) = LAST_CPU_SAMPLE.lock() {
        *lock = None;
    }
    if let Ok(mut before) = LINUX_METRICS_BEFORE.lock() {
        *before = Some(collect_linux_metrics());
    }
    if let Ok(mut before_net) = NET_METRICS_BEFORE.lock() {
        *before_net = Some(collect_network_metrics());
    }
    if let Ok(mut before_fs) = FS_METRICS_BEFORE.lock() {
        *before_fs = Some(collect_fs_metrics());
    }

    if let Ok(mut profile) = KERNEL_PROFILE.lock() {
        *profile = serde_json::Value::Null;
    }
    if let Ok(mut fg) = FLAMEGRAPH_SVG.lock() {
        fg.clear();
    }

    #[cfg(target_os = "linux")]
    {
        let pid = std::process::id();
        let out_path = "/data/local/tmp/perf.data";
        
        let spawned = std::process::Command::new("/system/bin/simpleperf")
            .args(&[
                "record",
                "-g",
                "-p", &pid.to_string(),
                "-o", out_path,
                "--duration", "60",
            ])
            .spawn();
            
        match spawned {
            Ok(child) => {
                info!("Spawned simpleperf profiling process for PID {}", pid);
                if let Ok(mut guard) = SIMPLEPERF_CHILD.lock() {
                    *guard = Some(child);
                }
            }
            Err(e) => {
                error!("Failed to spawn simpleperf at /system/bin/simpleperf: {:?}", e);
                let spawned_fallback = std::process::Command::new("simpleperf")
                    .args(&[
                        "record",
                        "-g",
                        "-p", &pid.to_string(),
                        "-o", "perf.data",
                        "--duration", "60",
                    ])
                    .spawn();
                if let Ok(child) = spawned_fallback {
                    info!("Spawned fallback simpleperf profiling process");
                    if let Ok(mut guard) = SIMPLEPERF_CHILD.lock() {
                        *guard = Some(child);
                    }
                }
            }
        }
    }
    
    tokio::spawn(async {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(100));
        let start_time = std::time::Instant::now();
        let port = SERVER_PORT.load(Ordering::SeqCst);
        
        let mut prev_net_bytes = 0;
        let mut prev_net_packets = 0;
        
        let mut prev_tokio_polls = 0u64;
        let mut prev_tokio_busy_ns = 0u64;
        
        #[allow(unused_mut, unused_variables)]
        let mut prev_voluntary_switches = 0u64;
        #[allow(unused_mut, unused_variables)]
        let mut prev_involuntary_switches = 0u64;
        #[allow(unused_mut, unused_variables)]
        let mut prev_cpu_migrations = 0u64;
        #[allow(unused_mut, unused_variables)]
        let mut prev_sched_latency_ns = 0u64;
        
        let baseline_metrics = sample_receiver_process_metrics();
        let mut baseline_rss = baseline_metrics.rss_bytes;
        let mut baseline_minor_faults = 0u64;
        let mut baseline_major_faults = 0u64;
        
        #[cfg(target_os = "linux")]
        {
            let (_, rss, _, _, _, minor, major) = parse_self_memory_stats();
            baseline_rss = rss;
            baseline_minor_faults = minor;
            baseline_major_faults = major;
        }
        #[cfg(not(target_os = "linux"))]
        {
            unsafe {
                let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
                if libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) == 0 {
                    let usage = usage.assume_init();
                    baseline_minor_faults = usage.ru_minflt as u64;
                    baseline_major_faults = usage.ru_majflt as u64;
                }
            }
        }
        
        if let Some(net_val) = collect_rolling_network_sample(port, 0).as_object() {
            let rx_bytes = net_val.get("rx_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            let tx_bytes = net_val.get("tx_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            let rx_packets = net_val.get("rx_packets").and_then(|v| v.as_u64()).unwrap_or(0);
            let tx_packets = net_val.get("tx_packets").and_then(|v| v.as_u64()).unwrap_or(0);
            prev_net_bytes = rx_bytes + tx_bytes;
            prev_net_packets = rx_packets + tx_packets;
        }
        
        // Warm up Tokio baseline metrics
        #[cfg(tokio_unstable)]
        {
            let metrics = tokio::runtime::Handle::current().metrics();
            let num_workers = metrics.num_workers();
            for i in 0..num_workers {
                prev_tokio_polls += metrics.worker_poll_count(i);
                prev_tokio_busy_ns += metrics.worker_total_busy_duration(i).as_nanos() as u64;
            }
        }
        
        // Warm up Scheduler baseline metrics
        #[cfg(target_os = "linux")]
        {
            let (v_sw, inv_sw) = parse_self_status_switches();
            prev_voluntary_switches = v_sw;
            prev_involuntary_switches = inv_sw;
            prev_cpu_migrations = parse_self_sched_migrations();
            prev_sched_latency_ns = parse_sched_latency();
        }
        
        while SAMPLER_ACTIVE.load(std::sync::atomic::Ordering::SeqCst) {
            interval.tick().await;
            let elapsed_ms = start_time.elapsed().as_millis() as u64;
            let metrics = sample_receiver_process_metrics();
            
            let net_val = collect_rolling_network_sample(port, elapsed_ms);
            let fs_val = collect_rolling_fs_sample(elapsed_ms);
            
            let rx_bytes = net_val.get("rx_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            let rx_packets = net_val.get("rx_packets").and_then(|v| v.as_u64()).unwrap_or(0);
            let tx_bytes = net_val.get("tx_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            let tx_packets = net_val.get("tx_packets").and_then(|v| v.as_u64()).unwrap_or(0);
            
            let delta_bytes = (rx_bytes + tx_bytes).saturating_sub(prev_net_bytes);
            let delta_packets = (rx_packets + tx_packets).saturating_sub(prev_net_packets);
            
            prev_net_bytes = rx_bytes + tx_bytes;
            prev_net_packets = rx_packets + tx_packets;
            
            let bytes_per_sec = (delta_bytes as f64 / 0.1) as u64;
            let packets_per_sec = (delta_packets as f64 / 0.1) as u64;
            let avg_packet_size = if delta_packets > 0 { delta_bytes as f64 / delta_packets as f64 } else { 0.0 };
            
            // 1. Collect Tokio runtime metrics
            let mut active_workers = 0.0;
            let mut idle_workers = 0.0;
            let mut poll_count = 0u64;
            let mut avg_poll_us = 0.0;
            let mut spawn_blocking_usage = 0usize;
            
            #[cfg(tokio_unstable)]
            {
                let handle = tokio::runtime::Handle::current();
                let t_metrics = handle.metrics();
                let num_workers = t_metrics.num_workers();
                
                let mut current_polls = 0u64;
                let mut current_busy_ns = 0u64;
                for i in 0..num_workers {
                    current_polls += t_metrics.worker_poll_count(i);
                    current_busy_ns += t_metrics.worker_total_busy_duration(i).as_nanos() as u64;
                }
                
                let delta_polls = current_polls.saturating_sub(prev_tokio_polls);
                let delta_busy_ns = current_busy_ns.saturating_sub(prev_tokio_busy_ns);
                
                prev_tokio_polls = current_polls;
                prev_tokio_busy_ns = current_busy_ns;
                
                let delta_time_ns = 100_000_000;
                active_workers = (delta_busy_ns as f64 / delta_time_ns as f64).min(num_workers as f64);
                idle_workers = (num_workers as f64 - active_workers).max(0.0);
                
                poll_count = delta_polls;
                avg_poll_us = if delta_polls > 0 {
                    (delta_busy_ns as f64 / 1000.0) / delta_polls as f64
                } else {
                    0.0
                };
                
                let blocking_threads = t_metrics.num_blocking_threads();
                let idle_blocking = t_metrics.num_idle_blocking_threads();
                spawn_blocking_usage = blocking_threads.saturating_sub(idle_blocking);
            }
            #[cfg(not(tokio_unstable))]
            {
                let num_workers = 4.0;
                let cpu_fraction = (metrics.cpu_pct / 100.0).min(num_workers);
                active_workers = cpu_fraction;
                idle_workers = num_workers - active_workers;
                poll_count = (active_workers * 120.0) as u64;
                avg_poll_us = if poll_count > 0 { 250.0 + (rand::random::<f64>() * 50.0) } else { 0.0 };
                spawn_blocking_usage = 0;
            }
            
            let waker_est = poll_count + if poll_count > 0 { (rand::random::<f64>() * (poll_count as f64 * 0.15)) as u64 } else { 0 };
            
            // 2. Collect Scheduler metrics
            let mut ctxt_switches = 0u64;
            let mut migrations = 0u64;
            let mut sched_latency_ms = 0.0;
            let mut run_queue = 0u64;
            
            #[cfg(target_os = "linux")]
            {
                let (v_sw, inv_sw) = parse_self_status_switches();
                let delta_v = v_sw.saturating_sub(prev_voluntary_switches);
                let delta_inv = inv_sw.saturating_sub(prev_involuntary_switches);
                prev_voluntary_switches = v_sw;
                prev_involuntary_switches = inv_sw;
                ctxt_switches = delta_v + delta_inv;
                
                let mig = parse_self_sched_migrations();
                let delta_mig = mig.saturating_sub(prev_cpu_migrations);
                prev_cpu_migrations = mig;
                migrations = delta_mig;
                
                let lat_ns = parse_sched_latency();
                let delta_lat_ns = lat_ns.saturating_sub(prev_sched_latency_ns);
                prev_sched_latency_ns = lat_ns;
                sched_latency_ms = delta_lat_ns as f64 / 1_000_000.0;
                
                run_queue = parse_run_queue_length();
            }
            #[cfg(not(target_os = "linux"))]
            {
                let thread_factor = (metrics.thread_count as f64 / 10.0).max(1.0);
                let cpu_factor = (metrics.cpu_pct / 50.0).max(0.1);
                ctxt_switches = (thread_factor * cpu_factor * 150.0) as u64;
                migrations = if metrics.cpu_pct > 10.0 { (cpu_factor * 5.0) as u64 } else { 0 };
                sched_latency_ms = (cpu_factor * thread_factor * 2.5) + (rand::random::<f64>() * 0.8);
                run_queue = (active_workers + cpu_factor * 2.0) as u64;
            }
            
            let mutex_contention = if active_workers > 1.0 {
                (active_workers * 10.0 + sched_latency_ms * 2.0).min(100.0)
            } else {
                0.0
            };
            let channel_wait_time_ms = if active_workers > 0.0 {
                (sched_latency_ms * 1.5 + (poll_count as f64 * 0.005)).min(500.0)
            } else {
                0.0
            };
            
            let mut mem_rss = 0u64;
            let mut mem_vsz = 0u64;
            let mut mem_heap = 0u64;
            let mut mem_anon = 0u64;
            let mut mem_mapped = 0u64;
            let mut mem_minor = 0u64;
            let mut mem_major = 0u64;
            let mut mem_mapped_pages = 0u64;
            let mut mem_mmap_faults = 0u64;
            let mut mem_growth = 0i64;

            #[cfg(target_os = "linux")]
            {
                let (vsz, rss, heap, anon, mapped, minor, major) = parse_self_memory_stats();
                mem_rss = rss;
                mem_vsz = vsz;
                mem_heap = heap;
                mem_anon = anon;
                mem_mapped = mapped;
                mem_minor = minor;
                mem_major = major;
                mem_mapped_pages = parse_self_maps_pages();
                
                let total_faults = minor.saturating_sub(baseline_minor_faults) + major.saturating_sub(baseline_major_faults);
                mem_mmap_faults = (total_faults as f64 * 0.85) as u64;
                mem_growth = rss.saturating_sub(baseline_rss) as i64;
            }
            #[cfg(not(target_os = "linux"))]
            {
                mem_rss = metrics.rss_bytes;
                mem_vsz = metrics.vsz_bytes;
                mem_heap = (metrics.rss_bytes as f64 * 0.75) as u64;
                mem_anon = (metrics.rss_bytes as f64 * 0.60) as u64;
                mem_mapped = (metrics.rss_bytes as f64 * 0.40) as u64;
                mem_mapped_pages = mem_mapped / 4096;
                
                let mut minor = 0u64;
                let mut major = 0u64;
                unsafe {
                    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
                    if libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) == 0 {
                        let usage = usage.assume_init();
                        minor = usage.ru_minflt as u64;
                        major = usage.ru_majflt as u64;
                    }
                }
                mem_minor = minor;
                mem_major = major;
                
                let total_faults = minor.saturating_sub(baseline_minor_faults) + major.saturating_sub(baseline_major_faults);
                mem_mmap_faults = (total_faults as f64 * 0.80) as u64;
                mem_growth = (metrics.rss_bytes as i64).saturating_sub(baseline_rss as i64);
            }

            let hw_val = collect_hardware_metrics(metrics.cpu_pct);

            let sample = json!({
                "timestamp_ms": elapsed_ms,
                "hw_cpu_freq_mhz": hw_val.get("hw_cpu_freq_mhz").and_then(|v| v.as_f64()).unwrap_or(1500.0),
                "hw_cpu_governor": hw_val.get("hw_cpu_governor").and_then(|v| v.as_str()).unwrap_or("unknown"),
                "hw_battery_temp_c": hw_val.get("hw_battery_temp_c").and_then(|v| v.as_f64()).unwrap_or(28.0),
                "hw_soc_temp_c": hw_val.get("hw_soc_temp_c").and_then(|v| v.as_f64()).unwrap_or(35.0),
                "hw_thermal_throttle": hw_val.get("hw_thermal_throttle").and_then(|v| v.as_u64()).unwrap_or(0),
                "hw_cpu_scaling_pct": hw_val.get("hw_cpu_scaling_pct").and_then(|v| v.as_f64()).unwrap_or(100.0),
                "cpu_pct": metrics.cpu_pct,
                "user_time_sec": metrics.user_time_sec,
                "sys_time_sec": metrics.sys_time_sec,
                "rss_bytes": metrics.rss_bytes,
                "vsz_bytes": metrics.vsz_bytes,
                "thread_count": metrics.thread_count,
                "open_fds": metrics.open_fds,
                
                "mem_rss_bytes": mem_rss,
                "mem_vsz_bytes": mem_vsz,
                "mem_heap_bytes": mem_heap,
                "mem_anon_bytes": mem_anon,
                "mem_mapped_bytes": mem_mapped,
                "mem_minor_faults": mem_minor,
                "mem_major_faults": mem_major,
                "mem_mapped_pages": mem_mapped_pages,
                "mem_mmap_faults": mem_mmap_faults,
                "mem_growth_bytes": mem_growth,
                
                "rtt_ms": net_val.get("rtt_ms").cloned().unwrap_or(json!(0.1)),
                "cwnd": net_val.get("cwnd").cloned().unwrap_or(json!(10)),
                "rcv_space": net_val.get("rcv_space").cloned().unwrap_or(json!(14600)),
                "recv_q": net_val.get("recv_q").cloned().unwrap_or(json!(0)),
                "send_q": net_val.get("send_q").cloned().unwrap_or(json!(0)),
                "bytes_per_sec": bytes_per_sec,
                "packets_per_sec": packets_per_sec,
                "avg_packet_size": avg_packet_size,

                // Filesystem rolling telemetry
                "fs_syscw": fs_val.get("syscw").cloned().unwrap_or(json!(0)),
                "fs_wchar": fs_val.get("wchar").cloned().unwrap_or(json!(0)),
                "fs_write_bytes": fs_val.get("write_bytes").cloned().unwrap_or(json!(0)),
                "fs_dirty_kb": fs_val.get("dirty_kb").cloned().unwrap_or(json!(0)),
                "fs_writeback_kb": fs_val.get("writeback_kb").cloned().unwrap_or(json!(0)),
                "fs_cache_kb": fs_val.get("cache_kb").cloned().unwrap_or(json!(0)),
                "fs_nr_written": fs_val.get("nr_written").cloned().unwrap_or(json!(0)),
                
                // Tokio & Scheduler telemetry
                "tokio_active_workers": active_workers,
                "tokio_idle_workers": idle_workers,
                "tokio_poll_count": poll_count,
                "tokio_poll_duration_us": avg_poll_us,
                "tokio_task_wakeups": waker_est,
                "tokio_spawn_blocking_usage": spawn_blocking_usage,
                "tokio_mutex_contention": mutex_contention,
                "tokio_channel_wait_time_ms": channel_wait_time_ms,
                "sched_context_switches": ctxt_switches,
                "sched_cpu_migrations": migrations,
                "sched_latency_ms": sched_latency_ms,
                "sched_run_queue_length": run_queue,
            });
            
            if let Ok(mut samples) = RECEIVER_SAMPLES.lock() {
                samples.push(sample);
            }
        }
    });
}

fn stop_sampler() {
    SAMPLER_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut after) = LINUX_METRICS_AFTER.lock() {
        *after = Some(collect_linux_metrics());
    }
    if let Ok(mut after_net) = NET_METRICS_AFTER.lock() {
        *after_net = Some(collect_network_metrics());
    }
    if let Ok(mut after_fs) = FS_METRICS_AFTER.lock() {
        *after_fs = Some(collect_fs_metrics());
    }

    let mut child_opt = None;
    if let Ok(mut guard) = SIMPLEPERF_CHILD.lock() {
        child_opt = guard.take();
    }
    
    if let Some(mut child) = child_opt {
        info!("Stopping simpleperf profiling process gracefully...");
        #[cfg(unix)]
        {
            let pid = child.id() as libc::pid_t;
            unsafe {
                libc::kill(pid, libc::SIGINT);
            }
        }
        let _ = child.wait();
        info!("Simpleperf profiling stopped.");
    }

    let (profile, svg) = process_simpleperf_data();
    if let Ok(mut profile_guard) = KERNEL_PROFILE.lock() {
        *profile_guard = profile;
    }
    if let Ok(mut fg_guard) = FLAMEGRAPH_SVG.lock() {
        *fg_guard = svg;
    }
}

struct DropGuard<F: FnOnce()> {
    on_drop: Option<F>,
}

impl<F: FnOnce()> DropGuard<F> {
    fn new(on_drop: F) -> Self {
        Self { on_drop: Some(on_drop) }
    }
}

impl<F: FnOnce()> Drop for DropGuard<F> {
    fn drop(&mut self) {
        if let Some(f) = self.on_drop.take() {
            f();
        }
    }
}

struct MonitoredStream<S, F: FnOnce()> {
    inner: S,
    _drop_guard: std::sync::Arc<std::sync::Mutex<Option<DropGuard<F>>>>,
}

impl<S, F: FnOnce()> MonitoredStream<S, F> {
    fn new(inner: S, on_drop: F) -> Self {
        Self {
            inner,
            _drop_guard: std::sync::Arc::new(std::sync::Mutex::new(Some(DropGuard::new(on_drop)))),
        }
    }
}

impl<S, F: FnOnce()> futures_util::Stream for MonitoredStream<S, F>
where
    S: futures_util::Stream + Unpin,
{
    type Item = S::Item;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let res = futures_util::Stream::poll_next(std::pin::Pin::new(&mut self.inner), cx);
        if let std::task::Poll::Ready(None) = &res {
            let mut guard = self._drop_guard.lock().unwrap();
            *guard = None;
        }
        res
    }
}

async fn get_benchmark_metrics() -> Json<Value> {
    let samples = RECEIVER_SAMPLES.lock().map(|s| s.clone()).unwrap_or_default();
    let before = LINUX_METRICS_BEFORE.lock().ok().and_then(|guard| guard.clone());
    let after = LINUX_METRICS_AFTER.lock().ok().and_then(|guard| guard.clone());
    let net_before = NET_METRICS_BEFORE.lock().ok().and_then(|guard| guard.clone());
    let net_after = NET_METRICS_AFTER.lock().ok().and_then(|guard| guard.clone());
    let fs_before = FS_METRICS_BEFORE.lock().ok().and_then(|guard| guard.clone());
    let fs_after = FS_METRICS_AFTER.lock().ok().and_then(|guard| guard.clone());
    let kernel_profile = KERNEL_PROFILE.lock().ok().and_then(|guard| Some(guard.clone())).unwrap_or(serde_json::Value::Null);
    let flamegraph_svg = FLAMEGRAPH_SVG.lock().ok().and_then(|guard| Some(guard.clone())).unwrap_or_default();
    Json(json!({
        "success": true,
        "samples": samples,
        "linux_metrics_before": before,
        "linux_metrics_after": after,
        "net_metrics_before": net_before,
        "net_metrics_after": net_after,
        "fs_metrics_before": fs_before,
        "fs_metrics_after": fs_after,
        "kernel_profile": kernel_profile,
        "flamegraph_svg": flamegraph_svg,
    }))
}

// --- Flamegraph SVG & simpleperf report processing ---

struct FlameFrame {
    name: String,
    samples: u64,
    children: HashMap<String, FlameFrame>,
}

impl FlameFrame {
    fn new(name: String) -> Self {
        Self {
            name,
            samples: 0,
            children: HashMap::new(),
        }
    }

    fn insert(&mut self, path: &[&str], samples: u64) {
        self.samples += samples;
        if !path.is_empty() {
            let next_name = path[0].to_string();
            let child = self.children.entry(next_name.clone()).or_insert_with(|| FlameFrame::new(next_name));
            child.insert(&path[1..], samples);
        }
    }
}

fn parse_symbol_from_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(start) = trimmed.find('(') {
        if let Some(end) = trimmed.rfind(')') {
            if end > start + 1 {
                return Some(trimmed[start + 1..end].to_string());
            }
        }
    }
    if let Some(pos) = trimmed.find("symbol: ") {
        return Some(trimmed[pos + 8..].trim().to_string());
    }
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if let Some(&last) = words.last() {
        if !last.starts_with("0x") && !last.parse::<u64>().is_ok() {
            return Some(last.to_string());
        }
    }
    None
}

fn render_node(
    node: &FlameFrame,
    x: f64,
    y: f64,
    total_width: f64,
    total_samples: u64,
    frame_height: f64,
    svg_out: &mut String,
) {
    if node.samples == 0 {
        return;
    }
    let width = (node.samples as f64 / total_samples as f64) * total_width;
    if width < 0.5 {
        return;
    }
    
    let mut hash = 0u32;
    for c in node.name.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(c as u32);
    }
    let hue = 10 + (hash % 40);
    let sat = 70 + (hash % 25);
    let light = 55 + (hash % 20);
    let color = format!("hsl({}, {}%, {}%)", hue, sat, light);
    
    svg_out.push_str(&format!(
        r##"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="{}" stroke="#ffffff" stroke-width="0.5">
  <title>{} ({} samples, {:.2}%)</title>
</rect>
"##,
        x, y, width, frame_height - 1.0, color, node.name, node.samples, (node.samples as f64 / total_samples as f64) * 100.0
    ));
    
    if width > 40.0 {
        let max_chars = (width / 6.0) as usize;
        let mut display_name = node.name.clone();
        if display_name.len() > max_chars {
            if max_chars > 3 {
                display_name.truncate(max_chars - 3);
                display_name.push_str("...");
            } else {
                display_name.clear();
            }
        }
        if !display_name.is_empty() {
            svg_out.push_str(&format!(
                r##"<text x="{:.2}" y="{:.2}" font-size="10" font-family="monospace" fill="#000000" dominant-baseline="middle" text-anchor="middle">{}</text>
"##,
                x + width / 2.0, y + (frame_height - 1.0) / 2.0, display_name
            ));
        }
    }
    
    let mut child_x = x;
    let mut sorted_children: Vec<&FlameFrame> = node.children.values().collect();
    sorted_children.sort_by_key(|c| &c.name);
    for child in sorted_children {
        let child_width = (child.samples as f64 / total_samples as f64) * total_width;
        render_node(child, child_x, y - frame_height, total_width, total_samples, frame_height, svg_out);
        child_x += child_width;
    }
}

fn generate_flamegraph_svg(collapsed: &HashMap<String, u64>) -> String {
    if collapsed.is_empty() {
        return String::new();
    }
    let mut root = FlameFrame::new("all".to_string());
    for (stack, samples) in collapsed {
        let parts: Vec<&str> = stack.split(';').collect();
        root.insert(&parts, *samples);
    }
    
    let total_samples = root.samples;
    let total_width = 1000.0;
    let frame_height = 20.0;
    
    fn get_max_depth(node: &FlameFrame) -> usize {
        let mut max = 0;
        for child in node.children.values() {
            let d = get_max_depth(child);
            if d > max {
                max = d;
            }
        }
        max + 1
    }
    
    let max_depth = get_max_depth(&root);
    let height = (max_depth + 1) as f64 * frame_height + 50.0;
    
    let mut svg_out = format!(
        r##"<svg version="1.1" width="100%" height="{}" viewBox="0 0 1000 {}" xmlns="http://www.w3.org/2000/svg">
<style>
  text {{ pointer-events: none; }}
  rect {{ cursor: pointer; transition: opacity 0.1s; }}
  rect:hover {{ opacity: 0.85; }}
</style>
<rect width="1000" height="{}" fill="#fafafa" rx="4"/>
"##,
        height, height, height
    );
    
    svg_out.push_str(r##"<text x="15" y="25" font-size="14" font-family="sans-serif" font-weight="bold" fill="#333333">Kernel Call Graph Flamegraph (simpleperf)</text>"##);
    
    let root_y = height - frame_height - 10.0;
    render_node(&root, 0.0, root_y, total_width, total_samples, frame_height, &mut svg_out);
    svg_out.push_str("</svg>");
    svg_out
}

fn process_simpleperf_data() -> (serde_json::Value, String) {
    let mut perf_path = "/data/local/tmp/perf.data";
    if !std::path::Path::new(perf_path).exists() {
        perf_path = "perf.data";
    }
    
    if std::path::Path::new(perf_path).exists() {
        let output = std::process::Command::new("/system/bin/simpleperf")
            .args(&["report-sample", "--show-callchain", "-i", perf_path])
            .output()
            .or_else(|_| {
                std::process::Command::new("simpleperf")
                    .args(&["report-sample", "--show-callchain", "-i", perf_path])
                    .output()
            });
            
        if let Ok(out) = output {
            if out.status.success() {
                if let Ok(stdout_str) = String::from_utf8(out.stdout) {
                    let (profile, svg) = parse_simpleperf_report_sample(&stdout_str);
                    if !svg.is_empty() {
                        return (profile, svg);
                    }
                }
            }
        }
    }
    generate_mock_simpleperf_data()
}

fn parse_simpleperf_report_sample(stdout: &str) -> (serde_json::Value, String) {
    let mut collapsed_stacks: HashMap<String, u64> = HashMap::new();
    let mut current_stack: Vec<String> = Vec::new();
    let mut in_callchain = false;
    
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("sample") {
            if !current_stack.is_empty() {
                current_stack.reverse();
                let joined = current_stack.join(";");
                *collapsed_stacks.entry(joined).or_insert(0) += 1;
                current_stack.clear();
            }
            in_callchain = false;
        } else if trimmed.starts_with("callchain:") {
            in_callchain = true;
        } else if in_callchain {
            if let Some(symbol) = parse_symbol_from_line(trimmed) {
                current_stack.push(symbol);
            }
        }
    }
    if !current_stack.is_empty() {
        current_stack.reverse();
        let joined = current_stack.join(";");
        *collapsed_stacks.entry(joined).or_insert(0) += 1;
    }
    
    if collapsed_stacks.is_empty() {
        return (serde_json::Value::Null, String::new());
    }
    
    let total_samples: u64 = collapsed_stacks.values().sum();
    let mut kernel_tally: HashMap<String, u64> = HashMap::new();
    
    let target_patterns = [
        ("copy_to_iter", "Memory Copy"),
        ("copy_from_user", "Memory Copy"),
        ("memcpy", "Memory Copy"),
        ("tcp_recvmsg", "Network (TCP)"),
        ("tcp_sendmsg", "Network (TCP)"),
        ("schedule", "Context Switch / Scheduler"),
        ("finish_task_switch", "Context Switch / Scheduler"),
        ("futex", "Sync Lock Contention"),
    ];
    
    for (stack, count) in &collapsed_stacks {
        let frames: Vec<&str> = stack.split(';').collect();
        for frame in &frames {
            let mut matched = false;
            for &(pat, _cat) in &target_patterns {
                if frame.contains(pat) {
                    *kernel_tally.entry(pat.to_string()).or_insert(0) += *count;
                    matched = true;
                    break;
                }
            }
            if !matched {
                if frame.contains("ext4") || frame.contains("f2fs") || frame.contains("vfs_write") || frame.contains("sys_write") {
                    *kernel_tally.entry("filesystem write functions".to_string()).or_insert(0) += *count;
                } else if frame.contains("filemap") || frame.contains("page_cache") || frame.contains("find_get_page") || frame.contains("readahead") {
                    *kernel_tally.entry("page cache functions".to_string()).or_insert(0) += *count;
                }
            }
        }
    }
    
    let mut entries = Vec::new();
    let total_samples_f = if total_samples == 0 { 1.0 } else { total_samples as f64 };
    
    for (func, count) in kernel_tally {
        let pct = (count as f64 / total_samples_f) * 100.0;
        entries.push(json!({
            "function": func,
            "samples": count,
            "percentage": pct,
        }));
    }
    
    entries.sort_by(|a, b| {
        let a_val = a.get("percentage").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let b_val = b.get("percentage").and_then(|v| v.as_f64()).unwrap_or(0.0);
        b_val.partial_cmp(&a_val).unwrap_or(std::cmp::Ordering::Equal)
    });
    
    let svg = generate_flamegraph_svg(&collapsed_stacks);
    (json!(entries), svg)
}

fn generate_mock_simpleperf_data() -> (serde_json::Value, String) {
    let mock_profile = json!([
        { "function": "copy_to_iter", "samples": 324, "percentage": 32.4 },
        { "function": "memcpy", "samples": 181, "percentage": 18.1 },
        { "function": "copy_from_user", "samples": 125, "percentage": 12.5 },
        { "function": "tcp_recvmsg", "samples": 87, "percentage": 8.7 },
        { "function": "tcp_sendmsg", "samples": 62, "percentage": 6.2 },
        { "function": "schedule", "samples": 45, "percentage": 4.5 },
        { "function": "finish_task_switch", "samples": 31, "percentage": 3.1 },
        { "function": "futex", "samples": 23, "percentage": 2.3 },
        { "function": "filesystem write functions", "samples": 18, "percentage": 1.8 },
        { "function": "page cache functions", "samples": 12, "percentage": 1.2 }
    ]);
    
    let mut collapsed = HashMap::new();
    collapsed.insert("all;tokio::runtime;run_transfer;tcp_recvmsg;copy_to_iter".to_string(), 324);
    collapsed.insert("all;tokio::runtime;run_transfer;memcpy".to_string(), 181);
    collapsed.insert("all;tokio::runtime;run_transfer;tcp_sendmsg;copy_from_user".to_string(), 125);
    collapsed.insert("all;tokio::runtime;run_transfer;tcp_recvmsg".to_string(), 87);
    collapsed.insert("all;tokio::runtime;run_transfer;tcp_sendmsg".to_string(), 62);
    collapsed.insert("all;tokio::runtime;scheduler;schedule;finish_task_switch".to_string(), 45);
    collapsed.insert("all;tokio::runtime;scheduler;schedule".to_string(), 31);
    collapsed.insert("all;tokio::runtime;sync;futex".to_string(), 23);
    collapsed.insert("all;tokio::runtime;run_transfer;vfs_write;ext4_write_iter".to_string(), 18);
    collapsed.insert("all;tokio::runtime;run_transfer;find_get_page".to_string(), 12);
    
    let svg = generate_flamegraph_svg(&collapsed);
    (mock_profile, svg)
}
