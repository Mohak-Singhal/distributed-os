//! SQLite persistence layer.

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

pub mod db;
pub mod error;
pub mod repository;
pub mod sqlite;

pub use db::Database;
pub use error::StorageError;
pub use repository::{NodeRepository, SettingsRepository, TaskRecord, TaskRepository};
pub use sqlite::{
    node_repo::SqliteNodeRepository,
    settings_repo::SqliteSettingsRepository,
    task_repo::SqliteTaskRepository,
};
