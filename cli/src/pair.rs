use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

use dos_core::TaskStatus;
use dos_networking::Connection;
use dos_protocol::{builder::pair_request, ids::NodeId, Message, PairCode};
use dos_task_manager::{Task, TaskContext, TaskDispatcher, TaskError, TaskOutput, TaskQueue};

pub struct ClientPairTask {
    id: Uuid,
    target: NodeId,
    cli_id: NodeId,
    pair_code: String,
    conn: dos_networking::WsConnection,
}

impl ClientPairTask {
    pub fn new(target: NodeId, cli_id: NodeId, pair_code: String, conn: dos_networking::WsConnection) -> Self {
        Self {
            id: Uuid::new_v4(),
            target,
            cli_id,
            pair_code,
            conn,
        }
    }
}

#[async_trait]
impl Task for ClientPairTask {
    fn id(&self) -> Uuid {
        self.id
    }

    fn kind(&self) -> &'static str {
        "pair"
    }

    fn status(&self) -> TaskStatus {
        TaskStatus::Pending
    }

    async fn execute(&self, _ctx: &TaskContext) -> Result<TaskOutput, TaskError> {
        let req = pair_request(self.cli_id, self.target, "CLI", "0000", vec![], self.pair_code.clone());
        self.conn
            .send(&req)
            .await
            .map_err(|e| TaskError::ExecutionFailed(e.to_string()))?;

        while let Ok(Some(msg)) = self.conn.recv().await {
            match msg {
                Message::PairResponse(resp) => {
                    return Ok(TaskOutput {
                        result: serde_json::to_value(resp).unwrap_or(serde_json::Value::Null),
                    });
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

pub async fn run_pair(target_id: &str) -> anyhow::Result<()> {
    let node_id = NodeId(Uuid::parse_str(target_id)?);
    let (conn, cli_id) = crate::net::connect_and_identify().await?;
    let pair_code = PairCode::generate();
    
    println!("Pairing code: {}", pair_code);
    println!("Waiting for {} to accept...", node_id);

    let (res_tx, mut res_rx) = tokio::sync::mpsc::unbounded_channel();
    let (task_queue, task_rx) = TaskQueue::new(10);
    let context = TaskContext {
        node_id: cli_id.0,
        origin: None,
        result_tx: Some(res_tx),
    };
    
    let dispatcher = TaskDispatcher::new(task_rx, context);
    tokio::spawn(dispatcher.run());

    let task = Arc::new(ClientPairTask::new(node_id, cli_id, pair_code.to_string(), conn));
    task_queue.submit(task, None).await?;

    if let Some((_id, _origin, result)) = res_rx.recv().await {
        match result {
            Ok(output) => {
                let resp: dos_protocol::message::PairResponse = 
                    serde_json::from_value(output.result).unwrap();
                println!("Pairing accepted by {}!", resp.from);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }

    Ok(())
}
