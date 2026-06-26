//! Core domain models for the Personal Distributed OS.
//!
//! This crate is the root of the internal dependency graph — it has **no**
//! internal crate dependencies. All other crates may depend on this one.

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

pub mod capability;
pub mod error;
pub mod node;
pub mod status;
pub mod task;

pub use capability::Capability;
pub use error::CoreError;
pub use node::{Node, Platform};
pub use status::NodeStatus;
pub use task::TaskStatus;
