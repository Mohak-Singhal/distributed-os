use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::{mpsc, RwLock};
use tracing::info;

use dos_core::{Capability, NodeStatus, Platform};
use dos_protocol::{
    ids::NodeId,
    message::{DeviceListResponse, HeartbeatPayload, NodeSummary},
};

/// Sender half of a node's outbound message channel.
pub type NodeTx = mpsc::UnboundedSender<String>;

/// In-memory record for a connected node.
#[derive(Debug, Clone)]
pub struct ConnectedNode {
    pub node_id: NodeId,
    pub name: String,
    pub platform: Platform,
    pub capabilities: Vec<Capability>,
    pub version: String,
    pub status: NodeStatus,
    pub last_seen: chrono::DateTime<Utc>,
    pub tx: NodeTx,
    pub connection_id: uuid::Uuid,
}

/// Thread-safe registry of all connected nodes.
#[derive(Debug, Default)]
pub struct Registry {
    nodes: RwLock<HashMap<NodeId, ConnectedNode>>,
}

impl Registry {
    /// Create a new empty registry.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Register or update a node from a heartbeat.
    pub async fn upsert_from_heartbeat(
        &self,
        node_id: NodeId,
        name: String,
        platform: Platform,
        capabilities: Vec<Capability>,
        payload: &HeartbeatPayload,
        tx: NodeTx,
        connection_id: uuid::Uuid,
    ) {
        let mut nodes = self.nodes.write().await;
        nodes.insert(
            node_id,
            ConnectedNode {
                node_id,
                name,
                platform,
                capabilities,
                version: payload.version.clone(),
                status: NodeStatus::Online,
                last_seen: payload.timestamp,
                tx,
                connection_id,
            },
        );
    }

    /// Update only the heartbeat fields for a known node.
    pub async fn update_heartbeat(&self, node_id: NodeId, payload: &HeartbeatPayload) {
        let mut nodes = self.nodes.write().await;
        if let Some(n) = nodes.get_mut(&node_id) {
            n.status = NodeStatus::Online;
            n.version = payload.version.clone();
            n.last_seen = payload.timestamp;
        }
    }

    /// Remove a node on disconnect if the connection ID matches.
    pub async fn remove(&self, node_id: NodeId, connection_id: uuid::Uuid) {
        let mut nodes = self.nodes.write().await;
        if let Some(n) = nodes.get(&node_id) {
            if n.connection_id == connection_id {
                nodes.remove(&node_id);
            }
        }
    }

    /// Return a snapshot of all connected nodes for a DeviceListResponse.
    pub async fn device_list(&self) -> DeviceListResponse {
        let nodes = self.nodes.read().await;
        let summaries = nodes
            .values()
            .map(|n| NodeSummary {
                id: n.node_id,
                name: n.name.clone(),
                platform: n.platform.clone(),
                status: n.status,
                last_seen: Some(n.last_seen),
                capabilities: n.capabilities.clone(),
            })
            .collect();
        DeviceListResponse { nodes: summaries }
    }

    /// Search nodes by query string and return ranked results.
    pub async fn search(&self, query: &str) -> Vec<dos_protocol::message::SearchResult> {
        let q = query.trim().to_lowercase();
        let nodes = self.nodes.read().await;
        let mut results: Vec<dos_protocol::message::SearchResult> = nodes
            .values()
            .filter_map(|n| {
                let score = score_node(n, &q);
                if score > 0.0 {
                    Some(dos_protocol::message::SearchResult {
                        node_id: n.node_id,
                        name: n.name.clone(),
                        platform: n.platform.clone(),
                        status: n.status,
                        capabilities: n.capabilities.clone(),
                        version: n.version.clone(),
                        score,
                    })
                } else {
                    None
                }
            })
            .collect();
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    /// Get the outbound channel for a specific node (for forwarding).
    pub async fn get_tx(&self, node_id: NodeId) -> Option<NodeTx> {
        self.nodes.read().await.get(&node_id).map(|n| n.tx.clone())
    }

    /// Log current connected nodes.
    #[allow(dead_code)]
    pub async fn log_state(&self) {
        let nodes = self.nodes.read().await;
        info!(count = nodes.len(), "registry state");
        for n in nodes.values() {
            info!(
                node_id = %n.node_id,
                name    = %n.name,
                platform = %n.platform,
                "  connected node"
            );
        }
    }
}

fn score_node(node: &ConnectedNode, q: &str) -> f32 {
    if q.is_empty() {
        return 1.0;
    }
    if node.status.to_string() == q {
        return 1.0;
    }
    if node.platform.to_string() == q {
        return 1.0;
    }
    if node.node_id.to_string().to_lowercase().contains(q) {
        return 0.9;
    }
    if node.name.to_lowercase().contains(q) {
        return 0.8;
    }
    if node.version.to_lowercase().contains(q) {
        return 0.8;
    }
    if node.platform.to_string().contains(q) {
        return 0.7;
    }

    let target_cap = if let Some(stripped) = q.strip_prefix("capability=") {
        stripped
    } else {
        q
    };
    if node
        .capabilities
        .iter()
        .any(|c| c.to_string().to_lowercase().contains(target_cap))
    {
        return if q.starts_with("capability=") { 1.0 } else { 0.6 };
    }

    0.0
}
