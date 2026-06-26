//! System-wide constants.
//!
//! Single source of truth for values used across multiple crates.
//! Import from here rather than hardcoding numbers inline.

/// Default WebSocket port for the relay server.
pub const DEFAULT_RELAY_PORT: u16 = 7890;

/// Default WebSocket port for direct node-to-node connections.
pub const DEFAULT_NODE_PORT: u16 = 7891;

/// Heartbeat interval in seconds. Nodes that miss 3× this window are marked offline.
pub const HEARTBEAT_INTERVAL_SECS: u64 = 15;

/// Number of missed heartbeats before a node is considered offline.
pub const HEARTBEAT_OFFLINE_THRESHOLD: u32 = 3;

/// Maximum size of a single WebSocket message frame in bytes (1 MiB).
pub const MAX_FRAME_SIZE_BYTES: usize = 1024 * 1024;

/// Default database filename.
pub const DEFAULT_DB_FILENAME: &str = "dos.db";

/// Agent version string (matches Cargo.toml).
pub const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");
