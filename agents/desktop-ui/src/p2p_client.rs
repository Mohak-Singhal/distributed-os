use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use dos_protocol::{
    builder::{heartbeat, task_request, task_result},
    ids::NodeId,
    message::HeartbeatPayload,
    Codec, Envelope, Message,
};
use dos_core::{NodeStatus, Platform};
use futures_util::SinkExt;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message as WsMsg;
use tracing::{info, error, warn};
use uuid::Uuid;

pub struct P2pClient {
    pub node_id: NodeId,
    pub conn: Option<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>,
}

impl P2pClient {
    pub fn new() -> Self {
        Self {
            node_id: NodeId(Uuid::new_v4()),
            conn: None,
        }
    }

    pub async fn connect(&mut self, url: &str) -> anyhow::Result<()> {
        info!("Connecting to {url}");
        let (ws, _) = tokio_tungstenite::connect_async(url).await?;
        self.conn = Some(ws);
        Ok(())
    }

    pub async fn send_heartbeat(&mut self) -> anyhow::Result<()> {
        let ws = self.conn.as_mut().ok_or(anyhow::anyhow!("Not connected"))?;
        let payload = HeartbeatPayload {
            cpu_usage: 0.0,
            memory_usage: 0.0,
            battery_level: None,
            platform: Platform::Mac,
            version: env!("CARGO_PKG_VERSION").into(),
            status: NodeStatus::Online,
            capabilities: vec![],
            timestamp: Utc::now(),
        };
        let msg = heartbeat(self.node_id, payload);
        let json = Codec::new().encode(&Envelope::new(msg))?;
        ws.send(WsMsg::Text(json)).await?;
        Ok(())
    }

    pub async fn send_task(
        &mut self,
        kind: &str,
        payload: serde_json::Value,
    ) -> anyhow::Result<()> {
        let ws = self.conn.as_mut().ok_or(anyhow::anyhow!("Not connected"))?;
        let msg = task_request(self.node_id, None, kind, payload);
        let json = Codec::new().encode(&Envelope::new(msg))?;
        ws.send(WsMsg::Text(json)).await?;
        Ok(())
    }

    pub async fn disconnect(&mut self) {
        if let Some(mut ws) = self.conn.take() {
            let _ = ws.close(None).await;
        }
    }
}
