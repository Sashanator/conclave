-- Tasks, decisions and requests extracted from each finished summary (knowledge layer, spec 6.2).
CREATE TABLE meeting_extracts (
    meeting_id TEXT PRIMARY KEY,
    decisions_json TEXT NOT NULL,
    requests_json TEXT NOT NULL,
    status TEXT NOT NULL, -- 'done' | 'failed' | 'pending'
    error TEXT,
    extracted_at TEXT NOT NULL
);

CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    meeting_id TEXT, -- NULL for a manual task
    text TEXT NOT NULL,
    owner TEXT NOT NULL, -- 'me' | 'unassigned'
    due TEXT,
    at REAL, -- seconds into the recording
    status TEXT NOT NULL, -- 'open' | 'done' | 'dismissed'
    source TEXT NOT NULL, -- 'auto' | 'manual'
    edited INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    done_at TEXT
);

CREATE INDEX idx_tasks_meeting ON tasks(meeting_id);
CREATE INDEX idx_tasks_status ON tasks(status);
