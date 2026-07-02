-- Migration 0002: Add file transfers table
CREATE TABLE IF NOT EXISTS file_transfers (
    id TEXT PRIMARY KEY NOT NULL,
    filename TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    direction TEXT NOT NULL CHECK(direction IN ('send', 'receive')),
    device_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'in_progress', 'completed', 'failed', 'cancelled'))
);
