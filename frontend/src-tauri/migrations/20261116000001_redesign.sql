-- Redesign data model (spec 6.1). Never loses prep data: meeting_preps is rebuilt with every column copied.
ALTER TABLE meetings ADD COLUMN template_id TEXT;
ALTER TABLE meetings ADD COLUMN reviewed INTEGER NOT NULL DEFAULT 0;
ALTER TABLE meetings ADD COLUMN gist TEXT;
ALTER TABLE meetings ADD COLUMN started_at TEXT;
ALTER TABLE meetings ADD COLUMN ended_at TEXT;
ALTER TABLE meetings ADD COLUMN calendar_end TEXT;
ALTER TABLE meetings ADD COLUMN calendar_original_start TEXT;
ALTER TABLE meetings ADD COLUMN calendar_marker TEXT;
ALTER TABLE meetings ADD COLUMN calendar_cancelled_at TEXT;

CREATE TABLE meeting_preps_new (id INTEGER PRIMARY KEY AUTOINCREMENT,
  event_uid TEXT, event_start TEXT, event_title TEXT, template_id TEXT,
  material TEXT NOT NULL DEFAULT '', chat_json TEXT NOT NULL DEFAULT '[]', items_json TEXT NOT NULL DEFAULT '[]',
  approved_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  meeting_id TEXT UNIQUE,
  UNIQUE(event_uid, event_start));
INSERT INTO meeting_preps_new (id, event_uid, event_start, event_title, template_id, material, chat_json, items_json, approved_at, created_at, updated_at)
  SELECT id, event_uid, event_start, event_title, template_id, material, chat_json, items_json, approved_at, created_at, updated_at FROM meeting_preps;
-- AUTOINCREMENT keeps its counter in sqlite_sequence; carry it over so deleted ids are never reused.
DELETE FROM sqlite_sequence WHERE name = 'meeting_preps_new';
INSERT INTO sqlite_sequence (name, seq) SELECT 'meeting_preps_new', seq FROM sqlite_sequence WHERE name = 'meeting_preps';
DROP TABLE meeting_preps;
ALTER TABLE meeting_preps_new RENAME TO meeting_preps;

CREATE TABLE meeting_views (id TEXT PRIMARY KEY, name TEXT, position INTEGER, builtin INTEGER NOT NULL DEFAULT 0, config_json TEXT NOT NULL);
INSERT INTO meeting_views (id, name, position, builtin, config_json) VALUES
  ('all', 'All meetings', 0, 1, '{}'),
  ('week', 'This week', 1, 1, '{}'),
  ('unreviewed', 'Not reviewed', 2, 1, '{}');
CREATE TABLE dismissed_occurrences (calendar_uid TEXT NOT NULL, original_start TEXT NOT NULL, dismissed_at TEXT NOT NULL,
  PRIMARY KEY (calendar_uid, original_start));
CREATE INDEX idx_meetings_calendar ON meetings(calendar_uid, calendar_start);

UPDATE meetings SET reviewed = 1,
  template_id = (SELECT CASE WHEN json_valid(s.result) THEN json_extract(s.result, '$.template_id') END
                 FROM summary_processes s WHERE s.meeting_id = meetings.id),
  ended_at = created_at,
  started_at = COALESCE((SELECT strftime('%Y-%m-%dT%H:%M:%f+00:00', meetings.created_at, printf('-%.3f seconds', MAX(t.audio_end_time)))
                         FROM transcripts t WHERE t.meeting_id = meetings.id AND t.audio_end_time > 0 HAVING MAX(t.audio_end_time) > 0), created_at);
