use super::profile::TransferProfile;

pub struct StrategyExecutor;

impl StrategyExecutor {
    pub async fn execute_single_stream(
        host: &str, port: u16, src: &str, dst: &str,
        profile: &TransferProfile,
        cb: Option<std::sync::Arc<dyn Fn(u64, u64) + Send + Sync>>,
    ) -> anyhow::Result<crate::telemetry::TransferSession> {
        crate::http_transfer::http_upload_with_chunk_size(host, port, src, Some(dst), profile.chunk_size, cb).await
    }

    pub async fn execute_parallel_ranges(
        host: &str, port: u16, remote_path: &str, output_path: &str,
        _file_size: u64, _num_streams: u32, profile: &TransferProfile,
        cb: Option<std::sync::Arc<dyn Fn(u64, u64) + Send + Sync>>,
    ) -> anyhow::Result<crate::telemetry::TransferSession> {
        crate::http_transfer::http_download_with_chunk_size(host, port, remote_path, output_path, profile.chunk_size, cb).await
    }

    pub async fn execute_batched(
        host: &str, port: u16, paths: &[String], dst_base: &str,
        _num_workers: u32,
    ) -> anyhow::Result<()> {
        for path in paths {
            let fname = std::path::Path::new(path)
                .file_name().and_then(|n| n.to_str()).unwrap_or("file");
            let remote = format!("{}/{}", dst_base.trim_end_matches('/'), fname);
            crate::http_transfer::http_upload(host, port, path, Some(&remote), None).await?;
        }
        Ok(())
    }

    pub async fn execute_streaming_directory(
        host: &str,
        port: u16,
        dir_path: &std::path::Path,
        display_name: &str,
        profile: &TransferProfile,
        files: &[crate::transfer_engine::file_analyzer::FileEntry],
        total_size: u64,
        cb: Option<std::sync::Arc<dyn Fn(u64, u64) + Send + Sync>>,
    ) -> anyhow::Result<crate::telemetry::TransferSession> {
        crate::http_transfer::stream_directory_send(
            host, port, dir_path, display_name,
            profile.chunk_size, files, total_size, cb,
        ).await
    }
}
