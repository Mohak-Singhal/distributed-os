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
        while let Some((task, origin)) = self.receiver.receiver.recv().await {
            let mut ctx = self.context.clone();
            ctx.origin = origin;
            tokio::spawn(async move {
                Self::execute_task(task.as_ref(), &ctx).await;
            });
        }
        info!("task dispatcher stopped — queue closed");
    }

    #[instrument(skip(task, ctx), fields(task_id = %task.id(), kind = task.kind()))]
    async fn execute_task(task: &dyn crate::Task, ctx: &TaskContext) {
        info!("task started");
        let result = task.execute(ctx).await;
        
        match &result {
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

        // Send the result back if a channel is provided
        if let Some(tx) = &ctx.result_tx {
            let _ = tx.send((task.id(), ctx.origin, result));
        }
    }
}
