-- Weekly digests (knowledge layer, spec 6.3). A period summary, so it belongs to no one meeting and meeting delete keeps it.
CREATE TABLE digests (
    id TEXT PRIMARY KEY,
    period_start TEXT NOT NULL UNIQUE,
    period_end TEXT NOT NULL,
    markdown TEXT NOT NULL,
    meeting_ids_json TEXT NOT NULL, -- array; index i is the meeting for tag M(i+1)
    created_at TEXT NOT NULL
);
