//! Database handle and connection pool management.

use sqlx::SqlitePool;
use tracing::info;

use crate::StorageError;

/// A handle to the SQLite connection pool.
///
/// Cheaply cloneable — all clones share the same underlying pool.
#[derive(Debug, Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Open (or create) the SQLite database at `path` and run all pending migrations.
    ///
    /// # Errors
    /// Returns [`StorageError`] if the database cannot be opened or migrations fail.
    pub async fn open(path: &str) -> Result<Self, StorageError> {
        use sqlx::sqlite::SqliteConnectOptions;
        
        // Remove "sqlite:" prefix if present to get raw file path
        let raw_path = path.strip_prefix("sqlite:").unwrap_or(path);
        
        let options = SqliteConnectOptions::new()
            .filename(raw_path)
            .create_if_missing(true);
            
        let pool = SqlitePool::connect_with(options).await.map_err(StorageError::Sqlx)?;
        sqlx::migrate!("../../migrations").run(&pool).await.map_err(StorageError::Migration)?;
        info!(path = %raw_path, "database opened and migrations applied");
        Ok(Self { pool })
    }

    /// Return a reference to the underlying connection pool.
    ///
    /// Only `repository` modules should call this directly.
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
