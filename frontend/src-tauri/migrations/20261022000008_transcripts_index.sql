-- Meeting pages, search and delete read transcripts by meeting_id; without this index every read scans the table.
CREATE INDEX IF NOT EXISTS idx_transcripts_meeting ON transcripts(meeting_id, audio_start_time, id);
