//! Repository traits for each domain entity.
//!
//! Concrete sqlx implementations live in submodules. Callers depend only on
//! the traits, making the storage layer easy to mock in tests.

use async_trait::async_trait;
use uuid::Uuid;

use dos_core::Node;
use dos_core::TaskStatus;

use crate::StorageError;

// ── Node Repository ───────────────────────────────────────────────────────────

/// Persistent store for [`Node`] records.
#[async_trait]
pub trait NodeRepository: Send + Sync {
    /// Upsert a node record (insert or update by `id`).
    async fn upsert(&self, node: &Node) -> Result<(), StorageError>;

    /// Find a node by its UUID.
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Node>, StorageError>;

    /// Return all known nodes.
    async fn list_all(&self) -> Result<Vec<Node>, StorageError>;

    /// Remove a node record permanently.
    async fn delete(&self, id: Uuid) -> Result<(), StorageError>;
}

// ── Task Repository ───────────────────────────────────────────────────────────

/// A persisted task record (lightweight summary, not the full payload).
#[derive(Debug, Clone)]
pub struct TaskRecord {
    /// Task UUID.
    pub id: Uuid,
    /// Task kind string (e.g. `"ping"`, `"search"`).
    pub kind: String,
    /// Current status.
    pub status: TaskStatus,
    /// ISO-8601 creation timestamp.
    pub created_at: String,
    /// ISO-8601 completion timestamp, or `None`.
    pub completed_at: Option<String>,
    /// JSON-encoded error, or `None` on success.
    pub error: Option<String>,
}

/// Persistent store for task history.
#[async_trait]
pub trait TaskRepository: Send + Sync {
    /// Insert a new task record.
    async fn insert(&self, record: &TaskRecord) -> Result<(), StorageError>;

    /// Update the status and completion timestamp of an existing task.
    async fn update_status(
        &self,
        id: Uuid,
        status: TaskStatus,
        error: Option<&str>,
    ) -> Result<(), StorageError>;

    /// Return the N most recent task records, newest first.
    async fn list_recent(&self, limit: u32) -> Result<Vec<TaskRecord>, StorageError>;
}

// ── Settings Repository ───────────────────────────────────────────────────────

/// Key-value settings store (TOML values serialised as JSON strings).
#[async_trait]
pub trait SettingsRepository: Send + Sync {
    /// Read a setting by key. Returns `None` if the key does not exist.
    async fn get(&self, key: &str) -> Result<Option<String>, StorageError>;

    /// Write or overwrite a setting.
    async fn set(&self, key: &str, value: &str) -> Result<(), StorageError>;

    /// Delete a setting.
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
}
