//! SQLite implementation of [`TaskRepository`].
use async_trait::async_trait;
use sqlx::Row;
use uuid::Uuid;

use dos_core::TaskStatus;

use crate::{db::Database, error::StorageError, repository::{TaskRecord, TaskRepository}};

fn status_to_str(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Pending   => "pending",
        TaskStatus::Queued    => "queued",
        TaskStatus::Running   => "running",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed    => "failed",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn status_from_str(s: &str) -> TaskStatus {
    match s {
        "queued"    => TaskStatus::Queued,
        "running"   => TaskStatus::Running,
        "completed" => TaskStatus::Completed,
        "failed"    => TaskStatus::Failed,
        "cancelled" => TaskStatus::Cancelled,
        _           => TaskStatus::Pending,
    }
}

/// Concrete SQLite implementation of [`TaskRepository`].
#[derive(Clone)]
pub struct SqliteTaskRepository {
    db: Database,
}

impl SqliteTaskRepository {
    /// Create a new repository backed by `db`.
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

#[async_trait]
impl TaskRepository for SqliteTaskRepository {
    async fn insert(&self, record: &TaskRecord) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO task_history (id, kind, status, created_at, completed_at, error) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(record.id.to_string())
        .bind(&record.kind)
        .bind(status_to_str(record.status))
        .bind(&record.created_at)
        .bind(&record.completed_at)
        .bind(&record.error)
        .execute(self.db.pool())
        .await
        .map_err(StorageError::Sqlx)?;
        Ok(())
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: TaskStatus,
        error: Option<&str>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE task_history SET status = ?1, error = ?2 WHERE id = ?3",
        )
        .bind(status_to_str(status))
        .bind(error)
        .bind(id.to_string())
        .execute(self.db.pool())
        .await
        .map_err(StorageError::Sqlx)?;
        Ok(())
    }

    async fn list_recent(&self, limit: u32) -> Result<Vec<TaskRecord>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, kind, status, created_at, completed_at, error \
             FROM task_history ORDER BY created_at DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(self.db.pool())
        .await
        .map_err(StorageError::Sqlx)?;

        Ok(rows
            .iter()
            .map(|r| {
                let id_str: String = r.get("id");
                TaskRecord {
                    id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil()),
                    kind: r.get("kind"),
                    status: status_from_str(r.get::<&str, _>("status")),
                    created_at: r.get("created_at"),
                    completed_at: r.get("completed_at"),
                    error: r.get("error"),
                }
            })
            .collect())
    }
}
