ALTER TABLE practice_setups ADD COLUMN language TEXT NOT NULL DEFAULT 'en' CHECK (language IN ('en','de'));
-- NULL: recorded before v2.1.0; the session then follows its setup's current language.
ALTER TABLE practice_sessions ADD COLUMN language TEXT CHECK (language IN ('en','de'));
