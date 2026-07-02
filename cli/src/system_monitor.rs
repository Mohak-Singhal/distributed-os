use chrono::Local;
use std::collections::VecDeque;
use std::sync::Mutex;
use sysinfo::{Networks, Pid, System, Disks};

lazy_static::lazy_static! {
    pub static ref TRANSFER_PROGRESS: Mutex<serde_json::Value> = Mutex::new(serde_json::json!({
        "active": false,
        "filename": "",
        "bytes_sent": 0,
        "total_bytes": 0,
        "progress_pct": 0.0,
        "speed_mbps": 0.0,
        "status": "idle"
    }));
    pub static ref OPERATION_LOGS: Mutex<VecDeque<serde_json::Value>> = Mutex::new(VecDeque::with_capacity(500));
    pub static ref CURRENT_METRICS: Mutex<serde_json::Value> = Mutex::new(serde_json::json!({
        "cpu_usage": 0.0,
        "memory_mb": 0.0,
        "network_tx_mbps": 0.0,
        "network_rx_mbps": 0.0,
        "processes": {},
        "thermal": {},
        "disks": [],
        "interfaces": [],
        "protocol_stats": {},
        "logs": []
    }));
    pub static ref REMOTE_METRICS: Mutex<serde_json::Value> = Mutex::new(serde_json::json!({}));
    pub static ref ANDROID_IP: Mutex<Option<String>> = Mutex::new(None);
    pub static ref REMOTE_PROGRESS: Mutex<serde_json::Value> = Mutex::new(serde_json::json!({
        "active": false,
        "filename": "",
        "bytes_sent": 0,
        "total_bytes": 0,
        "progress_pct": 0.0,
        "speed_mbps": 0.0,
        "status": "idle"
    }));
    pub static ref SENDER_TRANSFER_PROGRESS: Mutex<serde_json::Value> = Mutex::new(serde_json::json!({
        "active": false,
        "filename": "",
        "bytes_sent": 0,
        "total_bytes": 0,
        "progress_pct": 0.0,
        "speed_mbps": 0.0,
        "status": "idle"
    }));
    pub static ref REMOTE_CAPABILITIES: Mutex<Option<Box<crate::transfer_engine::capabilities::CapabilityExchange>>> = Mutex::new(None);
}

pub fn log_op(level: &str, msg: &str) {
    let now = Local::now().format("%H:%M:%S").to_string();
    let log_entry = serde_json::json!({
        "time": now,
        "level": level.to_uppercase(),
        "msg": msg
    });

    if let Ok(mut logs) = OPERATION_LOGS.lock() {
        if logs.len() >= 500 {
            logs.pop_front();
        }
        logs.push_back(log_entry.clone());
        println!("[{}] {} - {}", now, level.to_uppercase(), msg);
    }
}

pub async fn start_monitoring() {
    // Spawn background mDNS listener to continuously discover Android LAN IP
    tokio::spawn(async {
        if let Ok(mdns) = mdns_sd::ServiceDaemon::new() {
            if let Ok(receiver) = mdns.browse("_xync._tcp.local.") {
                while let Ok(event) = receiver.recv_async().await {
                    if let mdns_sd::ServiceEvent::ServiceResolved(svc) = event {
                        let is_android = svc.get_property_val_str("platform").map(|p| p == "android").unwrap_or(false)
                            || svc.get_fullname().contains("Android")
                            || svc.get_fullname().contains("android");
                        if is_android {
                            if let Some(ip) = svc.get_addresses_v4().iter().next() {
                                if let Ok(mut android_ip) = ANDROID_IP.lock() {
                                    *android_ip = Some(ip.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    let mut sys = System::new_all();
    let mut networks = Networks::new_with_refreshed_list();
    let mut disks = Disks::new_with_refreshed_list();
    let pid = sysinfo::get_current_pid().unwrap_or(Pid::from(0));

    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    networks.refresh(true);
    disks.refresh(true);

    let mut prev_rx = 0u64;
    let mut prev_tx = 0u64;

    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;

        // Poll Android telemetry if we have discovered an IP
        let android_ip_opt = {
            if let Ok(ip) = ANDROID_IP.lock() {
                ip.clone()
            } else {
                None
            }
        };

        if let Some(ip) = android_ip_opt {
            tokio::spawn(async move {
                if let Ok(val) = fetch_android_telemetry(&ip).await {
                    let cpu_usage_str = match val.get("cpu_usage") {
                        Some(serde_json::Value::String(s)) => s.clone(),
                        Some(serde_json::Value::Number(n)) => format!("{:.1}", n.as_f64().unwrap_or(0.0)),
                        _ => "0.0".to_string(),
                    };
                    let ram_usage_str = match val.get("memory_mb") {
                        Some(serde_json::Value::String(s)) => s.clone(),
                        Some(serde_json::Value::Number(n)) => format!("{:.0}", n.as_f64().unwrap_or(0.0)),
                        _ => match val.get("ram_usage") {
                            Some(serde_json::Value::Number(n)) => format!("{:.0}", n.as_f64().unwrap_or(0.0)),
                            Some(serde_json::Value::String(s)) => s.clone(),
                            _ => "0".to_string(),
                        },
                    };
                    if let Ok(mut rm) = REMOTE_METRICS.lock() {
                        *rm = serde_json::json!({
                            "cpu_usage": cpu_usage_str,
                            "memory_mb": ram_usage_str,
                        });
                    }
                }
            });
        }
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        networks.refresh(true);
        disks.refresh(true);

        let mut cpu_usage = 0.0;
        let mut mem_usage = 0.0;

        if let Some(process) = sys.process(pid) {
            cpu_usage = process.cpu_usage();
            mem_usage = process.memory() as f64 / (1024.0 * 1024.0);
        }

        let mut rx_bytes = 0u64;
        let mut tx_bytes = 0u64;
        let mut interfaces = Vec::new();

        for (name, data) in &networks {
            rx_bytes += data.received();
            tx_bytes += data.transmitted();
            interfaces.push(serde_json::json!({
                "name": name,
                "rx": data.received(),
                "tx": data.transmitted(),
                "mac": format!("{}", data.mac_address()),
            }));
        }

        let rx_delta = rx_bytes.saturating_sub(prev_rx);
        let tx_delta = tx_bytes.saturating_sub(prev_tx);
        let rx_mbps = (rx_delta as f64 * 8.0) / 2_000_000.0;
        let tx_mbps = (tx_delta as f64 * 8.0) / 2_000_000.0;
        prev_rx = rx_bytes;
        prev_tx = tx_bytes;

        let disk_list: Vec<serde_json::Value> = disks.iter().map(|d| {
            serde_json::json!({
                "name": d.name().to_string_lossy(),
                "mount": d.mount_point().to_string_lossy(),
                "total_gb": d.total_space() as f64 / 1e9,
                "available_gb": d.available_space() as f64 / 1e9,
                "file_system": d.file_system().to_string_lossy(),
            })
        }).collect();

        let logs: Vec<serde_json::Value> = {
            if let Ok(queue) = OPERATION_LOGS.lock() {
                queue.iter().cloned().collect()
            } else {
                Vec::new()
            }
        };

        let remote_metrics = {
            if let Ok(rm) = REMOTE_METRICS.lock() {
                rm.clone()
            } else {
                serde_json::json!({})
            }
        };

        let thermal_info = {
            if let Ok(mut thermal) = crate::telemetry::THERMAL_STATE.lock() {
                if thermal.cpu_temp_c.is_none() {
                    if let Ok(out) = std::process::Command::new("pmset").args(["-g", "therm"]).output() {
                        if let Ok(s) = String::from_utf8(out.stdout) {
                            if s.contains("CPU_Scheduler_Limit") {
                                if let Some(line) = s.lines().find(|l| l.contains("CPU_Scheduler_Limit")) {
                                    if let Some(val) = line.split('=').nth(1).and_then(|v| v.trim().parse::<f64>().ok()) {
                                        thermal.cpu_temp_c = Some(40.0 + (100.0 - val) * 0.6);
                                    }
                                }
                            }
                        }
                    }
                }
                if thermal.thermal_state.is_empty() || thermal.thermal_state == "Nominal" {
                    let out = std::process::Command::new("pmset").args(["-g", "therm"]).output().ok()
                        .and_then(|o| String::from_utf8(o.stdout).ok()).unwrap_or_default();
                    thermal.thermal_state = if out.contains("CPU_Scheduler_Limit") && out.contains("Critical") { "Critical".into() }
                    else if out.contains("CPU_Scheduler_Limit") { "Fair".into() }
                    else { "Nominal".into() };
                }
                if thermal.battery_pct.is_none() {
                    let out = std::process::Command::new("pmset").args(["-g", "batt"]).output().ok()
                        .and_then(|o| String::from_utf8(o.stdout).ok()).unwrap_or_default();
                    thermal.battery_pct = out.lines()
                        .filter_map(|l| {
                            let pct_pos = l.find('%')?;
                            let start = l[..pct_pos].rfind(|c: char| !c.is_ascii_digit() && c != '.')? + 1;
                            l[start..pct_pos].parse::<f64>().ok()
                        })
                        .next();
                }
                serde_json::json!({
                    "cpu_temp_c": thermal.cpu_temp_c,
                    "thermal_state": thermal.thermal_state,
                    "fan_rpm": thermal.fan_rpm,
                    "battery_pct": thermal.battery_pct,
                    "battery_temp_c": thermal.battery_temp_c,
                })
            } else {
                serde_json::json!({})
            }
        };

        let process_metrics = {
            if let Ok(pm) = crate::telemetry::PROCESS_METRICS.lock() {
                serde_json::json!({
                    "rust_daemon": { "cpu": pm.rust_daemon_cpu, "ram_mb": pm.rust_daemon_ram },
                    "hash_thread": { "cpu": pm.hash_thread_cpu, "ram_mb": pm.hash_thread_ram },
                })
            } else {
                serde_json::json!({})
            }
        };

        let protocol_stats = {
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
                })
            } else {
                serde_json::json!({})
            }
        };

        if let Ok(mut metrics) = CURRENT_METRICS.lock() {
            *metrics = serde_json::json!({
                "cpu_usage": format!("{:.1}", cpu_usage),
                "global_cpu": format!("{:.1}", sys.global_cpu_usage()),
                "memory_mb": format!("{:.1}", mem_usage),
                "total_memory_mb": format!("{:.1}", sys.total_memory() as f64 / (1024.0 * 1024.0)),
                "used_memory_mb": format!("{:.1}", sys.used_memory() as f64 / (1024.0 * 1024.0)),
                "network_tx_mbps": format!("{:.2}", tx_mbps),
                "network_rx_mbps": format!("{:.2}", rx_mbps),
                "android_cpu": remote_metrics.get("cpu_usage").and_then(|v| v.as_str()).unwrap_or("0.0"),
                "android_ram": remote_metrics.get("memory_mb").and_then(|v| v.as_str()).unwrap_or("0"),
                "processes": process_metrics,
                "thermal": thermal_info,
                "disks": disk_list,
                "interfaces": interfaces,
                "protocol_stats": protocol_stats,
                "logs": logs
            });
        }
    }
}

async fn fetch_android_telemetry(ip: &str) -> anyhow::Result<serde_json::Value> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use std::time::Duration;

    let addr = format!("{}:7894", ip);
    let stream_fut = TcpStream::connect(&addr);
    let mut stream = tokio::time::timeout(Duration::from_millis(500), stream_fut).await??;

    let request = "GET /api/telemetry HTTP/1.1\r\n\
                   Host: localhost\r\n\
                   Connection: close\r\n\
                   \r\n";
    
    stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::new();
    let read_fut = stream.read_to_end(&mut response);
    tokio::time::timeout(Duration::from_millis(500), read_fut).await??;

    let response_str = String::from_utf8_lossy(&response);
    if let Some(body_start) = response_str.find("\r\n\r\n") {
        let body = &response_str[body_start + 4..];
        let val: serde_json::Value = serde_json::from_str(body)?;
        return Ok(val);
    }
    Err(anyhow::anyhow!("Invalid HTTP response"))
}
