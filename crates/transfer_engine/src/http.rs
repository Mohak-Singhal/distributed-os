use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use sha2::Digest;

use crate::adaptive;
use crate::control::ControlLoop;
use crate::streaming::TransferSessionResult;
use crate::{CancelToken, TransferOptions};

// ── Connection ────────────────────────────────────────────────────────────

/// Optimized TCP connection with configurable buffers and retry logic.
pub async fn connect_optimized(host: &str, port: u16) -> anyhow::Result<TcpStream> {
    let addrs: Vec<_> = tokio::net::lookup_host(format!("{}:{}", host, port))
        .await?
        .collect();
    let mut last_err = None;

    for retry in 0..3 {
        for addr in &addrs {
            let socket = if addr.is_ipv4() {
                tokio::net::TcpSocket::new_v4()
            } else {
                tokio::net::TcpSocket::new_v6()
            };
            if let Ok(socket) = socket {
                let config = adaptive::get_active_config();
                let _ = socket.set_send_buffer_size((config.send_buffer_kb * 1024) as u32);
                let _ = socket.set_recv_buffer_size((config.recv_buffer_kb * 1024) as u32);
                match socket.connect(*addr).await {
                    Ok(s) => {
                        let _ = s.set_nodelay(true);
                        return Ok(s);
                    }
                    Err(e) => last_err = Some(e),
                }
            }
        }
        if retry < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(1000 * (retry + 1))).await;
        }
    }

    let msg = last_err
        .as_ref()
        .map(|e| e.to_string())
        .unwrap_or_default();
    if msg.contains("timed out") || msg.contains("Connection refused") {
        Err(anyhow::anyhow!(
            "Cannot reach {}:{}. Make sure the target device is running and the port is open. ({})",
            host, port, msg
        ))
    } else {
        Err(anyhow::anyhow!(
            "Failed to connect to {}:{}: {}",
            host, port, msg
        ))
    }
}

// ── URL encoding ──────────────────────────────────────────────────────────

pub fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".to_string(),
            _ => format!("%{:02X}", b),
        })
        .collect()
}

// ── Zip helpers ───────────────────────────────────────────────────────────

fn zip_directory(dir_path: &str) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut zip_writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
    let options = zip::write::FileOptions::<()>::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let base = Path::new(dir_path);
    add_dir_to_zip(&mut zip_writer, base, base, &options)?;
    zip_writer.finish()?;
    Ok(buf)
}

fn add_dir_to_zip<W: std::io::Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    base: &Path,
    dir: &Path,
    options: &zip::write::FileOptions<()>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let entry_path = entry.path();
        let relative = entry_path.strip_prefix(base)?;
        let name = relative.to_string_lossy().replace('\\', "/");

        if entry_path.is_dir() {
            zip.add_directory(&name, *options)?;
            add_dir_to_zip(zip, base, &entry_path, options)?;
        } else {
            zip.start_file(&name, *options)?;
            let mut f = std::fs::File::open(&entry_path)?;
            std::io::copy(&mut f, zip)?;
        }
    }
    Ok(())
}

pub fn unzip_to(received_path: &str, output_dir: &str) -> anyhow::Result<()> {
    let file = std::fs::File::open(received_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let target = Path::new(output_dir);
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let out_path = target.join(entry.name());
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out)?;
        }
    }
    Ok(())
}

// ── Stream file send (core streaming loop with closed-loop control) ──────

/// The closed-loop streaming workhorse.
///
/// Every chunk:
/// 1. Reads current config from the shared `ControlLoop` config (live adjustable)
/// 2. Writes the chunk to the TCP stream
/// 3. Records write metrics into the `ControlLoop`'s metrics collector
/// 4. Applies pacing if a rate limit is set
///
/// The background control loop runs every 100ms, reads the metrics,
/// classifies the bottleneck, and writes adjustments back to config.
pub(crate) async fn stream_file_send_inner(
    stream: impl tokio::io::AsyncWrite + Unpin + Send,
    header: &str,
    file_path: &Path,
    file_size: u64,
    chunk_size: usize,
    progress_cb: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
    control: Option<&ControlLoop>,
    cancel: Option<&CancelToken>,
) -> anyhow::Result<(u64, String)> {
    stream_file_send_with_resume(stream, header, file_path, file_size, chunk_size, 0, progress_cb, control, cancel).await
}

/// Like `stream_file_send_inner` but starts from a given offset for resume.
///
/// If `cancel` is provided and signals cancellation, sends a CANCEL frame
/// on the stream and returns `Cancelled` error.
pub(crate) async fn stream_file_send_with_resume(
    stream: impl tokio::io::AsyncWrite + Unpin + Send,
    header: &str,
    file_path: &Path,
    file_size: u64,
    _chunk_size: usize,
    resume_offset: u64,
    progress_cb: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
    control: Option<&ControlLoop>,
    cancel: Option<&CancelToken>,
) -> anyhow::Result<(u64, String)> {
    // Delegate to the streaming implementation
    send_streaming(stream, header, file_path, file_size, _chunk_size, resume_offset, progress_cb, control, cancel).await
}

/// Streaming file send — reads the file in chunks using `tokio::fs::File`,
/// hashes incrementally, writes to the transport stream.
///
/// This is the mmap-free replacement for the old mmap-based send path.
async fn send_streaming(
    mut stream: impl tokio::io::AsyncWrite + Unpin + Send,
    header: &str,
    file_path: &Path,
    file_size: u64,
    chunk_size: usize,
    resume_offset: u64,
    progress_cb: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
    control: Option<&ControlLoop>,
    cancel: Option<&CancelToken>,
) -> anyhow::Result<(u64, String)> {
    use tokio::io::AsyncWriteExt;

    if resume_offset == 0 {
        stream.write_all(header.as_bytes()).await?;
    }

    let mut file = tokio::fs::File::open(file_path).await?;
    let mut hasher = sha2::Sha256::new();

    // Pre-hash skipped bytes for resume
    if resume_offset > 0 {
        let mut skip_buf = vec![0u8; 65536];
        let mut remaining = resume_offset;
        while remaining > 0 {
            let to_read = remaining.min(65536) as usize;
            file.read_exact(&mut skip_buf[..to_read]).await?;
            hasher.update(&skip_buf[..to_read]);
            remaining -= to_read as u64;
        }
    }

    let mut offset = resume_offset;
    let mut batch_count = 0usize;
    let cs = if chunk_size > 0 { chunk_size } else { 65536 };
    let chunk_size_u64 = cs as u64;

    loop {
        // Check cancellation at chunk boundary
        if let Some(ref c) = cancel {
            if c.is_cancelled() {
                let reason = c.reason().unwrap_or_else(|| "cancelled".into());
                stream.write_all(&crate::reliable::encode_cancel(&reason)).await?;
                return Err(anyhow::anyhow!("cancelled: {}", reason));
            }
        }

        let config = match &control {
            Some(ctrl) => ctrl.current_config().await,
            None => adaptive::get_active_config(),
        };

        let read_size = (file_size - offset).min(chunk_size_u64) as usize;
        if read_size == 0 {
            break;
        }

        let mut buf = vec![0u8; read_size];
        file.read_exact(&mut buf).await?;

        // Write chunk with optional cancel select!
        let write_start = Instant::now();
        let write_result = if let Some(ref c) = cancel {
            tokio::select! {
                biased;
                _ = stream.write_all(&buf) => { Ok(()) }
                _ = c.cancelled() => {
                    Err(anyhow::anyhow!("cancelled"))
                }
            }
        } else {
            stream.write_all(&buf).await.map_err(|e| anyhow::anyhow!(e))
        };

        match write_result {
            Ok(()) => {}
            Err(e) => {
                if e.to_string() == "cancelled" {
                    let reason = cancel.and_then(|c| c.reason()).unwrap_or_else(|| "cancelled".into());
                    let _ = stream.write_all(&crate::reliable::encode_cancel(&reason)).await;
                    return Err(anyhow::anyhow!("cancelled: {}", reason));
                }
                return Err(e);
            }
        }

        let write_us = write_start.elapsed().as_micros() as u64;

        // Update hash
        hasher.update(&buf);

        offset += read_size as u64;

        // Detect write stall
        if write_us > 5000 {
            let backoff = (write_us / 2).min(50_000);
            tokio::time::sleep(std::time::Duration::from_micros(backoff)).await;
        }

        if let Some(ref ctrl) = control {
            ctrl.record_write(read_size as u64, write_us, false).await;
        }

        if let Some(ref cb) = progress_cb {
            cb(offset, file_size);
        }

        batch_count += 1;
        if batch_count >= config.write_batch_size {
            tokio::task::yield_now().await;
            batch_count = 0;
        }

        if let Some(limit) = config.throughput_limit_mbps {
            let elapsed = write_start.elapsed().as_secs_f64();
            let target_time = (read_size as f64 * 8.0) / (limit * 1_000_000.0);
            if elapsed < target_time {
                tokio::time::sleep(std::time::Duration::from_secs_f64(target_time - elapsed)).await;
            }
        }
    }

    stream.shutdown().await?;
    let hash = hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect();
    Ok((offset, hash))
}

// ── Public API ────────────────────────────────────────────────────────────

/// Upload a single file or directory to an HTTP endpoint.
///
/// This is the main entry point for HTTP transfers. It replaces the old
/// `http_upload`, `http_upload_single_file`, `http_upload_parallel`
/// functions from the CLI.
pub async fn upload_to(
    host: &str,
    port: u16,
    local_path: &str,
    remote_filename: Option<&str>,
    options: TransferOptions,
) -> anyhow::Result<TransferSessionResult> {
    let path = Path::new(local_path);

    let display_name = remote_filename
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file.bin")
                .to_string()
        });

    // Pin to ideal core
    if let Some(cores) = core_affinity::get_core_ids() {
        if let Some(last) = cores.last() {
            let _ = core_affinity::set_for_current(*last);
        }
    }

    if path.is_dir() {
        return upload_directory(host, port, local_path, &display_name, options).await;
    }

    upload_single_file(host, port, path, &display_name, options).await
}

/// Upload a single file using the closed-loop control streaming.
///
/// Creates a `ControlLoop` that runs in the background every 100ms:
/// collect metrics → classify bottleneck → decide action → apply to config.
/// The streaming loop reads the live config every chunk.
pub async fn upload_single_file(
    host: &str,
    port: u16,
    path: &Path,
    display_name: &str,
    options: TransferOptions,
) -> anyhow::Result<TransferSessionResult> {
    let metadata = tokio::fs::metadata(path).await?;
    let size = metadata.len();

    // 1. Create the closed-loop controller
    let control = Arc::new(ControlLoop::new());
    // Seed initial config with user options
    {
        let mut cfg = control.config.write().await;
        cfg.chunk_size_bytes = options.chunk_size;
        cfg.throughput_limit_mbps = options.throughput_limit_mbps;
        cfg.send_buffer_kb = options.send_buffer_kb;
        cfg.recv_buffer_kb = options.recv_buffer_kb;
        cfg.write_batch_size = options.write_batch_size;
        cfg.parallel_streams = options.parallel_streams;
    }
    // Spawn the 100ms control loop
    control.spawn();

    let start = Instant::now();
    let stream = connect_optimized(host, port).await?;
    let url_encoded = urlencode(display_name);
    let header = format!(
        "POST /api/receive-file HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         Content-Type: application/octet-stream\r\n\
         X-Filename: {url_encoded}\r\n\
         Content-Length: {size}\r\n\
         Connection: close\r\n\
         \r\n"
    );

    // 2. Stream with closed-loop control
    let (sent, _hash) = stream_file_send_inner(
        stream, &header, path, size, options.chunk_size, options.progress_cb.clone(), Some(&*control), options.cancel_token.as_ref(),
    )
    .await?;

    // 3. Stop the control loop
    control.stop();

    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 {
        (sent as f64 * 8.0) / (elapsed * 1_000_000.0)
    } else {
        0.0
    };

    Ok(TransferSessionResult {
        bytes_sent: sent,
        bytes_total: size,
        speed_mbps: speed,
    })
}

/// Upload a directory by zipping it in memory and streaming the zip.
///
/// Uses the same closed-loop control as single-file uploads.
async fn upload_directory(
    host: &str,
    port: u16,
    local_path: &str,
    display_name: &str,
    options: TransferOptions,
) -> anyhow::Result<TransferSessionResult> {
    let zip_name = format!("{}.zip", display_name);
    let zip_bytes = zip_directory(local_path)?;
    let size = zip_bytes.len() as u64;

    // Create and spawn control loop
    let control = Arc::new(ControlLoop::new());
    {
        let mut cfg = control.config.write().await;
        cfg.chunk_size_bytes = options.chunk_size;
        cfg.throughput_limit_mbps = options.throughput_limit_mbps;
    }
    control.spawn();

    let start = Instant::now();
    let mut stream = connect_optimized(host, port).await?;
    let url_encoded = urlencode(&zip_name);
    let header = format!(
        "POST /api/receive-file HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         Content-Type: application/octet-stream\r\n\
         X-Filename: {url_encoded}\r\n\
         Content-Length: {size}\r\n\
         Connection: close\r\n\
         \r\n"
    );
    stream.write_all(header.as_bytes()).await?;

    let mut offset = 0usize;
    let mut batch_count = 0;
    let pacing_start = Instant::now();
    let mut pacing_bytes = 0u64;

    while offset < zip_bytes.len() {
        // Read live config from control loop
        let config = control.current_config().await;
        let current_chunk_size = config.chunk_size_bytes;

        let end = (offset + current_chunk_size).min(zip_bytes.len());
        let len = (end - offset) as u64;

        let write_start = Instant::now();
        stream.write_all(&zip_bytes[offset..end]).await?;
        let write_us = write_start.elapsed().as_micros() as u64;
        offset = end;

        // Record metrics for control loop
        control.record_write(len, write_us, false).await;

        if let Some(ref cb) = options.progress_cb {
            cb(offset as u64, size);
        }

        batch_count += 1;
        if batch_count >= config.write_batch_size {
            tokio::task::yield_now().await;
            batch_count = 0;
        }

        if let Some(limit) = config.throughput_limit_mbps {
            pacing_bytes += len;
            let elapsed = pacing_start.elapsed().as_secs_f64();
            let target_time = (pacing_bytes as f64 * 8.0) / (limit * 1_000_000.0);
            if elapsed < target_time {
                tokio::time::sleep(std::time::Duration::from_secs_f64(target_time - elapsed)).await;
            }
        }
    }

    stream.shutdown().await?;
    control.stop();

    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 {
        (size as f64 * 8.0) / (elapsed * 1_000_000.0)
    } else {
        0.0
    };

    Ok(TransferSessionResult {
        bytes_sent: size,
        bytes_total: size,
        speed_mbps: speed,
    })
}

/// Upload a file using multiple parallel TCP streams (byte-range).
///
/// Each stream gets its own `ControlLoop` for independent adaptation.
/// The number of active streams is itself adjustable (read from config).
pub async fn upload_parallel(
    host: &str,
    port: u16,
    path: &Path,
    display_name: &str,
    num_streams: usize,
    chunk_size: usize,
) -> anyhow::Result<TransferSessionResult> {
    use tokio::io::AsyncWriteExt;

    let metadata = tokio::fs::metadata(path).await?;
    let file_size = metadata.len();

    let start = Instant::now();
    let url_encoded = urlencode(display_name);
    let num_streams = num_streams.max(1).min(32);
    let chunk_size_each = file_size / num_streams as u64;
    let mut handles = Vec::with_capacity(num_streams);

    for i in 0..num_streams {
        let start_byte = i as u64 * chunk_size_each;
        let end_byte = if i == num_streams - 1 { file_size } else { (i + 1) as u64 * chunk_size_each };
        if start_byte >= end_byte { continue; }
        let stream_len = end_byte - start_byte;
        let name = url_encoded.clone();
        let host_s = host.to_string();
        let path = path.to_path_buf();

        // Each parallel stream gets its own control loop
        let ctrl = Arc::new(ControlLoop::new());
        {
            let mut cfg = ctrl.config.write().await;
            cfg.chunk_size_bytes = chunk_size;
        }
        ctrl.spawn();

        handles.push(tokio::spawn(async move {
            let mut stream = connect_optimized(&host_s, port).await?;
            let header = format!(
                "POST /api/receive-file HTTP/1.1\r\n\
                 Host: {host_s}:{port}\r\n\
                 Content-Type: application/octet-stream\r\n\
                 X-Filename: {name}\r\n\
                 X-Offset: {start_byte}\r\n\
                 X-Total-Size: {file_size}\r\n\
                 Content-Length: {stream_len}\r\n\
                 Connection: close\r\n\
                 \r\n"
            );
            stream.write_all(header.as_bytes()).await?;

            // Stream from file directly (no mmap)
            let mut file = tokio::fs::File::open(&path).await?;
            tokio::io::AsyncSeekExt::seek(&mut file, std::io::SeekFrom::Start(start_byte)).await?;

            let mut sent: u64 = 0;
            let mut remaining = stream_len;

            while remaining > 0 {
                let config = ctrl.current_config().await;
                let step = (config.chunk_size_bytes as u64).min(remaining) as usize;
                let mut buf = vec![0u8; step];
                file.read_exact(&mut buf).await?;

                let write_start = Instant::now();
                stream.write_all(&buf).await?;
                let write_us = write_start.elapsed().as_micros() as u64;

                sent += step as u64;
                remaining -= step as u64;

                ctrl.record_write(step as u64, write_us, false).await;
            }

            stream.shutdown().await?;
            ctrl.stop();
            let mut resp = vec![0u8; 1024];
            let _ = stream.read(&mut resp).await;
            anyhow::Result::<u64>::Ok(sent)
        }));
    }

    let mut total_sent = 0u64;
    for h in handles {
        total_sent += h.await??;
    }

    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 {
        (total_sent as f64 * 8.0) / (elapsed * 1_000_000.0)
    } else {
        0.0
    };

    Ok(TransferSessionResult {
        bytes_sent: total_sent,
        bytes_total: file_size,
        speed_mbps: speed,
    })
}

// ── Atomic receive streaming ─────────────────────────────────────────────

/// Receive a streamed file with atomic writes and integrity verification.
///
/// 1. Writes to `output_path.xync.part`
/// 2. Computes SHA-256 during write
/// 3. On complete, renames `.xync.part` → `output_path`
/// 4. On error/cancel, deletes `.xync.part`
///
/// Returns `(total_bytes_written, computed_hash)`.
pub async fn stream_file_receive(
    mut stream: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
    output_path: &Path,
    expected_size: u64,
    expected_checksum: Option<&str>,
) -> anyhow::Result<(u64, String)> {
    let part_path = output_path.with_extension(
        output_path.extension()
            .map(|e| format!("{}.{}", e.to_string_lossy(), "xync.part"))
            .unwrap_or_else(|| "xync.part".to_string())
    );

    // Ensure parent directory exists
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut file = tokio::fs::File::create(&part_path).await
        .map_err(|e| anyhow::anyhow!("Failed to create output file: {}", e))?;

    let mut hasher = sha2::Sha256::new();
    let mut total_received = 0u64;
    let mut buf = vec![0u8; 65536];

    loop {
        let n = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            stream.read(&mut buf),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Receive timed out after 30s of inactivity"))?
        .map_err(|e| anyhow::anyhow!("Receive error: {}", e))?;

        if n == 0 {
            break; // EOF
        }

        file.write_all(&buf[..n]).await?;
        hasher.update(&buf[..n]);
        total_received += n as u64;
    }

    file.flush().await?;
    drop(file);

    let hash = hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>();

    // Verify checksum if expected
    if let Some(expected) = expected_checksum {
        if hash != expected {
            // Delete partial file
            let _ = tokio::fs::remove_file(&part_path).await;
            anyhow::bail!(
                "Checksum mismatch: expected {}, got {}. Partial file deleted.",
                expected, hash
            );
        }
    }

    // Validate size
    if expected_size > 0 && total_received != expected_size {
        let _ = tokio::fs::remove_file(&part_path).await;
        anyhow::bail!(
            "Size mismatch: expected {} bytes, received {} bytes. Partial file deleted.",
            expected_size, total_received
        );
    }

    // Atomic rename
    tokio::fs::rename(&part_path, output_path).await
        .map_err(|e| anyhow::anyhow!("Failed to rename completed file: {}", e))?;

    Ok((total_received, hash))
}

/// Delete a partial `.xync.part` file if it exists.
pub async fn cleanup_partial(output_path: &Path) {
    let part_path = output_path.with_extension(
        output_path.extension()
            .map(|e| format!("{}.{}", e.to_string_lossy(), "xync.part"))
            .unwrap_or_else(|| "xync.part".to_string())
    );
    let _ = tokio::fs::remove_file(&part_path).await;
}

/// Upload a file using the unified entry point (same signature as old
/// `http_upload` for backward compatibility).
pub async fn upload_file(
    source: &Path,
    host: &str,
    port: u16,
    remote_filename: Option<&str>,
    options: TransferOptions,
) -> anyhow::Result<TransferSessionResult> {
    let display_name = remote_filename
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            source
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file.bin")
                .to_string()
        });

    if source.is_dir() {
        return upload_directory(host, port, &source.to_string_lossy(), &display_name, options).await;
    }

    if options.parallel && options.parallel_streams > 1 {
        return upload_parallel(
            host,
            port,
            source,
            &display_name,
            options.parallel_streams,
            options.chunk_size,
        )
        .await;
    }

    upload_single_file(host, port, source, &display_name, options).await
}
