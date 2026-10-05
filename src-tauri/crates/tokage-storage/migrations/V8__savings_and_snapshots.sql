-- Savings events and usage snapshots (docs/08 schema.md §5, P8.m3a & P8.m3b).

CREATE TABLE savings_events (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    main_session_id  TEXT REFERENCES main_sessions(id) ON DELETE SET NULL,
    plan_id          INTEGER REFERENCES plans(id) ON DELETE SET NULL,
    kind             TEXT NOT NULL CHECK (kind IN ('ASSIGNMENT','CONTEXT','DIRECT')),
    ref_id           TEXT,
    baseline_usd     REAL NOT NULL,
    actual_usd       REAL,
    settled          INTEGER NOT NULL DEFAULT 0,
    detail_json      TEXT NOT NULL DEFAULT '{}',
    created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_savings_time ON savings_events(created_at);

CREATE TABLE usage_snapshots (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_name       TEXT NOT NULL,
    window_kind         TEXT NOT NULL,          -- SESSION_5H / WEEKLY / WEEKLY_MODEL / DAILY_BUDGET
    usage_percentage    REAL,
    quota_remaining_usd REAL,
    next_reset_time     TEXT,
    raw_text            TEXT,
    data_source         TEXT NOT NULL CHECK (data_source IN ('PASSIVE','STRUCTURED','POLL','MANUAL')),
    captured_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_usage_snapshots ON usage_snapshots(provider_name, window_kind, captured_at DESC);
