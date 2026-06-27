//! Core [`Task`] trait and supporting types.

use uuid::Uuid;

use dos_core::TaskStatus;

use crate::TaskError;

/// Output produced by a completed task.
#[derive(Debug, Clone)]
pub struct TaskOutput {
    /// Machine-readable JSON result, or `serde_json::Value::Null` on failure.
    pub result: serde_json::Value,
}

/// Context injected into every task at dispatch time.
///
/// Provides access to shared services (storage, networking) without requiring
/// tasks to hold their own Arc references.
#[derive(Clone)]
pub struct TaskContext {
    /// The ID of the node running this task.
    pub node_id: Uuid,
    /// The original requester node, if this task was triggered over the network.
    pub origin: Option<Uuid>,
    /// Optional channel to send the completion result back to the network layer.
    /// Format: `(task_id, origin, Result)`
    #[allow(clippy::type_complexity)]
    pub result_tx: Option<tokio::sync::mpsc::UnboundedSender<(Uuid, Option<Uuid>, Result<crate::TaskOutput, crate::TaskError>)>>,
}

/// The universal task abstraction.
///
/// Every action — ping, heartbeat, search — implements this trait. The
/// dispatcher calls `execute` and persists the result automatically.
///
/// # Example
/// ```ignore
/// struct PingTask { target: NodeId }
///
/// #[async_trait::async_trait]
/// impl Task for PingTask {
///     fn id(&self) -> Uuid { self.id }
///     fn kind(&self) -> &'static str { "ping" }
///     async fn execute(&self, ctx: &TaskContext) -> Result<TaskOutput, TaskError> {
///         // … send ping …
///         Ok(TaskOutput { result: serde_json::json!({ "latency_ms": 12 }) })
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait Task: Send + Sync {
    /// Unique ID for this task instance.
    fn id(&self) -> Uuid;

    /// Short machine-readable task kind (e.g. `"ping"`, `"search"`).
    fn kind(&self) -> &'static str;

    /// Current lifecycle status.
    fn status(&self) -> TaskStatus;

    /// Execute the task and return its output.
    ///
    /// # Errors
    /// Returns [`TaskError`] on any non-recoverable failure.
    async fn execute(&self, ctx: &TaskContext) -> Result<TaskOutput, TaskError>;
}
