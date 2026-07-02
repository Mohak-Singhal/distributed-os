use std::time::Duration;

use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMsg};
use tokio::net::TcpListener;
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
    builder::{heartbeat},
    Codec, Envelope, Message,
};

#[derive(Debug, serde::Serialize)]
#[serde(tag = "type", content = "data")]
pub enum AgentEvent {
    Connected { relay_url: String },
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
    pub registry: dos_task_manager::TaskRegistry,
}

impl Agent {
    /// Run the agent with automatic reconnection.
    pub async fn run(&self) -> anyhow::Result<()> {
        loop {
            info!(relay = %self.config.relay_url, "connecting to relay");
            match self.connect_and_run().await {
                Ok(_) => {
                    info!("connection closed normally — retrying in 5s");
                    if let Some(tx) = &self.event_tx {
                        let _ = tx.send(AgentEvent::Disconnected);
                    }
                }
                Err(e) => {
                    let err_msg = format!("Connection error: {}", e);
                    error!(error = %e, "connection lost — retrying in 5s");
                    if let Some(tx) = &self.event_tx {
                        let _ = tx.send(AgentEvent::Error { message: err_msg });
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        #[allow(unreachable_code)]
        Ok(())
    }

    /// Run in P2P server mode — listen on node_port, accept incoming
    /// WebSocket connections, and handle the protocol directly (no relay).
    pub async fn serve(&self) -> anyhow::Result<()> {
        let addr = format!("0.0.0.0:{}", self.config.node_port);
        info!(addr = %addr, "starting P2P WebSocket server");
        let listener = TcpListener::bind(&addr).await?;
        
        loop {
            match listener.accept().await {
                Ok((tcp, peer)) => {
                    info!(peer = %peer, "incoming connection");
                    let ws = tokio_tungstenite::accept_async(tcp).await?;
                    if let Err(e) = self.accept_and_run(ws).await {
                        error!(error = %e, "P2P session ended with error");
                    }
                }
                Err(e) => {
                    error!(error = %e, "accept failed — retrying in 1s");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    async fn accept_and_run(&self, ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) -> anyhow::Result<()> {
        info!("P2P client connected ✓");
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(AgentEvent::Connected { relay_url: format!("p2p://0.0.0.0:{}", self.config.node_port) });
        }
        self.run_event_loop(ws).await
    }

    async fn connect_and_run(&self) -> anyhow::Result<()> {
        let mut relay_url = self.config.relay_url.clone();
        
        let needs_discovery = relay_url == "discover"
            || relay_url.contains("discover")
            || relay_url.is_empty();

        let ws = if needs_discovery {
            info!("Relay IP configured as 'discover'. Initiating UDP auto-discovery...");
            if let Some(tx) = &self.event_tx {
                let _ = tx.send(AgentEvent::Error { message: "Searching for relay...".to_string() });
            }
            if let Some(discovered) = dos_discovery::udp::discover_relay(Duration::from_secs(10)).await {
                relay_url = format!("ws://{}", discovered);
                info!(url = %relay_url, "auto-discovered relay URL");
                connect_async(relay_url.as_str()).await?.0
            } else {
                return Err(anyhow::anyhow!("failed to auto-discover relay"));
            }
        } else {
            match connect_async(relay_url.as_str()).await {
                Ok((ws, _)) => ws,
                Err(e) => {
                    warn!(error = %e, url = %relay_url, "failed to connect to configured relay. Falling back to UDP discovery...");
                    if let Some(tx) = &self.event_tx {
                        let _ = tx.send(AgentEvent::Error { message: "Relay offline, searching local network...".to_string() });
                    }
                    if let Some(discovered) = dos_discovery::udp::discover_relay(Duration::from_secs(10)).await {
                        relay_url = format!("ws://{}", discovered);
                        info!(url = %relay_url, "auto-discovered relay URL as fallback");
                        connect_async(relay_url.as_str()).await?.0
                    } else {
                        return Err(anyhow::anyhow!("failed to connect to {} and discovery failed", relay_url));
                    }
                }
            }
        };

        info!("connected to relay ✓");
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(AgentEvent::Connected { relay_url: relay_url.clone() });
        }
        self.run_event_loop(ws).await
    }

    /// Shared WebSocket event loop used by both relay client and P2P server mode.
    async fn run_event_loop<S>(&self, ws: tokio_tungstenite::WebSocketStream<S>) -> anyhow::Result<()>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
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
        let capabilities = self.registry.get_capabilities();
        self.send_heartbeat(&out_tx, capabilities.clone())?;

        // Track heartbeat acknowledgments to detect half-open sockets
        let last_ack = std::sync::Arc::new(tokio::sync::Mutex::new(chrono::Utc::now()));
        let last_ack_clone = last_ack.clone();
        let node_id = NodeId(self.identity.node_id);
        
        let mut hb_interval = tokio::time::interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
        hb_interval.tick().await; // skip first (already sent above)

        loop {
            tokio::select! {
                res = stream.next() => {
                    match res {
                        Some(Ok(WsMsg::Text(text))) => {
                            match codec.decode(&text) {
                                Ok(envelope) => {
                                    if matches!(envelope.message, Message::HeartbeatAck) {
                                        let mut ack = last_ack_clone.lock().await;
                                        *ack = chrono::Utc::now();
                                    } else {
                                        self.handle_message(envelope.message, &out_tx, &task_queue, &self.registry).await;
                                    }
                                }
                                Err(e) => warn!(error = %e, "decode error"),
                            }
                        }
                        Some(Ok(_)) => {} // ignore other WS frames
                        Some(Err(e)) => {
                            error!(error = %e, "read error on stream");
                            break;
                        }
                        None => {
                            info!("connection closed by remote peer");
                            break;
                        }
                    }
                }
                _ = hb_interval.tick() => {
                    let elapsed = {
                        let last = last_ack_clone.lock().await;
                        chrono::Utc::now().signed_duration_since(*last)
                    };
                    if elapsed.num_seconds() > (HEARTBEAT_INTERVAL_SECS * 2 + 5) as i64 {
                        error!("No heartbeat acknowledgment received for {} seconds — closing connection", elapsed.num_seconds());
                        break;
                    }
                    
                    let payload = make_payload(&self.platform, capabilities.clone());
                    let msg = heartbeat(node_id, payload);
                    if let Ok(json) = Codec::new().encode(&Envelope::new(msg)) {
                        out_tx.send(json).ok();
                    }
                }
            }
        }

        write_task.abort();
        dispatcher_task.abort();
        result_task.abort();
        Ok(())
    }

    fn send_heartbeat(&self, tx: &mpsc::UnboundedSender<String>, capabilities: Vec<dos_core::Capability>) -> anyhow::Result<()> {
        let node_id = NodeId(self.identity.node_id);
        let payload = make_payload(&self.platform, capabilities);
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

fn make_payload(platform: &Platform, capabilities: Vec<dos_core::Capability>) -> HeartbeatPayload {
    HeartbeatPayload {
        cpu_usage: 0.0,
        memory_usage: 0.0,
        battery_level: None,
        platform: platform.clone(),
        version: env!("CARGO_PKG_VERSION").into(),
        status: NodeStatus::Online,
        capabilities,
        timestamp: Utc::now(),
    }
}
