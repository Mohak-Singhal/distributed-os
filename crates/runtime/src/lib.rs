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

#[derive(Debug, serde::Serialize)]
#[serde(tag = "type", content = "data")]
pub enum AgentEvent {
    Connected,
    Disconnected,
    PairingRequested { from: String, code: String },
    PairingAccepted { by: String },
    PingReceived { from: String },
    Error { message: String },
}

/// The core node runtime engine — connects to the relay, registers, and handles messages.
pub struct Agent {
    pub identity: NodeIdentity,
    pub config: Config,
    pub platform: Platform,
    pub event_tx: Option<mpsc::UnboundedSender<AgentEvent>>,
}

impl Agent {
    /// Run the agent with automatic reconnection.
    pub async fn run(&self) -> anyhow::Result<()> {
        loop {
            info!(relay = %self.config.relay_url, "connecting to relay");
            match self.connect_and_run().await {
                Ok(_) => break,
                Err(e) => {
                    let err_msg = format!("Connection error: {}", e);
                    error!(error = %e, "connection lost — retrying in 5s");
                    if let Some(tx) = &self.event_tx {
                        let _ = tx.send(AgentEvent::Error { message: err_msg });
                    }
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
        Ok(())
    }

    async fn connect_and_run(&self) -> anyhow::Result<()> {
        let (ws, _) = connect_async(self.config.relay_url.as_str()).await?;
        info!("connected to relay ✓");
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(AgentEvent::Connected);
        }

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

        // Set up TaskManager
        let (res_tx, mut res_rx) = mpsc::unbounded_channel();
        let (task_queue, task_rx) = dos_task_manager::TaskQueue::new(100);
        
        let mut registry = dos_task_manager::TaskRegistry::new();
        // Register agent ping task
        registry.register("ping", |req| {
            Ok(Box::new(dos_task_manager::PingTask::with_id(req.task_id.0)))
        });

        let context = dos_task_manager::TaskContext {
            node_id: self.identity.node_id,
            origin: None,
            result_tx: Some(res_tx),
        };

        let dispatcher = dos_task_manager::TaskDispatcher::new(task_rx, context);
        let dispatcher_task = tokio::spawn(dispatcher.run());

        // Result forwarder loop
        let res_out_tx = out_tx.clone();
        let my_id = self.identity.node_id;
        let result_task = tokio::spawn(async move {
            while let Some((task_id, origin, result)) = res_rx.recv().await {
                let val = match result {
                    Ok(out) => out.result,
                    Err(e) => serde_json::json!({ "error": e.to_string() }),
                };
                let msg = dos_protocol::builder::task_result(
                    NodeId(my_id),
                    origin.map(NodeId),
                    dos_protocol::ids::TaskId(task_id),
                    val,
                );
                if let Ok(json) = Codec::new().encode(&Envelope::new(msg)) {
                    res_out_tx.send(json).ok();
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
                Ok(envelope) => self.handle_message(envelope.message, &out_tx, &task_queue, &registry).await,
                Err(e) => warn!(error = %e, "decode error"),
            }
        }

        write_task.abort();
        heartbeat_task.abort();
        dispatcher_task.abort();
        result_task.abort();
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

    async fn handle_message(
        &self,
        msg: Message,
        tx: &mpsc::UnboundedSender<String>,
        task_queue: &dos_task_manager::TaskQueue,
        registry: &dos_task_manager::TaskRegistry,
    ) {
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
                if let Some(etx) = &self.event_tx {
                    let _ = etx.send(AgentEvent::Error { message: format!("{}: {}", code, message) });
                }
            }
            Message::PairRequest(req) => {
                info!("Pairing request received from {} with code {}", req.from, req.pair_code);
                if let Some(etx) = &self.event_tx {
                    let _ = etx.send(AgentEvent::PairingRequested { from: req.from.to_string(), code: req.pair_code.clone() });
                }
                // Auto-accept for now in v0.1
                let msg = dos_protocol::builder::pair_accept(NodeId(self.identity.node_id), req.from, "Android Agent", "0000");
                if let Ok(json) = Codec::new().encode(&Envelope::new(msg)) {
                    tx.send(json).ok();
                }
            }
            Message::TaskRequest(req) => {
                info!("Task request received: {} from {}", req.kind, req.from);
                
                // Fire a UI event if it's a ping, for UX purposes
                if req.kind == "ping" {
                    if let Some(etx) = &self.event_tx {
                        let _ = etx.send(AgentEvent::PingReceived { from: req.from.to_string() });
                    }
                }

                let origin = Some(req.from.0);
                
                // Lookup in registry and enqueue
                match registry.create_task(req) {
                    Ok(task) => {
                        if let Err(e) = task_queue.submit(task.into(), origin).await {
                            warn!(error = %e, "failed to submit task to queue");
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "unknown or invalid task requested");
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
