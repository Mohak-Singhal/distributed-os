use dos_common::config::Config;
use dos_networking::Connection;
use dos_protocol::{builder::pair_request, Message, ids::NodeId, PairCode};
use uuid::Uuid;

pub async fn run_pair(target_id: &str) -> anyhow::Result<()> {
    let node_id = NodeId(Uuid::parse_str(target_id)?);
    let config = Config::load("dos-config.toml")?;
    
    let conn = dos_networking::connect(&config.relay_url).await?;
    let pair_code = PairCode::generate();
    
    println!("Pairing code: {}", pair_code);
    println!("Waiting for {} to accept...", node_id);

    // Create a pair request
    let req = pair_request(NodeId(Uuid::nil()), "CLI", "0000", vec![], pair_code.to_string());
    conn.send(&req).await?;

    // Wait for the response
    while let Ok(Some(msg)) = conn.recv().await {
        match msg {
            Message::PairResponse(resp) => {
                println!("Pairing accepted by {}!", resp.from);
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
