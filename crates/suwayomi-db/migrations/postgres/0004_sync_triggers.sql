-- SyncYomi trigger definitions (0002 part B).
DROP TRIGGER IF EXISTS trg_bump_manga_version ON suwayomi.manga;
CREATE TRIGGER trg_bump_manga_version
    BEFORE UPDATE ON suwayomi.manga
    FOR EACH ROW EXECUTE FUNCTION suwayomi.bump_manga_version();
DROP TRIGGER IF EXISTS trg_bump_chapter_version ON suwayomi.chapter;
CREATE TRIGGER trg_bump_chapter_version
    BEFORE UPDATE ON suwayomi.chapter
    FOR EACH ROW EXECUTE FUNCTION suwayomi.bump_chapter_version();
DROP TRIGGER IF EXISTS trg_touch_manga_modified ON suwayomi.manga;
CREATE TRIGGER trg_touch_manga_modified
    BEFORE UPDATE ON suwayomi.manga
    FOR EACH ROW EXECUTE FUNCTION suwayomi.touch_manga_modified();
DROP TRIGGER IF EXISTS trg_touch_chapter_modified ON suwayomi.chapter;
CREATE TRIGGER trg_touch_chapter_modified
    BEFORE UPDATE ON suwayomi.chapter
    FOR EACH ROW EXECUTE FUNCTION suwayomi.touch_chapter_modified();
DROP TRIGGER IF EXISTS trg_bump_manga_on_catmanga ON suwayomi.category_manga;
CREATE TRIGGER trg_bump_manga_on_catmanga
    AFTER INSERT ON suwayomi.category_manga
    FOR EACH ROW EXECUTE FUNCTION suwayomi.bump_manga_version_on_catmanga();
DROP TRIGGER IF EXISTS trg_fill_category_uid ON suwayomi.category;
CREATE TRIGGER trg_fill_category_uid
    BEFORE INSERT ON suwayomi.category
    FOR EACH ROW EXECUTE FUNCTION suwayomi.fill_category_uid();
DROP TRIGGER IF EXISTS trg_bump_category_version ON suwayomi.category;
CREATE TRIGGER trg_bump_category_version
    BEFORE UPDATE ON suwayomi.category
    FOR EACH ROW EXECUTE FUNCTION suwayomi.bump_category_version();
