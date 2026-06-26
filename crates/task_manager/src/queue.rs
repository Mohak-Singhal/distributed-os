//! In-memory task queue backed by a tokio channel.

use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::debug;

use crate::{Task, TaskError};

/// An async MPSC queue for submitted tasks.
///
/// Producers call [`TaskQueue::submit`]; the [`crate::TaskDispatcher`] drains
/// the receiving end.
pub struct TaskQueue {
    sender: mpsc::Sender<Arc<dyn Task>>,
}

impl TaskQueue {
    /// Create a new queue with the given buffer capacity and return the
    /// matching [`TaskQueueReceiver`].
    pub fn new(capacity: usize) -> (Self, TaskQueueReceiver) {
        let (tx, rx) = mpsc::channel(capacity);
        (Self { sender: tx }, TaskQueueReceiver { receiver: rx })
    }

    /// Submit a task to the queue.
    ///
    /// # Errors
    /// Returns [`TaskError::QueueFull`] if the channel buffer is at capacity.
    pub async fn submit(&self, task: Arc<dyn Task>) -> Result<(), TaskError> {
        debug!(task_id = %task.id(), kind = task.kind(), "task submitted");
        self.sender.send(task).await.map_err(|_| TaskError::QueueFull)
    }
}

/// The receiving end of the [`TaskQueue`]. Owned by the [`crate::TaskDispatcher`].
pub struct TaskQueueReceiver {
    pub(crate) receiver: mpsc::Receiver<Arc<dyn Task>>,
}
