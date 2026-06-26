use std::time::Duration;

use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMsg};
use tracing::{error, info, warn};

use dos_common::{
    config::Config,
    constants::HEARTBEAT_INTERVAL_SECS,
};
use dos_core::{NodeStatus, Platform};
use dos_crypto::NodeIdentity;
use dos_protocol::{
    ids::NodeId,
    message::{DeviceListResponse, HeartbeatPayload},
    builder::{device_list_request, heartbeat},
    Codec, Envelope, Message,
};

/// The core desktop agent — connects to the relay, registers, and handles messages.
pub struct Agent {
    pub identity: NodeIdentity,
    pub config: Config,
    pub platform: Platform,
    // pub capabilities: Vec<Capability>, // Will be used in Phase 7+
}

impl Agent {
    /// Run the agent with automatic reconnection.
    pub async fn run(&self) -> anyhow::Result<()> {
        loop {
            info!(relay = %self.config.relay_url, "connecting to relay");
            match self.connect_and_run().await {
                Ok(_) => break,
                Err(e) => {
                    error!(error = %e, "connection lost — retrying in 5s");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
        Ok(())
    }

    async fn connect_and_run(&self) -> anyhow::Result<()> {
        let (ws, _) = connect_async(self.config.relay_url.as_str()).await?;
        info!("connected to relay ✓");

        let (mut sink, mut stream) = ws.split();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        let codec = Codec::new();

        // Write task: drains out_rx and sends to WebSocket
        let write_task = tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                if sink.send(WsMsg::Text(msg)).await.is_err() {
                    break;
                }
            }
        });

        // Send initial heartbeat immediately (relay uses it to identify us)
        self.send_heartbeat(&out_tx)?;

        // Heartbeat task: sends every HEARTBEAT_INTERVAL_SECS
        let hb_tx = out_tx.clone();
        let node_id = NodeId(self.identity.node_id);
        let platform = self.platform.clone();
        // let _version = self.config.relay_url.clone(); // removed
        let heartbeat_task = tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
            interval.tick().await; // skip first (already sent above)
            loop {
                interval.tick().await;
                let payload = make_payload(&platform);
                let msg = heartbeat(node_id, payload);
                if let Ok(json) = Codec::new().encode(&Envelope::new(msg)) {
                    hb_tx.send(json).ok();
                }
            }
        });

        // Request device list after 500ms (give relay time to register us)
        {
            let dl_tx = out_tx.clone();
            let node_id = NodeId(self.identity.node_id);
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let msg = device_list_request(node_id);
                if let Ok(json) = Codec::new().encode(&Envelope::new(msg)) {
                    dl_tx.send(json).ok();
                }
            });
        }

        // Main read loop
        while let Some(Ok(WsMsg::Text(text))) = stream.next().await {
            match codec.decode(&text) {
                Ok(envelope) => self.handle_message(envelope.message, &out_tx),
                Err(e) => warn!(error = %e, "decode error"),
            }
        }

        write_task.abort();
        heartbeat_task.abort();
        Ok(())
    }

    fn send_heartbeat(&self, tx: &mpsc::UnboundedSender<String>) -> anyhow::Result<()> {
        let node_id = NodeId(self.identity.node_id);
        let payload = make_payload(&self.platform);
        let msg = heartbeat(node_id, payload);
        let json = Codec::new().encode(&Envelope::new(msg))?;
        tx.send(json)?;
        Ok(())
    }

    fn handle_message(&self, msg: Message, tx: &mpsc::UnboundedSender<String>) {
        match msg {
            Message::DeviceListResponse(list) => self.print_device_list(&list),
            Message::SearchResponse(resp) => {
                info!("search results: {} matches", resp.results.len());
                for r in &resp.results {
                    info!("  [{:.1}] {} — {} ({})", r.score, r.name, r.platform, r.status);
                }
            }
            Message::Error { code, message } => {
                warn!(code, message, "relay error");
            }
            Message::PairRequest(req) => {
                info!("Pairing request received from {} with code {}", req.from, req.pair_code);
                // Auto-accept for now in v0.1
                let msg = dos_protocol::builder::pair_accept(NodeId(self.identity.node_id), req.from, "Desktop Agent", "0000");
                if let Ok(json) = Codec::new().encode(&Envelope::new(msg)) {
                    tx.send(json).ok();
                }
            }
            Message::TaskRequest(req) => {
                info!("Task request received: {} from {}", req.kind, req.from);
                if req.kind == "ping" {
                    let result = serde_json::json!({ "success": true, "message": "pong" });
                    let msg = dos_protocol::builder::task_result(NodeId(self.identity.node_id), Some(req.from), req.task_id, result);
                    if let Ok(json) = Codec::new().encode(&Envelope::new(msg)) {
                        tx.send(json).ok();
                    }
                }
            }
            _ => {}
        }
    }

    fn print_device_list(&self, list: &DeviceListResponse) {
        println!();
        println!("┌─────────────────────────────────────────────────────────┐");
        println!("│  Connected Devices ({:2})                                  │", list.nodes.len());
        println!("├──────────┬──────────────┬──────────────────────────────-─┤");
        println!("│ Status   │ Platform     │ Node ID                         │");
        println!("├──────────┼──────────────┼─────────────────────────────────┤");
        for node in &list.nodes {
            let status_icon = match node.status {
                NodeStatus::Online => "● online ",
                NodeStatus::Busy => "◐ busy   ",
                _ => "○ offline",
            };
            println!(
                "│ {status_icon} │ {:12} │ {} │",
                node.platform.to_string(),
                node.id
            );
        }
        println!("└──────────┴──────────────┴─────────────────────────────────┘");
        println!();
    }
}

fn make_payload(platform: &Platform) -> HeartbeatPayload {
    HeartbeatPayload {
        cpu_usage: 0.0,
        memory_usage: 0.0,
        battery_level: None,
        platform: platform.clone(),
        version: env!("CARGO_PKG_VERSION").into(),
        status: NodeStatus::Online,
        timestamp: Utc::now(),
    }
}
