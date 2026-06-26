//! Error types for the `dos-common` crate.

use thiserror::Error;

/// Errors from common utility operations.
#[derive(Debug, Error)]
pub enum CommonError {
    /// The config file could not be read from disk.
    #[error("failed to read config file: {0}")]
    ConfigRead(String),

    /// The config file contained invalid TOML.
    #[error("failed to parse config: {0}")]
    ConfigParse(String),

    /// A required environment variable was not set.
    #[error("missing environment variable: {0}")]
    MissingEnvVar(&'static str),
}
