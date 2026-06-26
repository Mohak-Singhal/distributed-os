//! Error types for the `dos-task-manager` crate.

use thiserror::Error;

/// Errors arising from task management.
#[derive(Debug, Error)]
pub enum TaskError {
    /// The task queue buffer is at capacity.
    #[error("task queue is full")]
    QueueFull,

    /// The task was cancelled before it could complete.
    #[error("task cancelled")]
    Cancelled,

    /// The task failed with a user-visible message.
    #[error("task execution failed: {0}")]
    ExecutionFailed(String),

    /// A required dependency (storage, network) was unavailable.
    #[error("dependency unavailable: {0}")]
    DependencyUnavailable(String),
}
