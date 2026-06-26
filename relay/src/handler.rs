use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::{accept_async, tungstenite::Message as WsMsg};
use tracing::{info, warn};

use dos_protocol::{
    ids::NodeId,
    message::SearchResponse,
    Codec, Envelope, Message,
};

use crate::registry::{NodeTx, Registry};

/// Handle one inbound WebSocket connection from a node.
pub async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    registry: Arc<Registry>,
) {
    let ws = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            warn!(peer = %peer, error = %e, "WebSocket handshake failed");
            return;
        }
    };

    info!(peer = %peer, "WebSocket connected");
    let (mut sink, mut stream) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Outbound write task: receives JSON strings and sends over WebSocket
    let write_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sink.send(WsMsg::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Wait for the first heartbeat (identification within 10 seconds)
    let node_id = match identify(&mut stream, &registry, tx.clone()).await {
        Some(id) => id,
        None => {
            warn!(peer = %peer, "node did not send heartbeat — dropping");
            write_task.abort();
            return;
        }
    };

    info!(peer = %peer, %node_id, "node identified and registered");

    // Main message loop
    while let Some(Ok(WsMsg::Text(text))) = stream.next().await {
        dispatch(&text, node_id, &registry, &tx).await;
    }

    // Cleanup
    registry.remove(node_id).await;
    info!(%node_id, "node disconnected");
    write_task.abort();
}

/// Wait for the first heartbeat to identify the connecting node.
async fn identify(
    stream: &mut (impl StreamExt<Item = Result<WsMsg, tokio_tungstenite::tungstenite::Error>> + Unpin),
    registry: &Arc<Registry>,
    tx: NodeTx,
) -> Option<NodeId> {
    let codec = Codec::new();
    // Give the client 10 seconds to send its first heartbeat
    let result = tokio::time::timeout(Duration::from_secs(10), stream.next()).await;
    let text = match result {
        Ok(Some(Ok(WsMsg::Text(t)))) => t,
        _ => return None,
    };

    let envelope = match codec.decode(&text) {
        Ok(e) => e,
        Err(e) => {
            warn!(error = %e, "failed to decode first message");
            return None;
        }
    };

    match envelope.message {
        Message::Heartbeat { from, ref payload } => {
            let name = format!("node-{}", &from.to_string()[..8]);
            registry
                .upsert_from_heartbeat(
                    from,
                    name,
                    payload.platform.clone(),
                    vec![],
                    payload,
                    tx,
                )
                .await;
            Some(from)
        }
        _ => {
            warn!("first message was not a heartbeat");
            None
        }
    }
}

/// Dispatch a decoded message from a known node.
async fn dispatch(text: &str, from: NodeId, registry: &Arc<Registry>, tx: &NodeTx) {
    let codec = Codec::new();
    let envelope = match codec.decode(text) {
        Ok(e) => e,
        Err(e) => {
            send_error(tx, "decode_error", &e.to_string());
            return;
        }
    };

    match envelope.message {
        Message::Heartbeat { from: node_id, ref payload } => {
            registry.update_heartbeat(node_id, payload).await;
        }

        Message::DeviceListRequest(_) => {
            let list = registry.device_list().await;
            send_msg(tx, Message::DeviceListResponse(list));
        }

        Message::SearchRequest(req) => {
            let results = registry.search(&req.query).await;
            send_msg(
                tx,
                Message::SearchResponse(SearchResponse {
                    request_id: req.request_id,
                    results,
                }),
            );
        }

        Message::PairRequest(ref req) => {
            // Forward to target if specified; otherwise send error
            if let Some(target_tx) = registry.get_tx(req.to).await {
                send_msg(&target_tx, envelope.message);
            } else {
                send_error(tx, "target_offline", "target node is not connected");
            }
        }

        Message::PairResponse(ref resp) => {
            if let Some(target_tx) = registry.get_tx(resp.to).await {
                send_msg(&target_tx, envelope.message);
            }
        }

        Message::TaskRequest(ref req) => {
            if let Some(to) = req.to {
                if let Some(target_tx) = registry.get_tx(to).await {
                    send_msg(&target_tx, envelope.message);
                } else {
                    send_error(tx, "target_offline", "target node is not connected");
                }
            }
        }

        Message::TaskResult(ref res) => {
            if let Some(to) = res.to {
                if let Some(target_tx) = registry.get_tx(to).await {
                    send_msg(&target_tx, envelope.message);
                }
            }
        }

        _ => {} // TaskResult, SearchResponse etc. — not handled by relay
    }
}

fn send_msg(tx: &NodeTx, msg: Message) {
    let env = Envelope::new(msg);
    if let Ok(json) = Codec::new().encode(&env) {
        tx.send(json).ok();
    }
}

fn send_error(tx: &NodeTx, code: &str, message: &str) {
    send_msg(tx, Message::Error { code: code.into(), message: message.into() });
}
