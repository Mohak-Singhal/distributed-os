use dos_networking::Connection;
use dos_protocol::{builder::task_request, Message, ids::NodeId};
use std::time::Instant;
use uuid::Uuid;

pub async fn run_ping(target_id: &str) -> anyhow::Result<()> {
    let node_id = NodeId(Uuid::parse_str(target_id)?);
    
    println!("Pinging {}...", node_id);
    let (conn, cli_id) = crate::net::connect_and_identify().await?;

    // Create a ping task request
    let payload = serde_json::json!({ "type": "ping" });
    let req = task_request(cli_id, Some(node_id), "ping".to_string(), payload);
    
    let start = Instant::now();
    conn.send(&req).await?;

    // Wait for the response
    while let Ok(Some(msg)) = conn.recv().await {
        match msg {
            Message::TaskResult(_result) => {
                let duration = start.elapsed();
                println!("Reply from {}: time={:?}", node_id, duration);
                break;
            }
            Message::Error { code, message } => {
                eprintln!("Error: {} - {}", code, message);
                break;
            }
            _ => {}
        }
    }

    Ok(())
}
