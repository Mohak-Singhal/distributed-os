//! Error types for the `dos-core` crate.

use thiserror::Error;

/// Errors arising from core domain operations.
#[derive(Debug, Error)]
pub enum CoreError {
    /// A provided ID was invalid or malformed.
    #[error("invalid ID: {0}")]
    InvalidId(String),

    /// A required field was missing or empty.
    #[error("missing required field: {0}")]
    MissingField(&'static str),

    /// A value was outside the acceptable range.
    #[error("value out of range: {0}")]
    OutOfRange(String),
}
