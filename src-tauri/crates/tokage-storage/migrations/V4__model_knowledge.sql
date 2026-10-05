-- Knowledge base tables for model matching (docs/08 schema.md §6, P6.m1c).

CREATE TABLE provider_profiles (
    provider_name TEXT PRIMARY KEY,
    profile_json  TEXT NOT NULL,
    pack_version  TEXT NOT NULL
);

CREATE TABLE model_knowledge_base (            -- Inherent model data
    model_id          TEXT PRIMARY KEY,
    display_name      TEXT NOT NULL,
    vendor            TEXT,
    roles             TEXT NOT NULL,            -- JSON array
    skills_json       TEXT NOT NULL,            -- Domain -> score (1..5)
    price_json        TEXT NOT NULL,            -- PricePerMtok, long_context_pricing, etc.
    cache_json        TEXT,
    context_window    INTEGER NOT NULL,
    effective_context INTEGER NOT NULL,
    speed             INTEGER,
    tags              TEXT NOT NULL,            -- JSON array
    description       TEXT NOT NULL,
    meta_json         TEXT,                     -- max_output / released / benchmarks / *_estimated
    deprecated        INTEGER NOT NULL DEFAULT 0,
    pack_version      TEXT NOT NULL,
    updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE model_offerings (                  -- CLI-specific model offerings
    provider_name     TEXT NOT NULL,
    model_id          TEXT NOT NULL REFERENCES model_knowledge_base(model_id) ON DELETE CASCADE,
    cli_model_arg     TEXT NOT NULL,
    context_window    INTEGER,                  -- NULL = inherits base model value
    effective_context INTEGER,
    credit_multiplier REAL,
    skills_patch_json TEXT,                     -- Channel-level domain score patch
    verified          INTEGER NOT NULL DEFAULT 1,
    pack_version      TEXT NOT NULL,
    PRIMARY KEY (provider_name, model_id)
);

CREATE TABLE knowledge_blobs (
    key          TEXT PRIMARY KEY,              -- success_model, domains, usage_profiles, triage_rules, etc.
    value_json   TEXT NOT NULL,
    pack_version TEXT NOT NULL
);

CREATE TABLE model_overrides (
    provider_name TEXT NOT NULL DEFAULT '*',    -- '*' = all channels
    model_id      TEXT NOT NULL,
    patch_json    TEXT NOT NULL,
    updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    PRIMARY KEY (provider_name, model_id)
);
