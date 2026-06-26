//! All wire messages for the distributed OS protocol.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use dos_core::{Capability, NodeStatus, Platform, TaskStatus};

use crate::ids::{NodeId, TaskId};

// ── Heartbeat ────────────────────────────────────────────────────────────────

/// Metrics payload attached to every heartbeat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatPayload {
    /// CPU utilisation 0.0–100.0.
    pub cpu_usage: f32,
    /// Memory utilisation 0.0–100.0.
    pub memory_usage: f32,
    /// Battery level 0–100, or `None` if not applicable.
    pub battery_level: Option<u8>,
    /// Platform of the sending node.
    pub platform: Platform,
    /// Agent version string.
    pub version: String,
    /// Current node status.
    pub status: NodeStatus,
    /// Wall-clock time the heartbeat was created.
    pub timestamp: DateTime<Utc>,
}

// ── Pairing ──────────────────────────────────────────────────────────────────

/// Sent by a node wishing to pair with another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairRequest {
    /// The requesting node's ID.
    pub from: NodeId,
    /// Human-readable name of the requesting node.
    pub name: String,
    /// Hex-encoded ed25519 public key of the requesting node.
    pub public_key: String,
    /// Capabilities the requesting node advertises.
    pub capabilities: Vec<Capability>,
    /// Short alphanumeric code displayed to the user for confirmation.
    pub pair_code: String,
}

/// Sent in response to a [`PairRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairResponse {
    /// The responding node's ID.
    pub from: NodeId,
    /// Human-readable name of the responding node.
    pub name: String,
    /// Hex-encoded ed25519 public key of the responding node.
    pub public_key: String,
    /// Whether the pairing was accepted.
    pub accepted: bool,
    /// Human-readable reason for rejection (if `accepted == false`).
    pub reason: Option<String>,
}

// ── Tasks ────────────────────────────────────────────────────────────────────

/// Request to execute a task on a remote node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRequest {
    /// Unique task ID.
    pub task_id: TaskId,
    /// The originating node.
    pub from: NodeId,
    /// The target node (or `None` for broadcast).
    pub to: Option<NodeId>,
    /// Task kind identifier (e.g. `"ping"`, `"search"`).
    pub kind: String,
    /// JSON-encoded task payload, interpreted by the task executor.
    pub payload: serde_json::Value,
}

/// Result of a completed or failed task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    /// Matches the originating [`TaskRequest::task_id`].
    pub task_id: TaskId,
    /// The node executing the task.
    pub from: NodeId,
    /// The originator to send the result back to.
    pub to: Option<NodeId>,
    /// Final status.
    pub status: TaskStatus,
    /// JSON-encoded result data, or `null` on failure.
    pub result: serde_json::Value,
    /// Human-readable error message if the task failed.
    pub error: Option<String>,
    /// Wall-clock completion time.
    pub completed_at: DateTime<Utc>,
}

// ── Search ───────────────────────────────────────────────────────────────────

/// A search request (v0.1: device search only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    /// Unique ID for correlating responses.
    pub request_id: TaskId,
    /// Free-text query (e.g. `"mac"`, `"online"`, `"android"`).
    pub query: String,
}

/// A single device search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The matching node's ID.
    pub node_id: NodeId,
    /// Display name.
    pub name: String,
    /// Platform.
    pub platform: Platform,
    /// Current status.
    pub status: NodeStatus,
    /// Relevance score (higher is better).
    pub score: f32,
}

/// Response to a [`SearchRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    /// Matches [`SearchRequest::request_id`].
    pub request_id: TaskId,
    /// Ranked list of matching nodes.
    pub results: Vec<SearchResult>,
}

// ── Device List ───────────────────────────────────────────────────────────────

/// Request the full list of known nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceListRequest {
    /// The requesting node's ID.
    pub from: NodeId,
}

/// Response containing all known nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceListResponse {
    /// All nodes known to the relay at this moment.
    pub nodes: Vec<NodeSummary>,
}

/// Lightweight node summary for the device list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSummary {
    /// Node ID.
    pub id: NodeId,
    /// Display name.
    pub name: String,
    /// Platform.
    pub platform: Platform,
    /// Current status.
    pub status: NodeStatus,
    /// Last heartbeat timestamp.
    pub last_seen: Option<DateTime<Utc>>,
    /// Advertised capabilities.
    pub capabilities: Vec<Capability>,
}

// ── Envelope ──────────────────────────────────────────────────────────────────

/// The top-level message envelope sent over the wire.
///
/// Every WebSocket frame contains exactly one [`Message`]. The relay inspects
/// the variant to decide whether to store, forward, or handle the message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// Periodic liveness signal.
    Heartbeat {
        /// The sending node's ID.
        from: NodeId,
        /// Metrics payload.
        payload: HeartbeatPayload,
    },
    /// Initiate device pairing.
    PairRequest(PairRequest),
    /// Respond to a pairing request.
    PairResponse(PairResponse),
    /// Submit a task for execution.
    TaskRequest(TaskRequest),
    /// Return task result to originator.
    TaskResult(TaskResult),
    /// Search for devices.
    SearchRequest(SearchRequest),
    /// Search results.
    SearchResponse(SearchResponse),
    /// Request all known nodes.
    DeviceListRequest(DeviceListRequest),
    /// Known nodes response.
    DeviceListResponse(DeviceListResponse),
    /// Relay-level error (e.g. authentication failure, unknown target).
    Error {
        /// Short machine-readable code.
        code: String,
        /// Human-readable description.
        message: String,
    },
}

impl Message {
    /// Serialise this message to a JSON string.
    ///
    /// # Errors
    /// Returns `Err` if serde serialisation fails (should never happen for
    /// well-formed messages).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialise a [`Message`] from a JSON string.
    ///
    /// # Errors
    /// Returns `Err` on malformed JSON or unknown message type.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn message_round_trip() {
        let msg = Message::Error {
            code: "not_found".to_string(),
            message: "node not found".to_string(),
        };
        let json = msg.to_json().expect("serialise");
        let decoded = Message::from_json(&json).expect("deserialise");
        match decoded {
            Message::Error { code, .. } => assert_eq!(code, "not_found"),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn node_id_display() {
        let id = NodeId(Uuid::nil());
        assert_eq!(id.to_string(), "00000000-0000-0000-0000-000000000000");
    }
}
