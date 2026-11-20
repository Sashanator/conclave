-- Tasks review queue → Notion (spec 2026-11-20 §5.1, §5.2). Additive: status/owner keep their meaning.
ALTER TABLE tasks ADD COLUMN review TEXT NOT NULL DEFAULT 'open'; -- open | keep | drop | sending | sent
ALTER TABLE tasks ADD COLUMN removed_by TEXT;  -- ai | user | NULL
ALTER TABLE tasks ADD COLUMN dest_id TEXT;     -- notion_destinations.id; NULL = none chosen
ALTER TABLE tasks ADD COLUMN due_date TEXT;    -- YYYY-MM-DD
ALTER TABLE tasks ADD COLUMN notion_page_id TEXT;
ALTER TABLE tasks ADD COLUMN notion_url TEXT;
ALTER TABLE tasks ADD COLUMN sent_at TEXT;
ALTER TABLE tasks ADD COLUMN send_error TEXT;
ALTER TABLE tasks ADD COLUMN send_ambiguous INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN meeting_title TEXT; -- copied when the meeting link is cleared
ALTER TABLE tasks ADD COLUMN meeting_date TEXT;

CREATE TABLE notion_destinations (id TEXT PRIMARY KEY, name TEXT NOT NULL, colour INTEGER NOT NULL, -- 1..9
  position INTEGER NOT NULL, database_id TEXT NOT NULL, data_source_id TEXT, mapping_json TEXT NOT NULL,
  created_at TEXT NOT NULL, last_status TEXT, last_message TEXT); -- last_*: last Test/send outcome, NULL = never tested
-- template_key: a template id, or '' for "No template". dest_id NULL = "Ask me".
CREATE TABLE notion_category_defaults (template_key TEXT PRIMARY KEY, dest_id TEXT);

-- Existing tasks (§5.2). dest_id stays NULL: no default exists yet; setting one later fills open/keep tasks.
UPDATE tasks SET
  review = CASE
    WHEN status = 'open' AND owner = 'unassigned' THEN 'drop'
    WHEN status = 'open' AND meeting_id IS NULL AND source = 'auto' THEN 'open'
    WHEN status = 'open' AND EXISTS (SELECT 1 FROM meetings m WHERE m.id = tasks.meeting_id AND m.reviewed = 0) THEN 'open'
    ELSE 'drop' END,
  removed_by = CASE WHEN status = 'open' AND owner = 'unassigned' THEN 'ai' END;

-- Nothing left to review → reviewed (spec §5.7), once for existing meetings with a finished extraction.
UPDATE meetings SET reviewed = 1
  WHERE reviewed = 0
    AND EXISTS (SELECT 1 FROM meeting_extracts x WHERE x.meeting_id = meetings.id AND x.status = 'done')
    AND NOT EXISTS (SELECT 1 FROM tasks t WHERE t.meeting_id = meetings.id AND t.review = 'open');
