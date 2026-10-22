CREATE TABLE llm_usage (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  at TEXT NOT NULL,              -- RFC 3339 UTC
  provider TEXT NOT NULL, model TEXT NOT NULL, purpose TEXT NOT NULL,
  meeting_id TEXT, recording_id TEXT,
  input_tokens INTEGER, output_tokens INTEGER, cache_read_tokens INTEGER, cache_write_tokens INTEGER,
  cost_usd REAL
);
CREATE INDEX idx_llm_usage_at ON llm_usage(at);
CREATE INDEX idx_llm_usage_meeting ON llm_usage(meeting_id);
