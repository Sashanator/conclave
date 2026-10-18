CREATE TABLE people (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  is_self INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX people_one_self ON people(is_self) WHERE is_self = 1;
INSERT INTO people (id, name, is_self, created_at) VALUES ('self', 'You', 1, datetime('now'));

CREATE TABLE meeting_speakers (
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  idx INTEGER NOT NULL,              -- 0 = self, 1..n = "Speaker n"
  person_id TEXT REFERENCES people(id) ON DELETE SET NULL,
  name TEXT,                         -- snapshot shown when person_id is NULL
  centroid BLOB,                     -- 192 x f32
  seconds REAL NOT NULL,
  PRIMARY KEY (meeting_id, idx)
);

CREATE TABLE voiceprints (
  id TEXT PRIMARY KEY,
  person_id TEXT NOT NULL REFERENCES people(id) ON DELETE CASCADE,
  source_meeting_id TEXT REFERENCES meetings(id) ON DELETE SET NULL,
  source_idx INTEGER,
  vector BLOB NOT NULL,              -- 192 x f32, L2-normalised
  seconds REAL NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (source_meeting_id, source_idx)
);

CREATE TABLE speaker_jobs (
  meeting_id TEXT PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
  status TEXT NOT NULL,              -- pending | running | done | failed | skipped
  reason TEXT,
  backend TEXT,                      -- directml | cpu | mixed
  run_token TEXT NOT NULL,
  audio_seconds REAL,
  run_seconds REAL,
  updated_at TEXT NOT NULL
);

ALTER TABLE transcripts ADD COLUMN speaker_idx INTEGER;   -- NULL = Others/Unknown
ALTER TABLE transcripts ADD COLUMN sentences TEXT;        -- JSON [[t0, t1, text], ...]
