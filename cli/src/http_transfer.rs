// ── HTTP Transfer — Legacy compatibility layer ───────────────────────────
//
// All real transfer logic lives in `transfer_engine::http`.
// This file is kept for backward compatibility only.
// New code should use `transfer_engine::TransferCoordinator` directly.
//
// TODO: Remove this file once all callers are updated.

use std::path::Path;
use std::sync::Arc;
use crate::telemetry::TransferSession;
use transfer_engine::TransferOptions;

// ── Upload ───────────────────────────────────────────────────────────────

pub async fn http_upload(
    host: &str,
    port: u16,
    local_path: &str,
    remote_filename: Option<&str>,
    progress_cb: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
) -> anyhow::Result<TransferSession> {
    let path = Path::new(local_path);
    let size = if path.is_file() {
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };
    let display_name = remote_filename
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string()
        });

    let benchmark = crate::observability::BenchmarkSession::start("upload", &display_name, size, host, port);

    let bytes_clone = benchmark.bytes_transferred.clone();
    let progress_cb_clone = progress_cb.clone();
    let wrapped_cb = Arc::new(move |transferred: u64, total: u64| {
        bytes_clone.store(transferred, std::sync::atomic::Ordering::SeqCst);
        if let Some(ref cb) = progress_cb_clone {
            cb(transferred, total);
        }
    });

    let options = TransferOptions {
        progress_cb: Some(wrapped_cb),
        ..TransferOptions::default()
    };

    let result_res = transfer_engine::http::upload_to(host, port, local_path, remote_filename, options).await;
    benchmark.stop().await;
    let result = result_res?;

    Ok(TransferSession {
        filename: display_name,
        original_size: result.bytes_sent,
        compressed_size: None,
        average_speed_mbps: result.speed_mbps,
        ..Default::default()
    })
}

pub async fn http_upload_parallel(
    host: &str,
    port: u16,
    path: &Path,
    display_name: &str,
    num_streams: usize,
    _chunk_size: usize,
    _progress_cb: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
) -> anyhow::Result<TransferSession> {
    let options = TransferOptions {
        parallel: true,
        parallel_streams: num_streams,
        ..Default::default()
    };
    let result = transfer_engine::http::upload_parallel(
        host, port, path, display_name, num_streams, options.chunk_size,
    ).await?;
    Ok(TransferSession {
        filename: display_name.to_string(),
        original_size: result.bytes_sent,
        compressed_size: None,
        average_speed_mbps: result.speed_mbps,
        ..Default::default()
    })
}

pub async fn http_upload_with_chunk_size(
    host: &str,
    port: u16,
    local_path: &str,
    remote_filename: Option<&str>,
    chunk_size: usize,
    progress_cb: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
) -> anyhow::Result<TransferSession> {
    let path = Path::new(local_path);
    let size = if path.is_file() {
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };
    let display_name = remote_filename
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string()
        });

    let benchmark = crate::observability::BenchmarkSession::start("upload", &display_name, size, host, port);

    let bytes_clone = benchmark.bytes_transferred.clone();
    let progress_cb_clone = progress_cb.clone();
    let wrapped_cb = Arc::new(move |transferred: u64, total: u64| {
        bytes_clone.store(transferred, std::sync::atomic::Ordering::SeqCst);
        if let Some(ref cb) = progress_cb_clone {
            cb(transferred, total);
        }
    });

    let options = TransferOptions {
        chunk_size,
        progress_cb: Some(wrapped_cb),
        ..TransferOptions::default()
    };

    let result_res = transfer_engine::http::upload_to(host, port, local_path, remote_filename, options).await;
    benchmark.stop().await;
    let result = result_res?;

    Ok(TransferSession {
        filename: display_name,
        original_size: result.bytes_sent,
        compressed_size: None,
        average_speed_mbps: result.speed_mbps,
        ..Default::default()
    })
}

pub async fn http_upload_single_file(
    host: &str,
    port: u16,
    path: &Path,
    display_name: &str,
    chunk_size: usize,
    progress_cb: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
) -> anyhow::Result<TransferSession> {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let benchmark = crate::observability::BenchmarkSession::start("upload", display_name, size, host, port);

    let bytes_clone = benchmark.bytes_transferred.clone();
    let progress_cb_clone = progress_cb.clone();
    let wrapped_cb = Arc::new(move |transferred: u64, total: u64| {
        bytes_clone.store(transferred, std::sync::atomic::Ordering::SeqCst);
        if let Some(ref cb) = progress_cb_clone {
            cb(transferred, total);
        }
    });

    let options = TransferOptions {
        chunk_size,
        progress_cb: Some(wrapped_cb),
        ..TransferOptions::default()
    };

    let result_res = transfer_engine::http::upload_single_file(host, port, path, display_name, options).await;
    benchmark.stop().await;
    let result = result_res?;

    Ok(TransferSession {
        filename: display_name.to_string(),
        original_size: result.bytes_sent,
        compressed_size: None,
        average_speed_mbps: result.speed_mbps,
        ..Default::default()
    })
}

// ── Download (kept as-is, inline connection) ────────────────────────────

pub async fn http_download(
    host: &str,
    port: u16,
    path: &str,
    output_path: &str,
    progress_cb: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
) -> anyhow::Result<TransferSession> {
    http_download_with_chunk_size(host, port, path, output_path, 1_048_576, progress_cb).await
}

pub async fn http_download_with_chunk_size(
    host: &str,
    port: u16,
    path: &str,
    output_path: &str,
    chunk_size: usize,
    progress_cb: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
) -> anyhow::Result<TransferSession> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use crate::file::{TransferTracker, store_session};

    let output_filename = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("downloaded.bin");

    let bytes_clone = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let bc2 = bytes_clone.clone();
    let wrapped_cb = Arc::new(move |t: u64, h: u64| {
        bc2.store(t, std::sync::atomic::Ordering::SeqCst);
        if let Some(ref cb) = progress_cb { cb(t, h); }
    });

    let mut tracker = TransferTracker::new(output_filename, 0);
    tracker.begin_phase("Streaming");

    let mut stream = transfer_engine::http::connect_optimized(host, port).await?;

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, host
    );
    stream.write_all(request.as_bytes()).await?;

    let mut header_buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 1];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Err(anyhow::anyhow!("Connection closed while reading headers"));
        }
        header_buf.push(tmp[0]);
        if header_buf.ends_with(b"\r\n\r\n") { break; }
        if header_buf.len() > 65536 {
            return Err(anyhow::anyhow!("Response headers too large"));
        }
    }

    let header_str = String::from_utf8_lossy(&header_buf);
    if !header_str.starts_with("HTTP/1.1 200") && !header_str.starts_with("HTTP/1.0 200") {
        return Err(anyhow::anyhow!("Server returned non-200"));
    }

    let total_hint: u64 = header_str
        .lines()
        .find(|l| l.to_lowercase().starts_with("content-length:"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);

    let out_path = std::path::Path::new(output_path);
    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = tokio::fs::File::create(out_path).await?;
    let mut total: u64 = 0;
    let mut buf = vec![0u8; chunk_size];
    let mut batch_count = 0;

    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 { break; }
        file.write_all(&buf[..n]).await?;
        total += n as u64;
        wrapped_cb(total, total_hint);
        batch_count += 1;
        if batch_count >= 4 {
            tokio::task::yield_now().await;
            batch_count = 0;
        }
    }

    tracker.session.original_size = total;
    tracker.record_speed(total);
    tracker.end_phase();
    tracker.session.verified = true;
    let session = tracker.complete(0.0);
    store_session(session.clone());
    Ok(session)
}

// ── Directory stream (legacy, fallback to zip) ──────────────────────────

pub async fn stream_directory_send(
    host: &str,
    port: u16,
    dir_path: &Path,
    display_name: &str,
    _chunk_size: usize,
    _files: &[crate::transfer_engine::file_analyzer::FileEntry],
    _total_size: u64,
    progress_cb: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
) -> anyhow::Result<TransferSession> {
    http_upload(host, port, &dir_path.to_string_lossy(), Some(display_name), progress_cb).await
}

// ── Remote file browser (kept as-is) ────────────────────────────────────

#[derive(Debug)]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_file: bool,
    pub size: u64,
}

pub async fn http_list_files(host: &str, port: u16, path: &str) -> anyhow::Result<Vec<RemoteEntry>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let encoded = transfer_engine::http::urlencode(path);
    let request = format!(
        "GET /api/list-files?path={} HTTP/1.1\r\n\
         Host: {}:{}\r\n\
         Connection: close\r\n\
         \r\n",
        encoded, host, port
    );

    let mut stream = transfer_engine::http::connect_optimized(host, port).await?;
    stream.write_all(request.as_bytes()).await?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    let response_str = String::from_utf8_lossy(&buf);
    let body_start = response_str.find("\r\n\r\n")
        .map(|i| i + 4)
        .unwrap_or(0);
    let body = &response_str[body_start..];
    let entries: Vec<serde_json::Value> = serde_json::from_str(body)
        .map_err(|e| anyhow::anyhow!("JSON parse error: {}", e))?;
    let result = entries.into_iter().map(|v| RemoteEntry {
        name: v.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
        path: v.get("path").and_then(|n| n.as_str()).unwrap_or("").to_string(),
        is_dir: v.get("is_dir").and_then(|b| b.as_bool()).unwrap_or(false),
        is_file: v.get("is_file").and_then(|b| b.as_bool()).unwrap_or(true),
        size: v.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
    }).collect();
    Ok(result)
}

// ── Zip helpers ─────────────────────────────────────────────────────────

pub fn unzip_to(received_path: &str, output_dir: &str) -> anyhow::Result<()> {
    transfer_engine::http::unzip_to(received_path, output_dir)
}
