// Transfer Engine Adapter — wraps `transfer_engine` crate for CLI use.

use transfer_engine::{
    TransferRequest, RemoteTarget, TransferDirection, TransferOptions, TransferEngine, TransferCoordinator,
};
use std::path::PathBuf;

/// Start a CLI upload using the unified transfer engine.
/// This is the NEW preferred entry point for CLI transfers.
pub async fn run_cli_upload(
    host: &str,
    port: u16,
    local_path: &str,
    remote_filename: Option<&str>,
) -> anyhow::Result<()> {
    let source = PathBuf::from(local_path);

    let request = TransferRequest {
        sources: vec![source],
        destination: RemoteTarget::Http {
            host: host.to_string(),
            port,
            path: remote_filename.map(|f| f.to_string()),
        },
        direction: TransferDirection::Upload,
        options: TransferOptions::default(),
    };

    let mut engine = TransferCoordinator::new();
    let handle = engine.start(request).await?;

    match &handle.state {
        transfer_engine::TransferStatus::Completed => {
            let p = &handle.progress;
            println!("Upload complete: {} bytes, {:.2} Mbps avg",
                p.bytes_sent, p.speed_mbps);
            Ok(())
        }
        transfer_engine::TransferStatus::Failed(e) => {
            Err(anyhow::anyhow!("Upload failed: {}", e))
        }
        _ => {
            Err(anyhow::anyhow!("Upload did not complete"))
        }
    }
}
