-- Local command-list overrides (docs/08 schema.md §7, docs/11 §3.3, P2.m1b).

CREATE TABLE command_rules (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  TEXT REFERENCES projects(id) ON DELETE CASCADE,
    provider    TEXT,
    pattern     TEXT NOT NULL,
    level       TEXT NOT NULL CHECK (level IN ('read_only','project','sensitive','forbidden')),
    source      TEXT NOT NULL CHECK (source IN ('digest','user','approval')),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE UNIQUE INDEX idx_command_rules_scope
    ON command_rules(IFNULL(project_id, ''), IFNULL(provider, ''), pattern);
