-- Learning and outcomes tables (docs/08 schema.md §4 & §6, P6.m6a).

CREATE TABLE task_outcomes (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id         TEXT,                               -- Task item ID (T1, T2, etc.) or NULL
    model_id        TEXT NOT NULL,
    provider_name   TEXT NOT NULL,
    domain          TEXT NOT NULL,
    difficulty      INTEGER NOT NULL,
    size            TEXT NOT NULL,
    result          TEXT NOT NULL CHECK (result IN ('FIRST_PASS','FIXED_IN_WINDOW','REASSIGNED','BLOCKED','USER_TAKEOVER')),
    p_predicted     REAL NOT NULL,
    est_cost_usd    REAL,
    actual_cost_usd REAL,
    turns           INTEGER,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_outcomes_model_domain ON task_outcomes(model_id, domain, difficulty);
CREATE INDEX idx_outcomes_offering ON task_outcomes(provider_name, model_id, domain, difficulty);

CREATE TABLE learned_stats (
    provider_name TEXT NOT NULL DEFAULT '*',     -- '*' = model-level, aggregated across all providers (docs/04 §4)
    model_id      TEXT NOT NULL DEFAULT '*',
    domain        TEXT NOT NULL DEFAULT '*',
    bucket        TEXT NOT NULL DEFAULT '*',     -- Difficulty or size, e.g. 'd2', 'M'
    metric        TEXT NOT NULL,                 -- success_alpha / success_beta / fix_rate / turns / calls_per_turn / growth_per_turn / output_per_turn / cost_calibration
    value         REAL NOT NULL,
    samples       INTEGER NOT NULL,
    updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    PRIMARY KEY (provider_name, model_id, domain, bucket, metric)
);
