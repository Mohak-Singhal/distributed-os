//! Task lifecycle status.
//!
//! The full [`Task`] trait lives in `dos-task-manager`. This module only holds
//! the status enum so lower-level crates can reference task state without
//! pulling in the task-manager dependency.

use serde::{Deserialize, Serialize};

/// The lifecycle state of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Created but not yet submitted to the queue.
    #[default]
    Pending,
    /// Accepted and waiting for a free executor slot.
    Queued,
    /// Currently running.
    Running,
    /// Completed successfully.
    Completed,
    /// Failed with an error.
    Failed,
    /// Cancelled before completion.
    Cancelled,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        };
        write!(f, "{s}")
    }
}
