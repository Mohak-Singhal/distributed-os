use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use uuid::Uuid;

use dos_core::TaskStatus;
use dos_networking::Connection;
use dos_protocol::{builder::task_request, ids::NodeId, Message};
use dos_task_manager::{Task, TaskContext, TaskDispatcher, TaskError, TaskOutput, TaskQueue};

pub struct ClientClipboardTask {
    id: Uuid,
    target: NodeId,
    cli_id: NodeId,
    action: String,
    content: Option<String>,
    conn: dos_networking::WsConnection,
}

impl ClientClipboardTask {
    pub fn new(
        target: NodeId,
        cli_id: NodeId,
        action: &str,
        content: Option<String>,
        conn: dos_networking::WsConnection,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            target,
            cli_id,
            action: action.to_string(),
            content,
            conn,
        }
    }
}

#[async_trait]
impl Task for ClientClipboardTask {
    fn id(&self) -> Uuid {
        self.id
    }

    fn kind(&self) -> &'static str {
        "clipboard"
    }

    fn status(&self) -> TaskStatus {
        TaskStatus::Pending
    }

    async fn execute(&self, _ctx: &TaskContext) -> Result<TaskOutput, TaskError> {
        let payload = if self.action == "get" {
            json!({ "action": "get" })
        } else {
            json!({
                "action": "set",
                "content": self.content.as_deref().unwrap_or("")
            })
        };

        let req = task_request(
            self.cli_id,
            Some(self.target),
            "clipboard".to_string(),
            payload,
        );

        self.conn
            .send(&req)
            .await
            .map_err(|e| TaskError::ExecutionFailed(e.to_string()))?;

        while let Ok(Some(msg)) = self.conn.recv().await {
            match msg {
                Message::TaskResult(res) => {
                    return Ok(TaskOutput { result: res.result });
                }
                Message::Error { code, message } => {
                    return Err(TaskError::ExecutionFailed(format!("{}: {}", code, message)));
                }
                _ => {}
            }
        }
        Err(TaskError::ExecutionFailed("Connection closed".into()))
    }
}

pub async fn run_clipboard_get_raw(target_id: Uuid) -> anyhow::Result<String> {
    let node_id = NodeId(target_id);
    let (conn, cli_id) = crate::net::connect_and_identify().await?;

    let (res_tx, mut res_rx) = tokio::sync::mpsc::unbounded_channel();
    let (task_queue, task_rx) = TaskQueue::new(10);
    let context = TaskContext {
        node_id: cli_id.0,
        origin: None,
        result_tx: Some(res_tx),
    };

    let dispatcher = TaskDispatcher::new(task_rx, context);
    tokio::spawn(dispatcher.run());

    let task = Arc::new(ClientClipboardTask::new(node_id, cli_id, "get", None, conn));
    task_queue.submit(task, None).await?;

    if let Some((_id, _origin, result)) = res_rx.recv().await {
        match result {
            Ok(output) => {
                if let Some(content) = output.result.get("content").and_then(|v| v.as_str()) {
                    return Ok(content.to_string());
                } else if let Some(err) = output.result.get("error").and_then(|v| v.as_str()) {
                    return Err(anyhow::anyhow!("Clipboard error: {}", err));
                } else {
                    return Err(anyhow::anyhow!("Invalid response: {:?}", output.result));
                }
            }
            Err(e) => return Err(anyhow::anyhow!("Error: {}", e)),
        }
    }
    Err(anyhow::anyhow!("No response from clipboard"))
}

pub async fn run_clipboard_get(target_id: Uuid) -> anyhow::Result<()> {
    match run_clipboard_get_raw(target_id).await {
        Ok(content) => println!("{}", content),
        Err(e) => eprintln!("Error: {}", e),
    }
    Ok(())
}

pub async fn run_clipboard_set_raw(target_id: Uuid, text: &str) -> anyhow::Result<()> {
    let node_id = NodeId(target_id);
    let (conn, cli_id) = crate::net::connect_and_identify().await?;

    let (res_tx, mut res_rx) = tokio::sync::mpsc::unbounded_channel();
    let (task_queue, task_rx) = TaskQueue::new(10);
    let context = TaskContext {
        node_id: cli_id.0,
        origin: None,
        result_tx: Some(res_tx),
    };

    let dispatcher = TaskDispatcher::new(task_rx, context);
    tokio::spawn(dispatcher.run());

    let task = Arc::new(ClientClipboardTask::new(
        node_id,
        cli_id,
        "set",
        Some(text.to_string()),
        conn,
    ));
    task_queue.submit(task, None).await?;

    if let Some((_id, _origin, result)) = res_rx.recv().await {
        match result {
            Ok(output) => {
                if output.result.get("success").and_then(|v| v.as_bool()) == Some(true) {
                    return Ok(());
                } else if let Some(err) = output.result.get("error").and_then(|v| v.as_str()) {
                    return Err(anyhow::anyhow!("Clipboard error: {}", err));
                } else {
                    return Err(anyhow::anyhow!("Invalid response: {:?}", output.result));
                }
            }
            Err(e) => return Err(anyhow::anyhow!("Error: {}", e)),
        }
    }
    Err(anyhow::anyhow!("No response from clipboard"))
}

pub async fn run_clipboard_set(target_id: Uuid, text: &str) -> anyhow::Result<()> {
    let node_id = NodeId(target_id);
    println!("Setting clipboard on {}...", node_id);
    match run_clipboard_set_raw(target_id, text).await {
        Ok(()) => println!("Clipboard set successfully!"),
        Err(e) => eprintln!("Error: {}", e),
    }
    Ok(())
}
