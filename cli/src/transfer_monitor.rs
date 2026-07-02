use std::sync::Arc;
use tokio::sync::RwLock;

/// Get transfer engine status
pub async fn get_transfer_status(transfer_id: &str) -> Result<TransferStatusInfo, Box<dyn std::error::Error>> {
    // This would integrate with the transfer engine
    // For now, return dummy data
    Ok(TransferStatusInfo {
        id: transfer_id.to_string(),
        state: TransferState::Running,
        progress: TransferProgressInfo {
            bytes_transferred: 0,
            total_bytes: 100_000_000,
            speed_mbps: 50.0,
        },
    })
}

#[derive(Debug, Clone)]
pub struct TransferStatusInfo {
    pub id: String,
    pub state: TransferState,
    pub progress: TransferProgressInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferState {
    Idle,
    Running,
    Paused,
    Completed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct TransferProgressInfo {
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub speed_mbps: f64,
}
