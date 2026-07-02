use std::sync::Arc;
use async_trait::async_trait;
use serde_json::json;
use uuid::Uuid;

use dos_core::TaskStatus;
use dos_protocol::message::TaskRequest;

use crate::providers::terminal::TerminalProvider;
use crate::{Task, TaskContext, TaskError, TaskOutput};

/// Task that executes a terminal command.
pub struct TerminalTask {
    request: TaskRequest,
    provider: Arc<dyn TerminalProvider>,
}

impl TerminalTask {
    /// Create a new terminal task.
    pub fn new(request: &TaskRequest, provider: Arc<dyn TerminalProvider>) -> Result<Self, TaskError> {
        Ok(Self {
            request: request.clone(),
            provider,
        })
    }
}

#[async_trait]
impl Task for TerminalTask {
    fn id(&self) -> Uuid {
        self.request.task_id.0
    }

    fn kind(&self) -> &'static str {
        "terminal"
    }

    fn status(&self) -> TaskStatus {
        TaskStatus::Pending
    }

    async fn execute(&self, _ctx: &TaskContext) -> Result<TaskOutput, TaskError> {
        let command = self.request.payload.get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TaskError::InvalidRequest("Missing 'command' field".into()))?;
            
        let args: Vec<String> = self.request.payload.get("args")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        match self.provider.execute(command, &args).await {
            Ok(output) => Ok(TaskOutput {
                result: json!({
                    "success": true,
                    "output": output
                }),
            }),
            Err(e) => Err(TaskError::ExecutionFailed(format!("Terminal execution failed: {}", e))),
        }
    }
}
