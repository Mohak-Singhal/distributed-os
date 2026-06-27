//! Task registry for mapping requests to executable tasks.

use std::collections::HashMap;

use dos_protocol::message::TaskRequest;

use crate::{Task, TaskError};

/// A factory function that creates a `Task` from a `TaskRequest`.
pub type TaskFactory = Box<dyn Fn(TaskRequest) -> Result<Box<dyn Task>, TaskError> + Send + Sync>;

/// A registry that maps task kinds to their factories.
#[derive(Default)]
pub struct TaskRegistry {
    factories: HashMap<String, TaskFactory>,
}

impl TaskRegistry {
    /// Create a new, empty task registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a factory for a given task kind.
    pub fn register<F>(&mut self, kind: impl Into<String>, factory: F)
    where
        F: Fn(TaskRequest) -> Result<Box<dyn Task>, TaskError> + Send + Sync + 'static,
    {
        self.factories.insert(kind.into(), Box::new(factory));
    }

    /// Instantiate a task from a task request.
    pub fn create_task(&self, request: TaskRequest) -> Result<Box<dyn Task>, TaskError> {
        if let Some(factory) = self.factories.get(&request.kind) {
            factory(request)
        } else {
            Err(TaskError::ExecutionFailed(format!("No task handler registered for kind: {}", request.kind)))
        }
    }
}
