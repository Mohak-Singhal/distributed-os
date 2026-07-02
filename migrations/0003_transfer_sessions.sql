-- Migration 0003: Transfer sessions table with full telemetry
CREATE TABLE IF NOT EXISTS transfer_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    filename TEXT NOT NULL,
    file_type TEXT NOT NULL DEFAULT '',
    file_extension TEXT NOT NULL DEFAULT '',
    original_size INTEGER NOT NULL,
    compressed_size INTEGER,
    compression_ratio REAL,
    compression_time_ms INTEGER,
    cpu_used_compression REAL,
    bandwidth_saved INTEGER,
    time_saved_sec REAL,

    sha256 TEXT,

    start_time TEXT NOT NULL,
    end_time TEXT,
    duration_secs REAL,

    average_speed_mbps REAL NOT NULL DEFAULT 0.0,
    peak_speed_mbps REAL NOT NULL DEFAULT 0.0,
    min_speed_mbps REAL NOT NULL DEFAULT 0.0,
    median_speed_mbps REAL NOT NULL DEFAULT 0.0,
    p95_speed_mbps REAL NOT NULL DEFAULT 0.0,

    average_rtt_ms REAL NOT NULL DEFAULT 0.0,
    peak_rtt_ms REAL NOT NULL DEFAULT 0.0,
    packet_loss_pct REAL NOT NULL DEFAULT 0.0,
    retransmissions INTEGER NOT NULL DEFAULT 0,
    reconnects INTEGER NOT NULL DEFAULT 0,

    disk_read_mbps REAL NOT NULL DEFAULT 0.0,
    disk_write_mbps REAL NOT NULL DEFAULT 0.0,
    disk_queue_depth REAL NOT NULL DEFAULT 0.0,
    disk_flush_latency_ms REAL NOT NULL DEFAULT 0.0,
    bytes_buffered INTEGER NOT NULL DEFAULT 0,

    average_cpu_pct REAL NOT NULL DEFAULT 0.0,
    peak_cpu_pct REAL NOT NULL DEFAULT 0.0,
    average_ram_mb REAL NOT NULL DEFAULT 0.0,
    peak_ram_mb REAL NOT NULL DEFAULT 0.0,

    completed INTEGER NOT NULL DEFAULT 0,
    verified INTEGER NOT NULL DEFAULT 0,
    resumed INTEGER NOT NULL DEFAULT 0,
    interrupted INTEGER NOT NULL DEFAULT 0,
    error TEXT,

    health_score REAL,
    bottleneck TEXT,
    recommendation TEXT
);

CREATE TABLE IF NOT EXISTS transfer_phases (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    name TEXT NOT NULL,
    start_ms REAL NOT NULL,
    end_ms REAL,
    duration_ms REAL,
    FOREIGN KEY (session_id) REFERENCES transfer_sessions(id)
);

CREATE TABLE IF NOT EXISTS protocol_counters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    counter_name TEXT NOT NULL,
    value INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS thermal_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    cpu_temp_c REAL,
    thermal_state TEXT,
    fan_rpm REAL,
    battery_pct REAL
);

CREATE TABLE IF NOT EXISTS network_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    interface_name TEXT NOT NULL,
    ip_address TEXT,
    rssi REAL,
    link_speed REAL,
    tx_bytes INTEGER NOT NULL DEFAULT 0,
    rx_bytes INTEGER NOT NULL DEFAULT 0,
    signal_event TEXT NOT NULL DEFAULT 'stable'
);
