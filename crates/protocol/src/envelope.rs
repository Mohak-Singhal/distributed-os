//! The versioned wire envelope.
//!
//! Every WebSocket frame carries exactly one [`Envelope`]. The relay and all
//! agents only ever serialise/deserialise `Envelope` — never `Message` directly.
//!
//! ```json
//! {
//!   "version": 1,
//!   "message_id": "550e8400-e29b-41d4-a716-446655440000",
//!   "type": "heartbeat",
//!   "from": "...",
//!   "payload": { ... }
//! }
//! ```

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ids::TaskId, message::Message, PROTOCOL_VERSION};

/// The single type serialised to the wire.
///
/// Wrapping every [`Message`] in an envelope lets the relay version-check
/// and correlate responses before touching message semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Protocol version of the sender. Must match [`PROTOCOL_VERSION`] for
    /// the receiver to process the message; otherwise a version-mismatch error
    /// is returned.
    pub version: u16,

    /// Unique ID for this envelope instance.
    ///
    /// Used to correlate request/response pairs (e.g. `SearchRequest` →
    /// `SearchResponse`) without storing per-connection state in the relay.
    pub message_id: Uuid,

    /// The actual message payload.
    #[serde(flatten)]
    pub message: Message,
}

impl Envelope {
    /// Wrap a [`Message`] in an envelope with a fresh random `message_id`.
    pub fn new(message: Message) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            message_id: Uuid::new_v4(),
            message,
        }
    }

    /// Wrap a [`Message`] in an envelope with a specific `message_id`.
    ///
    /// Use this when constructing a response that must echo the request's ID.
    pub fn reply(message: Message, request_id: TaskId) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            message_id: request_id.0,
            message,
        }
    }

    /// Returns `true` if this envelope's version matches the local version.
    pub fn is_compatible(&self) -> bool {
        self.version == PROTOCOL_VERSION
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_round_trip() {
        use crate::message::Message;
        let msg = Message::Error {
            code: "test".into(),
            message: "hello".into(),
        };
        let env = Envelope::new(msg);
        assert!(env.is_compatible());
        assert_eq!(env.version, PROTOCOL_VERSION);

        let json = serde_json::to_string(&env).expect("serialise");
        let decoded: Envelope = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(decoded.message_id, env.message_id);
        assert!(decoded.is_compatible());
    }

    #[test]
    fn old_version_is_incompatible() {
        let env = Envelope {
            version: 0,
            message_id: Uuid::new_v4(),
            message: Message::Error { code: "x".into(), message: "y".into() },
        };
        assert!(!env.is_compatible());
    }
}
