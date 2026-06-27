use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

use dos_core::TaskStatus;
use dos_networking::Connection;
use dos_protocol::{builder::search_request, Message};
use dos_task_manager::{Task, TaskContext, TaskDispatcher, TaskError, TaskOutput, TaskQueue};

pub struct ClientSearchTask {
    id: Uuid,
    query: String,
    conn: dos_networking::WsConnection,
}

impl ClientSearchTask {
    pub fn new(query: String, conn: dos_networking::WsConnection) -> Self {
        Self {
            id: Uuid::new_v4(),
            query,
            conn,
        }
    }
}

#[async_trait]
impl Task for ClientSearchTask {
    fn id(&self) -> Uuid {
        self.id
    }

    fn kind(&self) -> &'static str {
        "search"
    }

    fn status(&self) -> TaskStatus {
        TaskStatus::Pending
    }

    async fn execute(&self, _ctx: &TaskContext) -> Result<TaskOutput, TaskError> {
        let req = search_request(self.query.clone());
        self.conn
            .send(&req)
            .await
            .map_err(|e| TaskError::ExecutionFailed(e.to_string()))?;

        while let Ok(Some(msg)) = self.conn.recv().await {
            if let Message::SearchResponse(resp) = msg {
                return Ok(TaskOutput {
                    result: serde_json::to_value(resp.results)
                        .unwrap_or(serde_json::Value::Null),
                });
            }
        }
        Err(TaskError::ExecutionFailed("Connection closed".into()))
    }
}

pub async fn run_search(query: String) -> anyhow::Result<()> {
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

    let task = Arc::new(ClientSearchTask::new(query, conn));
    task_queue.submit(task, None).await?;

    if let Some((_id, _origin, result)) = res_rx.recv().await {
        match result {
            Ok(output) => {
                let results: Vec<dos_protocol::message::SearchResult> = 
                    serde_json::from_value(output.result).unwrap_or_default();
                    
                println!("Found {} devices:", results.len());
                for r in results {
                    let caps: Vec<String> = r.capabilities.iter().map(|c| c.to_string()).collect();
                    println!(
                        "  [{:.1}] {} ({} - {}) v{} ID: {}\n      Capabilities: [{}]",
                        r.score, r.name, r.platform, r.status, r.version, r.node_id, caps.join(", ")
                    );
                }
            }
            Err(e) => {
                eprintln!("Search error: {}", e);
            }
        }
    }

    Ok(())
}
