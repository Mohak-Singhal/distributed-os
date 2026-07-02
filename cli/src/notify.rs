use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use uuid::Uuid;

use dos_core::TaskStatus;
use dos_networking::Connection;
use dos_protocol::{builder::task_request, ids::NodeId, Message};
use dos_task_manager::{Task, TaskContext, TaskDispatcher, TaskError, TaskOutput, TaskQueue};

pub struct ClientNotifyTask {
    id: Uuid,
    target: NodeId,
    cli_id: NodeId,
    title: String,
    body: String,
    conn: dos_networking::WsConnection,
}

impl ClientNotifyTask {
    pub fn new(
        target: NodeId,
        cli_id: NodeId,
        title: &str,
        body: &str,
        conn: dos_networking::WsConnection,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            target,
            cli_id,
            title: title.to_string(),
            body: body.to_string(),
            conn,
        }
    }
}

#[async_trait]
impl Task for ClientNotifyTask {
    fn id(&self) -> Uuid {
        self.id
    }

    fn kind(&self) -> &'static str {
        "notifications"
    }

    fn status(&self) -> TaskStatus {
        TaskStatus::Pending
    }

    async fn execute(&self, _ctx: &TaskContext) -> Result<TaskOutput, TaskError> {
        let payload = json!({
            "title": self.title,
            "body": self.body
        });

        let req = task_request(
            self.cli_id,
            Some(self.target),
            "notifications".to_string(),
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

pub async fn run_notify_raw(target_id: Uuid, title: &str, body: &str) -> anyhow::Result<()> {
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

    let task = Arc::new(ClientNotifyTask::new(node_id, cli_id, title, body, conn));
    task_queue.submit(task, None).await?;

    if let Some((_id, _origin, result)) = res_rx.recv().await {
        match result {
            Ok(output) => {
                if output.result.get("success").and_then(|v| v.as_bool()) == Some(true) {
                    return Ok(());
                } else if let Some(err) = output.result.get("error").and_then(|v| v.as_str()) {
                    return Err(anyhow::anyhow!("Notification error: {}", err));
                } else {
                    return Err(anyhow::anyhow!("Invalid response: {:?}", output.result));
                }
            }
            Err(e) => return Err(anyhow::anyhow!("Error: {}", e)),
        }
    }
    Err(anyhow::anyhow!("No response from notify"))
}

pub async fn run_notify(target_id: Uuid, title: &str, body: &str) -> anyhow::Result<()> {
    let node_id = NodeId(target_id);
    println!("Sending notification to {}...", node_id);
    match run_notify_raw(target_id, title, body).await {
        Ok(()) => println!("Notification sent successfully!"),
        Err(e) => eprintln!("Error: {}", e),
    }
    Ok(())
}
