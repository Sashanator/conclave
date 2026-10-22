-- The normalised text an auto task was first extracted with, so an edit does not bring back a duplicate. NULL for older rows.
ALTER TABLE tasks ADD COLUMN origin_norm TEXT;
