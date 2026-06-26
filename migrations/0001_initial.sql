-- Migration 0001: initial schema
-- All tables use TEXT primary keys (UUID as string) for SQLite portability.

CREATE TABLE IF NOT EXISTS nodes (
    id          TEXT    PRIMARY KEY NOT NULL,
    name        TEXT    NOT NULL,
    platform    TEXT    NOT NULL,
    capabilities TEXT   NOT NULL DEFAULT '[]',  -- JSON array
    status      TEXT    NOT NULL DEFAULT 'offline',
    last_seen   TEXT,                            -- ISO-8601 or NULL
    public_key  TEXT    NOT NULL,
    version     TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS trusted_keys (
    node_id     TEXT    PRIMARY KEY NOT NULL,
    public_key  TEXT    NOT NULL,
    trusted_at  TEXT    NOT NULL                 -- ISO-8601
);

CREATE TABLE IF NOT EXISTS task_history (
    id           TEXT PRIMARY KEY NOT NULL,
    kind         TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending',
    created_at   TEXT NOT NULL,
    completed_at TEXT,
    error        TEXT
);

CREATE TABLE IF NOT EXISTS settings (
    key     TEXT PRIMARY KEY NOT NULL,
    value   TEXT NOT NULL
);
