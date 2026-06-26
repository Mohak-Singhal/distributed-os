//! SQLite implementation of [`SettingsRepository`].
use async_trait::async_trait;
use sqlx::Row;

use crate::{db::Database, error::StorageError, repository::SettingsRepository};

/// Concrete SQLite implementation of [`SettingsRepository`].
#[derive(Clone)]
pub struct SqliteSettingsRepository {
    db: Database,
}

impl SqliteSettingsRepository {
    /// Create a new repository backed by `db`.
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

#[async_trait]
impl SettingsRepository for SqliteSettingsRepository {
    async fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        let row = sqlx::query("SELECT value FROM settings WHERE key = ?1")
            .bind(key)
            .fetch_optional(self.db.pool())
            .await
            .map_err(StorageError::Sqlx)?;
        Ok(row.map(|r| r.get("value")))
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(self.db.pool())
        .await
        .map_err(StorageError::Sqlx)?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM settings WHERE key = ?1")
            .bind(key)
            .execute(self.db.pool())
            .await
            .map_err(StorageError::Sqlx)?;
        Ok(())
    }
}
