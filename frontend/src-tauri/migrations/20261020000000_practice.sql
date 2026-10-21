CREATE TABLE practice_setups (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, character TEXT NOT NULL, situation TEXT NOT NULL, voice TEXT NOT NULL,
  seeded INTEGER NOT NULL DEFAULT 0, plan TEXT, plan_fingerprint TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE practice_materials (
  id TEXT PRIMARY KEY, setup_id TEXT NOT NULL REFERENCES practice_setups(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('pdf','text')), name TEXT NOT NULL, content BLOB NOT NULL, bytes INTEGER NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE practice_sessions (
  id TEXT PRIMARY KEY, setup_id TEXT REFERENCES practice_setups(id) ON DELETE SET NULL,
  setup_name TEXT NOT NULL, character TEXT NOT NULL, voice TEXT NOT NULL, started_at TEXT NOT NULL,
  duration_secs REAL NOT NULL DEFAULT 0, folder_path TEXT, plan TEXT,
  status TEXT NOT NULL CHECK (status IN ('running','ended')), end_reason TEXT,
  report_json TEXT, report_status TEXT NOT NULL DEFAULT 'pending' CHECK (report_status IN ('pending','waiting','running','ready','failed')), report_error TEXT);
CREATE TABLE practice_turns (
  session_id TEXT NOT NULL REFERENCES practice_sessions(id) ON DELETE CASCADE, idx INTEGER NOT NULL,
  speaker TEXT NOT NULL CHECK (speaker IN ('user','character')), live_text TEXT NOT NULL, verbatim_text TEXT,
  at_secs REAL NOT NULL, wav_offset INTEGER, wav_len INTEGER, PRIMARY KEY (session_id, idx));

INSERT INTO practice_setups (id, name, character, situation, voice, seeded, created_at, updated_at) VALUES
  ('seed-interview', 'Job interview', 'Anna Keller, engineering manager',
   'Interview for a backend developer role at a mid-size software company. A mix of behavioural and technical questions. About 30 minutes.',
   'F2', 1, datetime('now'), datetime('now')),
  ('seed-thesis', 'Thesis defence', 'Prof. Weber, strict examiner',
   'Master''s thesis defence. The professor is sceptical about the evaluation method and likes follow-up questions. About 20 minutes.',
   'M1', 1, datetime('now'), datetime('now')),
  ('seed-casual', 'Casual chat', 'Mike, a friendly car mechanic',
   'I bring my car to Mike''s garage because of a strange noise. Small talk and explaining the problem.',
   'M3', 1, datetime('now'), datetime('now'));
