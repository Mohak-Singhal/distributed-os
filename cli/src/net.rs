use chrono::Utc;
use dos_common::config::Config;
use dos_core::{NodeStatus, Platform};
use dos_networking::{Connection, WsConnection};
use dos_protocol::{builder::heartbeat, ids::NodeId, message::HeartbeatPayload};
use uuid::Uuid;

pub async fn connect_and_identify() -> anyhow::Result<(WsConnection, NodeId)> {
    let config = Config::load("dos-config.toml")?;
    let conn = dos_networking::connect(&config.relay_url).await?;

    let cli_id = NodeId(Uuid::new_v4());
    let payload = HeartbeatPayload {
        cpu_usage: 0.0,
        memory_usage: 0.0,
        battery_level: None,
        platform: Platform::Mac, // Or detect platform if we want, Mac is fine for CLI
        version: env!("CARGO_PKG_VERSION").into(),
        status: NodeStatus::Online,
        capabilities: vec![],
        timestamp: Utc::now(),
    };

    let identify_msg = heartbeat(cli_id, payload);
    conn.send(&identify_msg).await?;

    // Give the relay a moment to register us before we start sending requests
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    Ok((conn, cli_id))
}
