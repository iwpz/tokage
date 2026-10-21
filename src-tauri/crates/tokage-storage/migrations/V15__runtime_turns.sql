-- Raw redacted transcript is paged separately from state events.
CREATE TABLE runtime_turn_events (
    run_id TEXT NOT NULL REFERENCES runtime_runs(run_id),
    turn_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK(sequence > 0),
    event TEXT NOT NULL,
    PRIMARY KEY(run_id,turn_id,sequence)
);
