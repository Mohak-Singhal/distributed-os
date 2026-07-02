//! Universal Task Manager.
//!
//! Every action in the distributed OS — from a simple ping to a remote file
//! search — is modelled as a [`Task`]. This crate defines:
//!
//! - The [`Task`] trait that all task implementations must satisfy.
//! - A [`TaskQueue`] for submitting and draining tasks.
//! - A [`TaskDispatcher`] that routes tasks to the correct executor.
//!
//! Adding a new task type in future phases requires **zero** changes here.

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

pub mod dispatcher;
pub mod error;
/// Clipboard task implementation.
pub mod clipboard;
/// Ping task implementation.
pub mod ping;
pub mod queue;
/// Task registry implementation.
pub mod registry;
pub mod task;
/// Provider interfaces for platform implementations.
pub mod providers;
/// Notifications task implementation.
pub mod notifications;
/// Terminal task implementation.
pub mod terminal;
/// File transfer task implementation.
pub mod file;

pub use clipboard::ClipboardTask;
pub use dispatcher::TaskDispatcher;
pub use error::TaskError;
pub use ping::PingTask;
pub use queue::TaskQueue;
pub use registry::TaskRegistry;
pub use task::{Task, TaskContext, TaskOutput};
pub use notifications::NotificationsTask;
pub use terminal::TerminalTask;
pub use file::FileTask;

