use std::sync::Arc;
use async_trait::async_trait;
use serde_json::json;
use uuid::Uuid;

use dos_core::TaskStatus;
use dos_protocol::message::TaskRequest;

use crate::providers::notifications::NotificationsProvider;
use crate::{Task, TaskContext, TaskError, TaskOutput};

/// Task that triggers a system notification.
pub struct NotificationsTask {
    request: TaskRequest,
    provider: Arc<dyn NotificationsProvider>,
}

impl NotificationsTask {
    /// Create a new notifications task.
    pub fn new(request: &TaskRequest, provider: Arc<dyn NotificationsProvider>) -> Result<Self, TaskError> {
        Ok(Self {
            request: request.clone(),
            provider,
        })
    }
}

#[async_trait]
impl Task for NotificationsTask {
    fn id(&self) -> Uuid {
        self.request.task_id.0
    }

    fn kind(&self) -> &'static str {
        "notifications"
    }

    fn status(&self) -> TaskStatus {
        TaskStatus::Pending
    }

    async fn execute(&self, _ctx: &TaskContext) -> Result<TaskOutput, TaskError> {
        let title = self.request.payload.get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("PDOS Notification");
            
        let body = self.request.payload.get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match self.provider.show(title, body).await {
            Ok(_) => Ok(TaskOutput {
                result: json!({ "success": true }),
            }),
            Err(e) => Err(TaskError::ExecutionFailed(format!("Notification failed: {}", e))),
        }
    }
}
