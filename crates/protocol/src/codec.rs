//! Protocol codec — encode/decode between [`Envelope`] and raw strings.
//!
//! All WebSocket framing goes through this single type. It enforces:
//! - Version compatibility on every inbound frame
//! - JSON encoding/decoding with typed errors
//! - A clean separation between transport (bytes) and protocol (messages)

use crate::{
    envelope::Envelope,
    error::ProtocolError,
    PROTOCOL_VERSION,
};

/// Encodes outgoing messages and decodes incoming frames.
///
/// The codec is stateless and cheaply cloneable — create one per connection.
#[derive(Debug, Clone, Default)]
pub struct Codec;

impl Codec {
    /// Create a new codec instance.
    pub fn new() -> Self {
        Self
    }

    /// Encode an [`Envelope`] to a JSON string ready for transmission.
    ///
    /// # Errors
    /// Returns [`ProtocolError::Serialisation`] if JSON encoding fails.
    pub fn encode(&self, envelope: &Envelope) -> Result<String, ProtocolError> {
        serde_json::to_string(envelope).map_err(ProtocolError::Serialisation)
    }

    /// Decode a raw JSON string into an [`Envelope`].
    ///
    /// Returns [`ProtocolError::VersionMismatch`] if the envelope version
    /// is not equal to [`PROTOCOL_VERSION`].
    ///
    /// # Errors
    /// - [`ProtocolError::Serialisation`] on malformed JSON
    /// - [`ProtocolError::VersionMismatch`] on incompatible protocol version
    pub fn decode(&self, raw: &str) -> Result<Envelope, ProtocolError> {
        let envelope: Envelope =
            serde_json::from_str(raw).map_err(ProtocolError::Serialisation)?;

        if !envelope.is_compatible() {
            return Err(ProtocolError::VersionMismatch {
                expected: PROTOCOL_VERSION,
                received: envelope.version,
            });
        }

        Ok(envelope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{builder, envelope::Envelope, message::Message};

    fn round_trip(msg: Message) -> Envelope {
        let codec = Codec::new();
        let env = Envelope::new(msg);
        let raw = codec.encode(&env).expect("encode");
        codec.decode(&raw).expect("decode")
    }

    #[test]
    fn heartbeat_round_trip() {
        use crate::ids::NodeId;
        use dos_core::{NodeStatus, Platform};
        use chrono::Utc;
        use crate::message::HeartbeatPayload;

        let from = NodeId::new_random();
        let payload = HeartbeatPayload {
            cpu_usage: 12.5,
            memory_usage: 40.0,
            battery_level: Some(80),
            platform: Platform::Mac,
            version: "0.1.0".into(),
            status: NodeStatus::Online,
            capabilities: vec![],
            timestamp: Utc::now(),
        };
        let msg = builder::heartbeat(from, payload);
        let decoded = round_trip(msg);
        match decoded.message {
            Message::Heartbeat { from: f, .. } => assert_eq!(f, from),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn wrong_version_rejected() {
        let codec = Codec::new();
        let env = Envelope {
            version: 0,
            message_id: uuid::Uuid::new_v4(),
            message: Message::Error { code: "x".into(), message: "y".into() },
        };
        let raw = codec.encode(&env).expect("encode");
        let result = codec.decode(&raw);
        assert!(matches!(result, Err(ProtocolError::VersionMismatch { .. })));
    }

    #[test]
    fn malformed_json_rejected() {
        let codec = Codec::new();
        let result = codec.decode("not json at all {{{");
        assert!(matches!(result, Err(ProtocolError::Serialisation(_))));
    }
}
