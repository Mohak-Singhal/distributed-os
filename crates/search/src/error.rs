//! Error types for the `dos-search` crate.

use thiserror::Error;

/// Errors from search operations.
#[derive(Debug, Error)]
pub enum SearchError {
    /// The storage layer returned an error.
    #[error("storage error: {0}")]
    Storage(String),
}
