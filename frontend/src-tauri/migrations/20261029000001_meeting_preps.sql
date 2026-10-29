CREATE TABLE meeting_preps (id INTEGER PRIMARY KEY AUTOINCREMENT,
  event_uid TEXT NOT NULL, event_start TEXT NOT NULL, event_title TEXT NOT NULL, template_id TEXT,
  material TEXT NOT NULL DEFAULT '', chat_json TEXT NOT NULL DEFAULT '[]', items_json TEXT NOT NULL DEFAULT '[]',
  approved_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  UNIQUE(event_uid, event_start));
