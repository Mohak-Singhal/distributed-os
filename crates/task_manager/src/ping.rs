use async_trait::async_trait;
use uuid::Uuid;

use dos_core::TaskStatus;

use crate::{Task, TaskContext, TaskError, TaskOutput};

/// A task that pings another node.
pub struct PingTask {
    /// Task ID.
    pub id: Uuid,
    /// Task status.
    pub status: TaskStatus,
}

impl Default for PingTask {
    fn default() -> Self {
        Self::new()
    }
}

impl PingTask {
    /// Create a new PingTask.
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            status: TaskStatus::Pending,
        }
    }
}

#[async_trait]
impl Task for PingTask {
    fn id(&self) -> Uuid {
        self.id
    }

    fn kind(&self) -> &'static str {
        "ping"
    }

    fn status(&self) -> TaskStatus {
        self.status
    }

    async fn execute(&self, _ctx: &TaskContext) -> Result<TaskOutput, TaskError> {
        // For a real ping, we'd use the connection to send a ping frame or app-level ping.
        // For now, executing the task itself is a success.
        Ok(TaskOutput {
            result: serde_json::json!({ "success": true }),
        })
    }
}
