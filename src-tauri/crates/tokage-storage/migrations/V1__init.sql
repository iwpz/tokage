-- First tables (docs/08 schema.md, P1.m3c). Columns pointing at tables that don't exist yet
-- (tasks, plans) carry no foreign key; it is added when those tables are created.

CREATE TABLE projects (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    local_path     TEXT NOT NULL UNIQUE,
    position       INTEGER NOT NULL DEFAULT 0,
    expanded       INTEGER NOT NULL DEFAULT 1,
    trusted        INTEGER,
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    last_opened_at TEXT
);

CREATE TABLE main_sessions (
    id           TEXT PRIMARY KEY,
    project_id   TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title        TEXT NOT NULL,
    route        TEXT CHECK (route IN ('PLAN','DIRECT')),
    status       TEXT NOT NULL DEFAULT 'IDLE'
                 CHECK (status IN ('PLANNING','REVIEW','RUNNING','PAUSED','BLOCKED','IDLE','COMPLETED')),
    position     INTEGER NOT NULL DEFAULT 0,
    archived     INTEGER NOT NULL DEFAULT 0,
    forked_from  TEXT REFERENCES main_sessions(id) ON DELETE SET NULL,
    worktree_json TEXT,
    thread_json  TEXT,
    queue_json   TEXT NOT NULL DEFAULT '[]',
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_main_sessions_project ON main_sessions(project_id, position);

CREATE TABLE attempts (
    id                  TEXT PRIMARY KEY,
    main_session_id     TEXT NOT NULL REFERENCES main_sessions(id) ON DELETE CASCADE,
    task_id             INTEGER,
    role                TEXT NOT NULL CHECK (role IN ('PLANNER','EXECUTOR','TESTER','REVIEWER')),
    provider_name       TEXT NOT NULL,
    model_id            TEXT NOT NULL,
    cli_window_identity TEXT,
    reused_from_id      TEXT REFERENCES attempts(id) ON DELETE SET NULL,
    predecessor_id      TEXT REFERENCES attempts(id) ON DELETE SET NULL,
    exec_mode           TEXT NOT NULL DEFAULT 'HEADLESS' CHECK (exec_mode IN ('HEADLESS','PTY')),
    workdir             TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'STARTING',
    baseline_tokens     INTEGER NOT NULL DEFAULT 0,
    context_tokens      INTEGER NOT NULL DEFAULT 0,
    token_source        TEXT NOT NULL DEFAULT 'ESTIMATED' CHECK (token_source IN ('EXACT','ESTIMATED')),
    turn_count          INTEGER NOT NULL DEFAULT 0,
    last_turn_ended_at  TEXT,
    touched_files_json  TEXT NOT NULL DEFAULT '[]',
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    ended_at            TEXT
);
CREATE INDEX idx_attempts_task ON attempts(task_id);
CREATE INDEX idx_attempts_main ON attempts(main_session_id);

CREATE TABLE chat_logs (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id        TEXT NOT NULL UNIQUE,
    main_session_id   TEXT NOT NULL REFERENCES main_sessions(id) ON DELETE CASCADE,
    attempt_id        TEXT NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
    seq               INTEGER NOT NULL,
    turn_index        INTEGER NOT NULL,
    role              TEXT NOT NULL CHECK (role IN ('USER','ASSISTANT','SYSTEM','TOOL','SHELL_STDOUT','BRIEF')),
    content           TEXT NOT NULL,
    content_truncated INTEGER NOT NULL DEFAULT 0,
    meta_json         TEXT NOT NULL DEFAULT '{}',
    created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_chat_logs_attempt ON chat_logs(attempt_id, id);
CREATE INDEX idx_chat_logs_session ON chat_logs(main_session_id, seq);

CREATE TABLE usage_events (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id         TEXT REFERENCES attempts(id) ON DELETE SET NULL,
    task_id            INTEGER,
    plan_id            INTEGER,
    provider_name      TEXT NOT NULL,
    model_id           TEXT,
    role               TEXT,
    domain             TEXT,
    turn_index         INTEGER,
    input_tokens       INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens  INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens      INTEGER NOT NULL DEFAULT 0,
    api_calls          INTEGER,
    context_tokens     INTEGER,
    gap_seconds        INTEGER,
    cost_usd           REAL,
    equiv_cost_usd     REAL,
    phase              TEXT NOT NULL CHECK (phase IN ('PLANNING','EXECUTION','REPAIR','CONTEXT','DIRECT')),
    source             TEXT NOT NULL CHECK (source IN ('STRUCTURED','STDOUT_REGEX','ESTIMATED')),
    created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_usage_events_time ON usage_events(provider_name, created_at);
CREATE INDEX idx_usage_events_plan ON usage_events(plan_id);

CREATE TABLE provider_health (
    provider_name     TEXT PRIMARY KEY,
    position          INTEGER NOT NULL DEFAULT 0,
    status            TEXT NOT NULL,
    cli_version       TEXT,
    info_json         TEXT NOT NULL,
    melt_reason       TEXT,
    melted_until      TEXT,
    consecutive_fails INTEGER NOT NULL DEFAULT 0,
    updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE custom_packs (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    name          TEXT NOT NULL,
    source_url    TEXT,
    position      INTEGER NOT NULL,
    pack_json     TEXT NOT NULL,
    warnings_json TEXT NOT NULL DEFAULT '[]',
    enabled       INTEGER NOT NULL DEFAULT 1,
    imported_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE settings (
    key        TEXT PRIMARY KEY,
    value_json TEXT NOT NULL
);
