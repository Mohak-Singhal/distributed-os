use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    TransferRequest, TransferHandle, TransferProgress, TransferStatus, CancelToken,
    RemoteTarget, TransferDirection,
    http,
};

/// The single stream engine — dispatches every transfer request.
pub struct StreamEngine {
    active: Arc<Mutex<HashMap<String, TransferHandle>>>,
}

impl StreamEngine {
    pub fn new() -> Self {
        Self {
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn execute(&mut self, req: TransferRequest) -> anyhow::Result<TransferHandle> {
        let id = uuid::Uuid::new_v4().to_string();
        let total = req.sources.iter().filter_map(|p| file_size(p)).sum();
        let total_files = req.sources.len();

        let handle = TransferHandle {
            id: id.clone(),
            state: TransferStatus::Running,
            progress: TransferProgress {
                bytes_sent: 0,
                bytes_total: total,
                speed_mbps: 0.0,
                files_completed: 0,
                files_total: total_files,
            },
        };

        {
            let mut map = self.active.lock().await;
            map.insert(id.clone(), handle.clone());
        }

        drop(handle);

        // Dispatch to the correct transport — iterate over all sources
        let total_sources = req.sources.len();
        let mut total_bytes_sent = 0u64;
        let mut total_bytes_all = 0u64;
        let mut files_completed = 0usize;
        let mut last_error = None;

        for source in &req.sources {
            let file_len = file_size(source).unwrap_or(0);
            total_bytes_all += file_len;

            let result = match &req.destination {
                RemoteTarget::Http { host, port, .. } => {
                    match req.direction {
                        TransferDirection::Upload => {
                            http::upload_file(source, host, *port, None, req.options.clone()).await
                        }
                        TransferDirection::Download => {
                            Err(anyhow::anyhow!("Download via HTTP not yet implemented in engine"))
                        }
                    }
                }
                RemoteTarget::Tcp { host, port, .. } => {
                    match req.direction {
                        TransferDirection::Upload => {
                            http::upload_file(source, host, *port, None, req.options.clone()).await
                        }
                        TransferDirection::Download => {
                            Err(anyhow::anyhow!("Download via TCP not yet implemented in engine"))
                        }
                    }
                }
                RemoteTarget::Udp { .. } | RemoteTarget::Quic { .. } => {
                    Err(anyhow::anyhow!("UDP/QUIC transport not yet integrated into engine"))
                }
                RemoteTarget::Local { .. } => {
                    Err(anyhow::anyhow!("Local transfers not supported via streaming engine"))
                }
            };

            match result {
                Ok(session) => {
                    total_bytes_sent += session.bytes_sent;
                    files_completed += 1;
                    if let Some(h) = self.active.lock().await.get_mut(&id) {
                        h.progress.bytes_sent = total_bytes_sent;
                        h.progress.speed_mbps = session.speed_mbps;
                    }
                }
                Err(e) => {
                    last_error = Some(e);
                    break;
                }
            }
        }

        let mut map = self.active.lock().await;
        if let Some(last_error) = last_error {
            if let Some(h) = map.get_mut(&id) {
                h.state = TransferStatus::Failed(last_error.to_string());
            }
            return Err(last_error);
        }

        if let Some(h) = map.get_mut(&id) {
            h.state = TransferStatus::Completed;
        }
        Ok(TransferHandle {
            id,
            state: TransferStatus::Completed,
            progress: TransferProgress {
                bytes_sent: total_bytes_sent,
                bytes_total: total_bytes_all,
                speed_mbps: 0.0,
                files_completed,
                files_total: total_sources,
            },
        })
    }

    /// Execute a transfer with cancellation support.
    ///
    /// Checks the `CancelToken` before starting. The actual upload
    /// functions also check the token internally at chunk boundaries.
    pub async fn execute_with_cancel(
        &mut self,
        req: TransferRequest,
        cancel: CancelToken,
    ) -> anyhow::Result<TransferHandle> {
        if cancel.is_cancelled() {
            return Err(anyhow::anyhow!("cancelled: {}", cancel.reason().unwrap_or_default()));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let total: u64 = req.sources.iter().filter_map(|p| file_size(p)).sum();
        let total_files = req.sources.len();

        let handle = TransferHandle {
            id: id.clone(),
            state: TransferStatus::Running,
            progress: TransferProgress {
                bytes_sent: 0,
                bytes_total: total,
                speed_mbps: 0.0,
                files_completed: 0,
                files_total: total_files,
            },
        };

        {
            let mut map = self.active.lock().await;
            map.insert(id.clone(), handle.clone());
        }

        drop(handle);

        let mut opts = req.options;
        opts.cancel_token = Some(cancel);

        let total_sources = req.sources.len();
        let mut total_bytes_sent = 0u64;
        let mut total_bytes_all = 0u64;
        let mut files_completed = 0usize;
        let mut last_error = None;

        for source in &req.sources {
            let file_len = file_size(source).unwrap_or(0);
            total_bytes_all += file_len;

            let result = match &req.destination {
                RemoteTarget::Http { host, port, .. } => {
                    match req.direction {
                        TransferDirection::Upload => {
                            http::upload_file(source, host, *port, None, opts.clone()).await
                        }
                        TransferDirection::Download => {
                            Err(anyhow::anyhow!("Download via HTTP not yet implemented in engine"))
                        }
                    }
                }
                RemoteTarget::Tcp { host, port, .. } => {
                    match req.direction {
                        TransferDirection::Upload => {
                            http::upload_file(source, host, *port, None, opts.clone()).await
                        }
                        TransferDirection::Download => {
                            Err(anyhow::anyhow!("Download via TCP not yet implemented in engine"))
                        }
                    }
                }
                RemoteTarget::Udp { .. } | RemoteTarget::Quic { .. } => {
                    Err(anyhow::anyhow!("UDP/QUIC transport not yet integrated into engine"))
                }
                RemoteTarget::Local { .. } => {
                    Err(anyhow::anyhow!("Local transfers not supported via streaming engine"))
                }
            };

            match result {
                Ok(session) => {
                    total_bytes_sent += session.bytes_sent;
                    files_completed += 1;
                    if let Some(h) = self.active.lock().await.get_mut(&id) {
                        h.progress.bytes_sent = total_bytes_sent;
                        h.progress.speed_mbps = session.speed_mbps;
                    }
                }
                Err(e) => {
                    last_error = Some(e);
                    break;
                }
            }
        }

        let mut map = self.active.lock().await;
        if let Some(last_error) = last_error {
            if let Some(h) = map.get_mut(&id) {
                h.state = TransferStatus::Failed(last_error.to_string());
            }
            return Err(last_error);
        }

        if let Some(h) = map.get_mut(&id) {
            h.state = TransferStatus::Completed;
        }
        Ok(TransferHandle {
            id,
            state: TransferStatus::Completed,
            progress: TransferProgress {
                bytes_sent: total_bytes_sent,
                bytes_total: total_bytes_all,
                speed_mbps: 0.0,
                files_completed,
                files_total: total_sources,
            },
        })
    }

    pub async fn cancel(&mut self, id: &str) -> anyhow::Result<()> {
        let mut map = self.active.lock().await;
        map.remove(id);
        Ok(())
    }

    pub async fn status(&self, id: &str) -> anyhow::Result<TransferProgress> {
        let map = self.active.lock().await;
        map.get(id)
            .map(|h| h.progress.clone())
            .ok_or_else(|| anyhow::anyhow!("Transfer not found: {}", id))
    }
}

fn file_size(path: &Path) -> Option<u64> {
    if path.is_file() {
        std::fs::metadata(path).ok().map(|m| m.len())
    } else if path.is_dir() {
        Some(dir_total_size(path))
    } else {
        None
    }
}

fn dir_total_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                total += std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            } else if path.is_dir() {
                total += dir_total_size(&path);
            }
        }
    }
    total
}

/// Result returned by transport implementations
pub struct TransferSessionResult {
    pub bytes_sent: u64,
    pub bytes_total: u64,
    pub speed_mbps: f64,
}
