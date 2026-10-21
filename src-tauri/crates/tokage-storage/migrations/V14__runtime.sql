-- Independent authority: no legacy rows are promoted to executable runtime runs.
CREATE TABLE runtime_runs (
    run_id TEXT PRIMARY KEY,
    revision INTEGER NOT NULL CHECK(revision > 0),
    snapshot TEXT NOT NULL
);
CREATE TABLE runtime_events (
    run_id TEXT NOT NULL REFERENCES runtime_runs(run_id),
    sequence INTEGER NOT NULL CHECK(sequence > 0),
    event TEXT NOT NULL,
    PRIMARY KEY(run_id, sequence)
);
CREATE TABLE runtime_commands (
    command_id TEXT PRIMARY KEY,
    request_hash TEXT NOT NULL,
    receipt TEXT NOT NULL
);
CREATE TABLE runtime_history (
    source_kind TEXT NOT NULL,
    source_id TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    original TEXT NOT NULL,
    PRIMARY KEY(source_kind, source_id)
);
