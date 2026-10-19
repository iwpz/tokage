-- New native Done receipts are deduplicated without rewriting legacy billing rows.
CREATE TABLE usage_receipts (
    event_key TEXT PRIMARY KEY,
    usage_id INTEGER NOT NULL REFERENCES usage_events(id) ON DELETE CASCADE,
    payload_json TEXT NOT NULL
);
-- Crash recovery cannot infer that a launch with no billable Done was free.
CREATE TABLE recovery_usage_gaps (
    turn_id TEXT PRIMARY KEY,
    plan_id INTEGER NOT NULL REFERENCES plans(id) ON DELETE CASCADE
);
