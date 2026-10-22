CREATE TABLE feed_items (id TEXT PRIMARY KEY, kind TEXT NOT NULL /* 'brief'|'digest' */, title TEXT NOT NULL,
  target_id TEXT NOT NULL, created_at TEXT NOT NULL, read_at TEXT);
CREATE INDEX idx_feed_created ON feed_items(created_at);
