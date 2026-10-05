-- Approvals audit table (docs/08 schema.md §7, docs/11 §8, P2.m3f).

CREATE TABLE approvals (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id  TEXT REFERENCES attempts(id) ON DELETE SET NULL,
    session_id  TEXT REFERENCES main_sessions(id) ON DELETE CASCADE,
    turn_id     TEXT,
    command     TEXT NOT NULL,
    level       TEXT NOT NULL,
    reason      TEXT,
    decision    TEXT NOT NULL CHECK (decision IN ('allow_once','allow_session','allow_project','deny')),
    mode        TEXT NOT NULL,
    decided_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_approvals_attempt ON approvals(attempt_id);
CREATE INDEX idx_approvals_session ON approvals(session_id);
CREATE INDEX idx_approvals_turn ON approvals(turn_id);
