CREATE TABLE briefs (id TEXT PRIMARY KEY, event_uid TEXT NOT NULL, event_start TEXT NOT NULL, title TEXT NOT NULL,
  template_id TEXT NOT NULL, previous_meeting_id TEXT NOT NULL, content_json TEXT NOT NULL, created_at TEXT NOT NULL,
  UNIQUE(event_uid, event_start));
