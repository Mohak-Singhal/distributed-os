use std::sync::Arc;

use async_trait::async_trait;
use dos_core::TaskStatus;
use dos_protocol::message::TaskRequest;
use serde::Deserialize;
use uuid::Uuid;

use crate::providers::clipboard::ClipboardProvider;
use crate::{Task, TaskContext, TaskError, TaskOutput};

#[derive(Deserialize)]
#[serde(tag = "action")]
enum ClipboardAction {
    #[serde(rename = "get")]
    Get,
    #[serde(rename = "set")]
    Set { content: String },
}

/// A task that interacts with the system clipboard.
pub struct ClipboardTask {
    id: Uuid,
    status: TaskStatus,
    action: ClipboardAction,
    provider: Arc<dyn ClipboardProvider>,
}

impl ClipboardTask {
    /// Create a new ClipboardTask from a network request.
    pub fn new(req: &TaskRequest, provider: Arc<dyn ClipboardProvider>) -> Result<Self, TaskError> {
        let action: ClipboardAction = serde_json::from_value(req.payload.clone())
            .map_err(|e| TaskError::InvalidRequest(format!("Invalid clipboard payload: {}", e)))?;

        Ok(Self {
            id: req.task_id.0,
            status: TaskStatus::Pending,
            action,
            provider,
        })
    }
}

#[async_trait]
impl Task for ClipboardTask {
    fn id(&self) -> Uuid {
        self.id
    }

    fn kind(&self) -> &'static str {
        "clipboard"
    }

    fn status(&self) -> TaskStatus {
        self.status
    }

    async fn execute(&self, _ctx: &TaskContext) -> Result<TaskOutput, TaskError> {
        match &self.action {
            ClipboardAction::Get => {
                let text = self.provider.get_text().await.map_err(|e| {
                    TaskError::ExecutionFailed(format!("Failed to get clipboard text: {}", e))
                })?;
                Ok(TaskOutput {
                    result: serde_json::json!({ "content": text }),
                })
            }
            ClipboardAction::Set { content } => {
                self.provider.set_text(content).await.map_err(|e| {
                    TaskError::ExecutionFailed(format!("Failed to set clipboard text: {}", e))
                })?;
                Ok(TaskOutput {
                    result: serde_json::json!({ "success": true }),
                })
            }
        }
    }
}
