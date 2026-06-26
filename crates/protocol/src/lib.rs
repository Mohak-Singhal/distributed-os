//! Wire protocol for the Personal Distributed OS.
//!
//! Every message exchanged between nodes — over WebSocket or through the relay
//! — is represented as a [`Message`] variant and wrapped in an [`Envelope`].
//!
//! # Protocol version
//! The current wire version is exposed as [`PROTOCOL_VERSION`]. Nodes running
//! incompatible versions are rejected at the codec layer before any message
//! parsing occurs.

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

/// Current wire protocol version. Increment when the wire format changes in a
/// backwards-incompatible way.
pub const PROTOCOL_VERSION: u16 = 1;

pub mod builder;
pub mod codec;
pub mod envelope;
pub mod error;
pub mod ids;
pub mod message;
pub mod pair_code;
pub mod validation;

pub use builder::{
    device_list_request, error_msg, heartbeat, pair_accept, pair_reject, pair_request,
    search_request,
};
pub use codec::Codec;
pub use envelope::Envelope;
pub use error::ProtocolError;
pub use ids::{NodeId, TaskId};
pub use message::Message;
pub use pair_code::PairCode;
pub use validation::validate;
