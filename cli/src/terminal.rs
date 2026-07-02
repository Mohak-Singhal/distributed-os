use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use uuid::Uuid;

use dos_core::TaskStatus;
use dos_networking::Connection;
use dos_protocol::{builder::task_request, ids::NodeId, Message};
use dos_task_manager::{Task, TaskContext, TaskDispatcher, TaskError, TaskOutput, TaskQueue};

pub struct ClientTerminalTask {
    id: Uuid,
    target: NodeId,
    cli_id: NodeId,
    command: String,
    args: Vec<String>,
    conn: dos_networking::WsConnection,
}

impl ClientTerminalTask {
    pub fn new(
        target: NodeId,
        cli_id: NodeId,
        command: &str,
        args: &[String],
        conn: dos_networking::WsConnection,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            target,
            cli_id,
            command: command.to_string(),
            args: args.to_vec(),
            conn,
        }
    }
}

#[async_trait]
impl Task for ClientTerminalTask {
    fn id(&self) -> Uuid {
        self.id
    }

    fn kind(&self) -> &'static str {
        "terminal"
    }

    fn status(&self) -> TaskStatus {
        TaskStatus::Pending
    }

    async fn execute(&self, _ctx: &TaskContext) -> Result<TaskOutput, TaskError> {
        let payload = json!({
            "command": self.command,
            "args": self.args
        });

        let req = task_request(
            self.cli_id,
            Some(self.target),
            "terminal".to_string(),
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

pub async fn run_terminal_raw(
    target_id: Uuid,
    command: &str,
    args: &[String],
) -> anyhow::Result<String> {
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

    let task = Arc::new(ClientTerminalTask::new(
        node_id, cli_id, command, args, conn,
    ));
    task_queue.submit(task, None).await?;

    if let Some((_id, _origin, result)) = res_rx.recv().await {
        match result {
            Ok(output) => {
                if output.result.get("success").and_then(|v| v.as_bool()) == Some(true) {
                    if let Some(out) = output.result.get("output").and_then(|v| v.as_str()) {
                        return Ok(out.to_string());
                    }
                    return Ok(String::new());
                } else if let Some(err) = output.result.get("error").and_then(|v| v.as_str()) {
                    return Err(anyhow::anyhow!("Terminal error: {}", err));
                } else {
                    return Err(anyhow::anyhow!("Invalid response: {:?}", output.result));
                }
            }
            Err(e) => return Err(anyhow::anyhow!("Error: {}", e)),
        }
    }
    Err(anyhow::anyhow!("No response from terminal"))
}

pub async fn run_terminal(target_id: Uuid, command: &str, args: &[String]) -> anyhow::Result<()> {
    match run_terminal_raw(target_id, command, args).await {
        Ok(output) => println!("{}", output),
        Err(e) => eprintln!("Error: {}", e),
    }
    Ok(())
}
