//! Universal search — v0.1 searches registered devices only.
//!
//! The search API is intentionally decoupled from UI and networking.
//! Queries are plain strings; results are ranked by a simple scoring function
//! that can be replaced without changing the API contract.

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

pub mod engine;
pub mod error;
pub mod query;

pub use engine::SearchEngine;
pub use error::SearchError;
pub use query::{SearchQuery, SearchResult};
