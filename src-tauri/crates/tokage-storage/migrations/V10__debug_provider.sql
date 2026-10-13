-- Execution transport and debug provenance are independent. Preserve existing attempts.
ALTER TABLE attempts ADD COLUMN debug_output TEXT
    CHECK (debug_output IS NULL OR debug_output IN ('mock', 'luna', 'deepseek'));
ALTER TABLE attempts ADD COLUMN requested_provider TEXT;
ALTER TABLE attempts ADD COLUMN requested_model TEXT;
