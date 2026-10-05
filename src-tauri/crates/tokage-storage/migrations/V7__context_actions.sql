-- Context actions table for in-window context optimizations and settlement (docs/08 schema.md §4, P8.m1f).

CREATE TABLE context_actions (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id        TEXT NOT NULL,
    kind              TEXT NOT NULL CHECK (kind IN ('CONTINUE','COMPACT','FRESH','REUSE')),
    trigger           TEXT NOT NULL CHECK (trigger IN ('PLANNER','HARD_LIMIT','USER')),
    status            TEXT NOT NULL DEFAULT 'SUGGESTED'
                      CHECK (status IN ('SUGGESTED','APPLIED','DISMISSED','UNDONE','FAILED')),
    est_costs_json    TEXT NOT NULL,
    est_saving_pct    REAL NOT NULL,
    actual_saving_usd REAL,
    new_attempt_id    TEXT,
    created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    applied_at        TEXT
);
CREATE INDEX idx_context_actions_attempt ON context_actions(attempt_id, id);
