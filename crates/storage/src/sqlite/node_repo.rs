//! SQLite implementation of [`NodeRepository`].
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::Row;
use uuid::Uuid;

use dos_core::Node;

use crate::{
    db::Database,
    error::StorageError,
    repository::NodeRepository,
    sqlite::{
        capabilities_from_json, capabilities_to_json, platform_from_str, status_from_str,
    },
};

/// Concrete SQLite implementation of [`NodeRepository`].
#[derive(Clone)]
pub struct SqliteNodeRepository {
    db: Database,
}

impl SqliteNodeRepository {
    /// Create a new repository backed by `db`.
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

#[async_trait]
impl NodeRepository for SqliteNodeRepository {
    async fn upsert(&self, node: &Node) -> Result<(), StorageError> {
        let caps_json = capabilities_to_json(&node.capabilities);
        let last_seen = node.last_seen.map(|t| t.to_rfc3339());
        sqlx::query(
            r#"
            INSERT INTO nodes (id, name, platform, capabilities, status, last_seen, public_key, version)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                name         = excluded.name,
                platform     = excluded.platform,
                capabilities = excluded.capabilities,
                status       = excluded.status,
                last_seen    = excluded.last_seen,
                public_key   = excluded.public_key,
                version      = excluded.version
            "#,
        )
        .bind(node.id.to_string())
        .bind(&node.name)
        .bind(node.platform.to_string())
        .bind(&caps_json)
        .bind(node.status.to_string())
        .bind(last_seen)
        .bind(&node.public_key)
        .bind(&node.version)
        .execute(self.db.pool())
        .await
        .map_err(StorageError::Sqlx)?;
        Ok(())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Node>, StorageError> {
        let row = sqlx::query(
            "SELECT id, name, platform, capabilities, status, last_seen, public_key, version \
             FROM nodes WHERE id = ?1",
        )
        .bind(id.to_string())
        .fetch_optional(self.db.pool())
        .await
        .map_err(StorageError::Sqlx)?;

        Ok(row.map(|r| row_to_node(&r)))
    }

    async fn list_all(&self) -> Result<Vec<Node>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, name, platform, capabilities, status, last_seen, public_key, version \
             FROM nodes ORDER BY name ASC",
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(StorageError::Sqlx)?;

        Ok(rows.iter().map(row_to_node).collect())
    }

    async fn delete(&self, id: Uuid) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM nodes WHERE id = ?1")
            .bind(id.to_string())
            .execute(self.db.pool())
            .await
            .map_err(StorageError::Sqlx)?;
        Ok(())
    }
}

fn row_to_node(row: &sqlx::sqlite::SqliteRow) -> Node {
    let id_str: String = row.get("id");
    let id = Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil());
    let caps_json: String = row.get("capabilities");
    let platform_str: String = row.get("platform");
    let status_str: String = row.get("status");
    let last_seen_str: Option<String> = row.get("last_seen");

    let mut node = Node::new(
        id,
        row.get::<String, _>("name"),
        platform_from_str(&platform_str),
        capabilities_from_json(&caps_json),
        row.get::<String, _>("public_key"),
        row.get::<String, _>("version"),
    );
    node.status = status_from_str(&status_str);
    node.last_seen = last_seen_str
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    node
}
