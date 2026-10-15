ALTER TABLE usage_events ADD COLUMN credits REAL CHECK (credits IS NULL OR credits >= 0);
