//! Drives a loaded extension source through reflection.
//!
//! Converts between the sandbox's JSON-friendly maps and the tachiyomi model
//! classes inside the extension jar (SManga / SChapter / Page / MangasPage).

package sandbox

import suwayomi.tachidesk.manga.impl.util.lang.awaitSingle

import java.lang.reflect.Modifier

class SourceDriver(private val src: LoadedSource) {

    // ---- MangasPage helpers -------------------------------------------------

    /** Reads a MangasPage into a plain JSON map list. */
    fun mangasPageToList(page: Any?): Pair<List<Map<String, Any?>>, Boolean> {
        if (page == null) return emptyList<Map<String, Any?>>() to false
        val mangas: List<Any> = readField(page, "mangas") as? List<Any> ?: emptyList()
        val hasNext: Boolean = readField(page, "hasNextPage") as? Boolean ?: false
        val items = mangas.map { mangaToMap(it) }
        return items to hasNext
    }

    fun mangaToMap(m: Any?): Map<String, Any?> {
        val genre: List<Any> = readField(m, "genre") as? List<Any> ?: emptyList()
        val out = mutableMapOf<String, Any?>()
        out["url"] = readField(m, "url") ?: ""
        out["title"] = readField(m, "title") ?: ""
        out["thumbnailUrl"] = readField(m, "thumbnailUrl")
        out["artist"] = readField(m, "artist")
        out["author"] = readField(m, "author")
        out["description"] = readField(m, "description")
        out["genre"] = genre.joinToString(", ")
        out["status"] = statusToInt(readField(m, "status"))
        return out
    }

    fun chapterToMap(c: Any?): Map<String, Any?> {
        val out = mutableMapOf<String, Any?>()
        out["url"] = readField(c, "url") ?: ""
        out["name"] = readField(c, "name") ?: ""
        out["dateUpload"] = (readField(c, "dateUpload") as? Number)?.toLong() ?: 0L
        out["chapterNumber"] = (readField(c, "chapterNumber") as? Number)?.toFloat() ?: -1f
        out["scanlator"] = readField(c, "scanlator")
        return out
    }

    fun pageToMap(p: Any?): Map<String, Any?> {
        val out = mutableMapOf<String, Any?>()
        out["index"] = (readField(p, "index") as? Number)?.toInt() ?: 0
        out["url"] = readField(p, "url") ?: ""
        out["imageUrl"] = readField(p, "imageUrl")
        return out
    }

    // ---- operations ----------------------------------------------------------
    // Mihon extensions (lib 1.x) implement the deprecated rx.Observable-based
    // fetch* methods; the suspend get* wrappers live on the interface. We call
    // fetch* and block on the observable.

    private fun awaitObservable(obs: Any?): Any? {
        if (obs == null) return null
        return kotlinx.coroutines.runBlocking {
            (obs as rx.Observable<*>).awaitSingle()
        }
    }

    fun getPopularManga(page: Int): Pair<List<Map<String, Any?>>, Boolean> =
        mangasPageToList(awaitObservable(callMethod(src.instance, "fetchPopularManga", page)))

    fun getLatestUpdates(page: Int): Pair<List<Map<String, Any?>>, Boolean> =
        mangasPageToList(awaitObservable(callMethod(src.instance, "fetchLatestUpdates", page)))

    fun search(query: String, page: Int): Pair<List<Map<String, Any?>>, Boolean> {
        val filters = emptyFilters()
        return mangasPageToList(awaitObservable(callMethod(src.instance, "fetchSearchManga", page, query, filters)))
    }

    private fun emptyFilters(): Any? {
        return try {
            val cls = src.sourceCls.classLoader.loadClass("eu.kanade.tachiyomi.source.model.FilterList")
            cls.getDeclaredConstructor().newInstance()
        } catch (e: Exception) {
            null
        }
    }

    fun getMangaDetails(fields: Map<String, Any?>): Map<String, Any?> {
        val smanga = buildModel(src.smangaCls, fields)
        val result = awaitObservable(callMethod(src.instance, "fetchMangaDetails", smanga)) ?: return fields
        return fields + mangaToMap(result)
    }

    fun getChapterList(mangaFields: Map<String, Any?>): List<Map<String, Any?>> {
        val smanga = buildModel(src.smangaCls, mangaFields)
        val obs = callMethod(src.instance, "fetchChapterList", smanga)
        val chapters: List<Any> = (if (obs is rx.Observable<*>) awaitObservable(obs) else obs) as? List<Any> ?: emptyList()
        return chapters.map { chapterToMap(it) }
    }

    fun getPageList(chapterFields: Map<String, Any?>): List<Map<String, Any?>> {
        val schapter = buildModel(src.schapterCls, chapterFields)
        val obs = callMethod(src.instance, "fetchPageList", schapter)
        val pages: List<Any> = (if (obs is rx.Observable<*>) awaitObservable(obs) else obs) as? List<Any> ?: emptyList()
        return pages.map { pageToMap(it) }
    }

    /** Fills each page's imageUrl if the extension left it blank. */
    fun resolveImageUrls(pages: List<Map<String, Any?>>): List<Map<String, Any?>> =
        pages.map { p ->
            if (p["imageUrl"] != null) {
                p
            } else {
                val pageObj = buildModel(src.pageCls, p)
                val imageUrl = try {
                    callMethod(src.instance, "getImageUrl", pageObj)?.toString()
                } catch (e: Exception) {
                    null
                }
                val out = mutableMapOf<String, Any?>()
                out.putAll(p)
                out["imageUrl"] = imageUrl
                out
            }
        }

    // ---- misc -----------------------------------------------------------------

    fun supportsLatest(): Boolean {
        val m = src.sourceCls.methods.firstOrNull { it.name == "supportsLatest" }
        return if (m == null || Modifier.isAbstract(m.modifiers)) true else (m.invoke(src.instance) as? Boolean) ?: true
    }

    private fun statusToInt(status: Any?): Int = when (status?.toString()) {
        "ONGOING" -> 1
        "COMPLETED" -> 2
        "LICENSED" -> 3
        "PUBLISHING_FINISHED" -> 4
        "CANCELLED" -> 5
        "ON_HIATUS" -> 6
        else -> 0
    }
}
