//! Shared utilities for the Personal Distributed OS.
//!
//! This crate contains cross-cutting concerns that would otherwise be
//! duplicated or invented independently across crates:
//!
//! - System-wide constants
//! - Application configuration loading (TOML)
//! - Time utilities
//! - A common `Result` alias

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

pub mod config;
pub mod constants;
pub mod error;
pub mod time;

pub use error::CommonError;

/// Convenience `Result` alias using [`CommonError`].
pub type Result<T> = std::result::Result<T, CommonError>;
