-- Chat logs FTS and cleanup indexes (docs/08 schema.md §3, P8.m5a).

CREATE VIRTUAL TABLE IF NOT EXISTS chat_logs_fts USING fts5(
    content,
    content='chat_logs',
    content_rowid='id',
    tokenize='trigram'
);

-- Triggers to keep FTS index synced with chat_logs table
CREATE TRIGGER IF NOT EXISTS trg_chat_logs_ai AFTER INSERT ON chat_logs BEGIN
    INSERT INTO chat_logs_fts(rowid, content) VALUES (new.id, new.content);
END;

CREATE TRIGGER IF NOT EXISTS trg_chat_logs_ad AFTER DELETE ON chat_logs BEGIN
    INSERT INTO chat_logs_fts(chat_logs_fts, rowid, content) VALUES('delete', old.id, old.content);
END;

CREATE TRIGGER IF NOT EXISTS trg_chat_logs_au AFTER UPDATE OF content ON chat_logs BEGIN
    INSERT INTO chat_logs_fts(chat_logs_fts, rowid, content) VALUES('delete', old.id, old.content);
    INSERT INTO chat_logs_fts(rowid, content) VALUES (new.id, new.content);
END;

-- Populate existing rows into FTS index
INSERT INTO chat_logs_fts(rowid, content) SELECT id, content FROM chat_logs;

-- Index for retention / idle cleanup queries
CREATE INDEX IF NOT EXISTS idx_chat_logs_created_at ON chat_logs(created_at);
