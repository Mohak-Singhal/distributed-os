//! Error types for the `dos-storage` crate.

use thiserror::Error;

/// Errors from storage operations.
#[derive(Debug, Error)]
pub enum StorageError {
    /// An `sqlx` database error.
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    /// A migration failed to apply.
    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    /// A stored record could not be deserialised.
    #[error("deserialisation error: {0}")]
    Deserialise(String),

    /// A record that was expected to exist was not found.
    #[error("record not found: {0}")]
    NotFound(String),
}
