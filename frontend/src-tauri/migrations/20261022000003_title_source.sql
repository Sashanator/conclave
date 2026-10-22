-- Existing rows never get renamed later, so they migrate as user titles.
ALTER TABLE meetings ADD COLUMN title_source TEXT NOT NULL DEFAULT 'user';
ALTER TABLE meetings ADD COLUMN calendar_uid TEXT;
ALTER TABLE meetings ADD COLUMN calendar_start TEXT;
