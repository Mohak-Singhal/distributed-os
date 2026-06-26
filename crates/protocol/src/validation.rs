//! Message validation — runs at the network boundary only.
//!
//! Validation is intentionally **not** called on messages we construct
//! internally. It is only applied to inbound messages from untrusted senders
//! (relay receiver, WebSocket acceptor) before they are processed.

use crate::{error::ProtocolError, message::Message};

/// Validate an inbound [`Message`].
///
/// Checks structural invariants that cannot be expressed in the type system:
/// - Required string fields are non-empty
/// - Public keys are valid 64-character hex strings
/// - Pair codes are exactly 6 alphanumeric characters
///
/// # Errors
/// Returns [`ProtocolError::ValidationFailed`] with a human-readable
/// description of the first violation found.
pub fn validate(msg: &Message) -> Result<(), ProtocolError> {
    match msg {
        Message::Heartbeat { from, payload } => {
            validate_node_id_nonzero(from.0)?;
            if payload.version.is_empty() {
                return Err(validation_err("heartbeat.payload.version must not be empty"));
            }
            if payload.cpu_usage < 0.0 || payload.cpu_usage > 100.0 {
                return Err(validation_err("heartbeat.payload.cpu_usage must be 0–100"));
            }
            if payload.memory_usage < 0.0 || payload.memory_usage > 100.0 {
                return Err(validation_err("heartbeat.payload.memory_usage must be 0–100"));
            }
            Ok(())
        }

        Message::PairRequest(req) => {
            validate_node_id_nonzero(req.from.0)?;
            if req.name.trim().is_empty() {
                return Err(validation_err("pair_request.name must not be empty"));
            }
            validate_public_key(&req.public_key)?;
            validate_pair_code_format(&req.pair_code)?;
            Ok(())
        }

        Message::PairResponse(resp) => {
            validate_node_id_nonzero(resp.from.0)?;
            if resp.name.trim().is_empty() {
                return Err(validation_err("pair_response.name must not be empty"));
            }
            validate_public_key(&resp.public_key)?;
            Ok(())
        }

        Message::TaskRequest(req) => {
            validate_node_id_nonzero(req.from.0)?;
            if req.kind.trim().is_empty() {
                return Err(validation_err("task_request.kind must not be empty"));
            }
            Ok(())
        }

        Message::SearchRequest(req) => {
            if req.query.trim().is_empty() {
                return Err(validation_err("search_request.query must not be empty"));
            }
            Ok(())
        }

        // DeviceListRequest, TaskResult, SearchResponse, DeviceListResponse,
        // Error — structurally valid by construction; no extra checks needed.
        _ => Ok(()),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn validation_err(reason: &str) -> ProtocolError {
    ProtocolError::ValidationFailed(reason.to_string())
}

fn validate_node_id_nonzero(id: uuid::Uuid) -> Result<(), ProtocolError> {
    if id.is_nil() {
        return Err(validation_err("node_id must not be the nil UUID"));
    }
    Ok(())
}

/// ed25519 public keys encode to exactly 64 lowercase hex characters.
fn validate_public_key(key: &str) -> Result<(), ProtocolError> {
    if key.len() != 64 || !key.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(validation_err(
            "public_key must be a 64-character lowercase hex string",
        ));
    }
    Ok(())
}

fn validate_pair_code_format(code: &str) -> Result<(), ProtocolError> {
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(validation_err(
            "pair_code must be exactly 6 alphanumeric characters",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ids::NodeId, message::*};
    use dos_core::{Capability, NodeStatus, Platform};
    use chrono::Utc;
    use uuid::Uuid;

    fn valid_heartbeat() -> Message {
        Message::Heartbeat {
            from: NodeId::new_random(),
            payload: HeartbeatPayload {
                cpu_usage: 10.0,
                memory_usage: 50.0,
                battery_level: None,
                platform: Platform::Mac,
                version: "0.1.0".into(),
                status: NodeStatus::Online,
                timestamp: Utc::now(),
            },
        }
    }

    #[test]
    fn valid_heartbeat_passes() {
        assert!(validate(&valid_heartbeat()).is_ok());
    }

    #[test]
    fn heartbeat_empty_version_fails() {
        let msg = Message::Heartbeat {
            from: NodeId::new_random(),
            payload: HeartbeatPayload {
                cpu_usage: 10.0,
                memory_usage: 50.0,
                battery_level: None,
                platform: Platform::Linux,
                version: "".into(),
                status: NodeStatus::Online,
                timestamp: Utc::now(),
            },
        };
        assert!(matches!(validate(&msg), Err(ProtocolError::ValidationFailed(_))));
    }

    #[test]
    fn heartbeat_nil_node_id_fails() {
        let msg = Message::Heartbeat {
            from: NodeId(Uuid::nil()),
            payload: HeartbeatPayload {
                cpu_usage: 0.0,
                memory_usage: 0.0,
                battery_level: None,
                platform: Platform::Linux,
                version: "0.1.0".into(),
                status: NodeStatus::Online,
                timestamp: Utc::now(),
            },
        };
        assert!(matches!(validate(&msg), Err(ProtocolError::ValidationFailed(_))));
    }

    #[test]
    fn pair_request_bad_pubkey_fails() {
        let msg = Message::PairRequest(PairRequest {
            from: NodeId::new_random(),
            name: "Test".into(),
            public_key: "not-a-key".into(),
            capabilities: vec![Capability::Compute],
            pair_code: "ABC123".into(),
        });
        assert!(matches!(validate(&msg), Err(ProtocolError::ValidationFailed(_))));
    }

    #[test]
    fn pair_request_empty_name_fails() {
        let msg = Message::PairRequest(PairRequest {
            from: NodeId::new_random(),
            name: "   ".into(),
            public_key: "a".repeat(64),
            capabilities: vec![],
            pair_code: "ABC123".into(),
        });
        assert!(matches!(validate(&msg), Err(ProtocolError::ValidationFailed(_))));
    }
}
