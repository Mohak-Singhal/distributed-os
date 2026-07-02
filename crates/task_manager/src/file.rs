use std::sync::Arc;
use base64::prelude::*;
use async_trait::async_trait;
use serde_json::json;
use uuid::Uuid;

use dos_core::TaskStatus;
use dos_protocol::message::TaskRequest;

use crate::providers::file::FileProvider;
use crate::{Task, TaskContext, TaskError, TaskOutput};

/// Task that reads or writes files on the node.
pub struct FileTask {
    request: TaskRequest,
    provider: Arc<dyn FileProvider>,
}

impl FileTask {
    /// Create a new file task.
    pub fn new(request: &TaskRequest, provider: Arc<dyn FileProvider>) -> Result<Self, TaskError> {
        Ok(Self {
            request: request.clone(),
            provider,
        })
    }
}

#[async_trait]
impl Task for FileTask {
    fn id(&self) -> Uuid {
        self.request.task_id.0
    }

    fn kind(&self) -> &'static str {
        "file_transfer"
    }

    fn status(&self) -> TaskStatus {
        TaskStatus::Pending
    }

    async fn execute(&self, _ctx: &TaskContext) -> Result<TaskOutput, TaskError> {
        let op = self.request.payload.get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TaskError::InvalidRequest("Missing 'op' field".into()))?;
            
        let path = self.request.payload.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TaskError::InvalidRequest("Missing 'path' field".into()))?;

        match op {
            "read" => {
                match self.provider.read(path).await {
                    Ok(data) => {
                        let content = BASE64_STANDARD.encode(&data);
                        Ok(TaskOutput {
                            result: json!({
                                "success": true,
                                "content": content
                            }),
                        })
                    }
                    Err(e) => Err(TaskError::ExecutionFailed(format!("Read failed: {}", e))),
                }
            }
            "write" => {
                let content_str = self.request.payload.get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| TaskError::InvalidRequest("Missing 'content' field for write".into()))?;
                    
                let mut data = BASE64_STANDARD.decode(content_str)
                    .map_err(|e| TaskError::InvalidRequest(format!("Invalid base64 content: {}", e)))?;
                    
                if self.request.payload.get("compressed").and_then(|v| v.as_bool()).unwrap_or(false) {
                    use flate2::read::GzDecoder;
                    use std::io::Read;
                    let mut decoder = GzDecoder::new(&data[..]);
                    let mut decompressed = Vec::new();
                    if decoder.read_to_end(&mut decompressed).is_ok() {
                        data = decompressed;
                    }
                }

                match self.provider.write(path, &data).await {
                    Ok(_) => Ok(TaskOutput {
                        result: json!({ "success": true }),
                    }),
                    Err(e) => Err(TaskError::ExecutionFailed(format!("Write failed: {}", e))),
                }
            }
            _ => Err(TaskError::InvalidRequest(format!("Unknown op: {}", op))),
        }
    }
}
