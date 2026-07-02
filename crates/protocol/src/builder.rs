//! Ergonomic message constructors.
//!
//! These functions replace scattered `Message::Variant { field: value, ... }`
//! constructions across the codebase. They are thin wrappers — no logic lives
//! here beyond field assignment.

use uuid::Uuid;

use dos_core::{Capability, TaskStatus};

use crate::{
    ids::{NodeId, TaskId},
    message::*,
};

/// Construct a [`Message::Heartbeat`].
pub fn heartbeat(from: NodeId, payload: HeartbeatPayload) -> Message {
    Message::Heartbeat { from, payload }
}

/// Construct a [`Message::PairRequest`].
pub fn pair_request(
    from: NodeId,
    to: NodeId,
    name: impl Into<String>,
    public_key: impl Into<String>,
    capabilities: Vec<Capability>,
    pair_code: impl Into<String>,
) -> Message {
    Message::PairRequest(PairRequest {
        from,
        to,
        name: name.into(),
        public_key: public_key.into(),
        capabilities,
        pair_code: pair_code.into(),
    })
}

/// Construct a [`Message::PairResponse`] accepting a pair request.
pub fn pair_accept(
    from: NodeId,
    to: NodeId,
    name: impl Into<String>,
    public_key: impl Into<String>,
) -> Message {
    Message::PairResponse(PairResponse {
        from,
        to,
        name: name.into(),
        public_key: public_key.into(),
        accepted: true,
        reason: None,
    })
}

/// Construct a [`Message::PairResponse`] rejecting a pair request.
pub fn pair_reject(
    from: NodeId,
    to: NodeId,
    name: impl Into<String>,
    public_key: impl Into<String>,
    reason: impl Into<String>,
) -> Message {
    Message::PairResponse(PairResponse {
        from,
        to,
        name: name.into(),
        public_key: public_key.into(),
        accepted: false,
        reason: Some(reason.into()),
    })
}

/// Construct a [`Message::DeviceListRequest`].
pub fn device_list_request(from: NodeId) -> Message {
    Message::DeviceListRequest(DeviceListRequest { from })
}

/// Construct a [`Message::SearchRequest`] with a fresh request ID.
pub fn search_request(query: impl Into<String>) -> Message {
    Message::SearchRequest(SearchRequest {
        request_id: TaskId(Uuid::new_v4()),
        query: query.into(),
    })
}

/// Construct a [`Message::TaskRequest`].
pub fn task_request(
    from: NodeId,
    to: Option<NodeId>,
    kind: impl Into<String>,
    payload: serde_json::Value,
) -> Message {
    Message::TaskRequest(TaskRequest {
        task_id: TaskId(Uuid::new_v4()),
        from,
        to,
        kind: kind.into(),
        payload,
    })
}

/// Construct a [`Message::TaskResult`].
pub fn task_result(
    from: NodeId,
    to: Option<NodeId>,
    task_id: TaskId,
    result: serde_json::Value,
) -> Message {
    Message::TaskResult(TaskResult {
        task_id,
        from,
        to,
        status: TaskStatus::Completed, // simplified for v0.1
        result,
        error: None,
        completed_at: chrono::Utc::now(),
    })
}

/// Construct a [`Message::Error`].
pub fn error_msg(code: impl Into<String>, message: impl Into<String>) -> Message {
    Message::Error { code: code.into(), message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dos_core::{NodeStatus, Platform};
    use chrono::Utc;

    #[test]
    fn heartbeat_builder() {
        let from = NodeId::new_random();
        let payload = HeartbeatPayload {
            cpu_usage: 5.0,
            memory_usage: 30.0,
            battery_level: Some(90),
            platform: Platform::Mac,
            version: "0.1.0".into(),
            status: NodeStatus::Online,
            capabilities: vec![],
            timestamp: Utc::now(),
        };
        let msg = heartbeat(from, payload);
        assert!(matches!(msg, Message::Heartbeat { .. }));
    }

    #[test]
    fn search_request_builder() {
        let msg = search_request("android");
        match msg {
            Message::SearchRequest(req) => assert_eq!(req.query, "android"),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn pair_accept_sets_accepted_true() {
        let from = NodeId::new_random();
        let to = NodeId::new_random();
        let msg = pair_accept(from, to, "My Node", "a".repeat(64));
        match msg {
            Message::PairResponse(r) => {
                assert!(r.accepted);
                assert!(r.reason.is_none());
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn pair_reject_sets_accepted_false() {
        let from = NodeId::new_random();
        let to = NodeId::new_random();
        let msg = pair_reject(from, to, "My Node", "a".repeat(64), "user denied");
        match msg {
            Message::PairResponse(r) => {
                assert!(!r.accepted);
                assert_eq!(r.reason.as_deref(), Some("user denied"));
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn error_builder() {
        let msg = error_msg("not_found", "node not found");
        match msg {
            Message::Error { code, message } => {
                assert_eq!(code, "not_found");
                assert_eq!(message, "node not found");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }
}
