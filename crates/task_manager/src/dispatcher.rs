//! Task dispatcher — drains the queue and executes tasks.

use tracing::{error, info, instrument};

use crate::{queue::TaskQueueReceiver, TaskContext, TaskError};

/// Drains the [`TaskQueue`] and dispatches each task to its executor.
///
/// Run [`TaskDispatcher::run`] in a dedicated tokio task.
pub struct TaskDispatcher {
    receiver: TaskQueueReceiver,
    context: TaskContext,
}

impl TaskDispatcher {
    /// Create a dispatcher that drains `receiver` using `context`.
    pub fn new(receiver: TaskQueueReceiver, context: TaskContext) -> Self {
        Self { receiver, context }
    }

    /// Start draining the queue. Runs until the sender is dropped.
    pub async fn run(mut self) {
        info!("task dispatcher started");
        while let Some(task) = self.receiver.receiver.recv().await {
            let ctx = self.context.clone();
            tokio::spawn(async move {
                Self::execute_task(task.as_ref(), &ctx).await;
            });
        }
        info!("task dispatcher stopped — queue closed");
    }

    #[instrument(skip(task, ctx), fields(task_id = %task.id(), kind = task.kind()))]
    async fn execute_task(task: &dyn crate::Task, ctx: &TaskContext) {
        info!("task started");
        match task.execute(ctx).await {
            Ok(output) => {
                info!(result = ?output.result, "task completed");
            }
            Err(TaskError::Cancelled) => {
                info!("task cancelled");
            }
            Err(e) => {
                error!(error = %e, "task failed");
            }
        }
    }
}
