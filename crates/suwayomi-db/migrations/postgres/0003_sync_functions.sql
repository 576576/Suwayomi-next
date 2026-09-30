-- SyncYomi trigger functions (0002 part A).
CREATE OR REPLACE FUNCTION suwayomi.bump_manga_version() RETURNS trigger AS $$
BEGIN
    IF NOT NEW.is_syncing AND (
        OLD.url IS DISTINCT FROM NEW.url OR
        OLD.description IS DISTINCT FROM NEW.description OR
        OLD.in_library IS DISTINCT FROM NEW.in_library
    ) THEN
        NEW.version := NEW.version + 1;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE OR REPLACE FUNCTION suwayomi.bump_chapter_version() RETURNS trigger AS $$
BEGIN
    IF NOT NEW.is_syncing AND (
        OLD.read IS DISTINCT FROM NEW.read OR
        OLD.bookmark IS DISTINCT FROM NEW.bookmark OR
        OLD.last_page_read IS DISTINCT FROM NEW.last_page_read
    ) THEN
        NEW.version := NEW.version + 1;
        UPDATE suwayomi.manga SET version = version + 1
         WHERE id = NEW.manga AND NOT is_syncing;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE OR REPLACE FUNCTION suwayomi.touch_manga_modified() RETURNS trigger AS $$
BEGIN
    NEW.last_modified_at := extract(epoch FROM now())::BIGINT;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE OR REPLACE FUNCTION suwayomi.touch_chapter_modified() RETURNS trigger AS $$
BEGIN
    NEW.last_modified_at := extract(epoch FROM now())::BIGINT;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE OR REPLACE FUNCTION suwayomi.bump_manga_version_on_catmanga() RETURNS trigger AS $$
BEGIN
    UPDATE suwayomi.manga SET version = version + 1
     WHERE id = NEW.manga AND NOT is_syncing;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE OR REPLACE FUNCTION suwayomi.fill_category_uid() RETURNS trigger AS $$
BEGIN
    IF NEW.uid = 0 THEN
        -- Kotlin uses Random.nextLong(1, Long.MAX_VALUE); any positive value works
        NEW.uid := (random() * 9007199254740991)::BIGINT + 1;
    END IF;
    IF NEW.last_modified_at = 0 THEN
        NEW.last_modified_at := extract(epoch FROM now())::BIGINT;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE OR REPLACE FUNCTION suwayomi.bump_category_version() RETURNS trigger AS $$
BEGIN
    IF NOT NEW.is_syncing AND (
        OLD.name IS DISTINCT FROM NEW.name OR
        OLD.sort_order IS DISTINCT FROM NEW.sort_order
    ) THEN
        NEW.version := NEW.version + 1;
        NEW.last_modified_at := extract(epoch FROM now())::BIGINT;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
