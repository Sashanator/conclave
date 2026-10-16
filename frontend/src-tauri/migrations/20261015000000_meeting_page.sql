-- Phase 4 (spec §4): one mark per transcript line, the per-meeting "Keep audio" pin,
-- and when the user deleted the audio in the old-audio review.
ALTER TABLE transcripts ADD COLUMN mark TEXT;
ALTER TABLE meetings ADD COLUMN keep_audio INTEGER NOT NULL DEFAULT 0;
ALTER TABLE meetings ADD COLUMN audio_deleted_at TEXT;
