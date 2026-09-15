-- =====================================================================
-- SyncYomi triggers — SQLite port of `migrations/pg-only/0002_sync_functions.sql`
-- + `0002_sync_triggers.sql`.
--
-- SQLite has no PL/pgSQL, and a BEFORE trigger cannot modify NEW, so every
-- BEFORE-ROW trigger from the PostgreSQL version becomes an AFTER trigger that
-- updates the row it just observed. `UPDATE OF <cols>` keeps those follow-up
-- writes from re-entering the same trigger, and the timestamp triggers are
-- guarded by `NEW.last_modified_at = OLD.last_modified_at` so the inner write
-- terminates immediately (documented divergence: an explicit `last_modified_at`
-- write is preserved instead of being overwritten, which PostgreSQL did).
-- =====================================================================

-- manga: url / description / in_library changed -> version += 1
CREATE TRIGGER IF NOT EXISTS trg_bump_manga_version
AFTER UPDATE OF url, description, in_library ON manga
FOR EACH ROW WHEN NEW.is_syncing = 0
    AND (OLD.url IS NOT NEW.url OR OLD.description IS NOT NEW.description OR OLD.in_library IS NOT NEW.in_library)
BEGIN
    UPDATE manga SET version = version + 1 WHERE id = NEW.id;
END;

-- chapter: read / bookmark / last_page_read changed -> bump chapter and manga
CREATE TRIGGER IF NOT EXISTS trg_bump_chapter_version
AFTER UPDATE OF read, bookmark, last_page_read ON chapter
FOR EACH ROW WHEN NEW.is_syncing = 0
    AND (OLD.read IS NOT NEW.read OR OLD.bookmark IS NOT NEW.bookmark OR OLD.last_page_read IS NOT NEW.last_page_read)
BEGIN
    UPDATE chapter SET version = version + 1 WHERE id = NEW.id;
    UPDATE manga SET version = version + 1 WHERE id = NEW.manga AND is_syncing = 0;
END;

-- any other write to manga stamps last_modified_at
CREATE TRIGGER IF NOT EXISTS trg_touch_manga_modified
AFTER UPDATE ON manga
FOR EACH ROW WHEN NEW.last_modified_at = OLD.last_modified_at
BEGIN
    UPDATE manga SET last_modified_at = CAST(strftime('%s', 'now') AS INTEGER) WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_touch_chapter_modified
AFTER UPDATE ON chapter
FOR EACH ROW WHEN NEW.last_modified_at = OLD.last_modified_at
BEGIN
    UPDATE chapter SET last_modified_at = CAST(strftime('%s', 'now') AS INTEGER) WHERE id = NEW.id;
END;

-- adding a manga to a category bumps the manga's version
CREATE TRIGGER IF NOT EXISTS trg_bump_manga_on_catmanga
AFTER INSERT ON category_manga
FOR EACH ROW
BEGIN
    UPDATE manga SET version = version + 1 WHERE id = NEW.manga AND is_syncing = 0;
END;

-- new category: mint a positive uid and a modification stamp when unset
CREATE TRIGGER IF NOT EXISTS trg_fill_category_uid
AFTER INSERT ON category
FOR EACH ROW WHEN NEW.uid = 0 OR NEW.last_modified_at = 0
BEGIN
    UPDATE category SET
        uid = CASE WHEN NEW.uid = 0 THEN (abs(random()) % 9007199254740991) + 1 ELSE NEW.uid END,
        last_modified_at = CASE
            WHEN NEW.last_modified_at = 0 THEN CAST(strftime('%s', 'now') AS INTEGER)
            ELSE NEW.last_modified_at
        END
    WHERE id = NEW.id;
END;

-- category: name / sort_order changed -> version += 1 and a fresh stamp
CREATE TRIGGER IF NOT EXISTS trg_bump_category_version
AFTER UPDATE OF name, sort_order ON category
FOR EACH ROW WHEN NEW.is_syncing = 0
    AND (OLD.name IS NOT NEW.name OR OLD.sort_order IS NOT NEW.sort_order)
BEGIN
    UPDATE category SET version = version + 1, last_modified_at = CAST(strftime('%s', 'now') AS INTEGER)
    WHERE id = NEW.id;
END;
