use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use uuid::Uuid;

use dos_core::TaskStatus;
use dos_networking::Connection;
use dos_protocol::{builder::task_request, ids::NodeId, Message};
use dos_task_manager::{Task, TaskContext, TaskDispatcher, TaskError, TaskOutput, TaskQueue};

pub struct ClientPingTask {
    id: Uuid,
    target: NodeId,
    cli_id: NodeId,
    conn: dos_networking::WsConnection,
}

impl ClientPingTask {
    pub fn new(target: NodeId, cli_id: NodeId, conn: dos_networking::WsConnection) -> Self {
        Self {
            id: Uuid::new_v4(),
            target,
            cli_id,
            conn,
        }
    }
}

#[async_trait]
impl Task for ClientPingTask {
    fn id(&self) -> Uuid {
        self.id
    }

    fn kind(&self) -> &'static str {
        "ping"
    }

    fn status(&self) -> TaskStatus {
        TaskStatus::Pending
    }

    async fn execute(&self, _ctx: &TaskContext) -> Result<TaskOutput, TaskError> {
        let payload = serde_json::json!({ "type": "ping" });
        let req = task_request(self.cli_id, Some(self.target), "ping".to_string(), payload);

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

pub async fn run_ping(target_id: &str) -> anyhow::Result<()> {
    let node_id = NodeId(Uuid::parse_str(target_id)?);

    println!("Pinging {}...", node_id);
    let (conn, cli_id) = crate::net::connect_and_identify().await?;

    // Setup TaskManager architecture for the CLI
    let (res_tx, mut res_rx) = tokio::sync::mpsc::unbounded_channel();
    let (task_queue, task_rx) = TaskQueue::new(10);
    let context = TaskContext {
        node_id: cli_id.0,
        origin: None,
        result_tx: Some(res_tx),
    };
    
    let dispatcher = TaskDispatcher::new(task_rx, context);
    tokio::spawn(dispatcher.run());

    let task = Arc::new(ClientPingTask::new(node_id, cli_id, conn));
    
    let start = Instant::now();
    task_queue.submit(task, None).await?;

    // Wait for the dispatcher to return the result
    if let Some((_id, _origin, result)) = res_rx.recv().await {
        let duration = start.elapsed();
        match result {
            Ok(output) => {
                println!("Reply from {}: time={:?} result={}", node_id, duration, output.result);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }

    Ok(())
}

