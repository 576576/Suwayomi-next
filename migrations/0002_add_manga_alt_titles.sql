-- Alternative titles (other-language titles from archive metadata) shown on
-- the manga details page as "Alternative title: …". JSON array of strings.
ALTER TABLE suwayomi.manga ADD COLUMN IF NOT EXISTS alt_titles TEXT NOT NULL DEFAULT '[]';
