//! Drives a loaded extension source through reflection.
//!
//! Converts between the sandbox's JSON-friendly maps and the tachiyomi model
//! classes inside the extension jar (SManga / SChapter / Page / MangasPage).

package sandbox

import eu.kanade.tachiyomi.source.model.Filter
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
        // genre may arrive as a String ("a, b, c") on some lib builds or as a
        // List<String> on others — never cast it blindly to a List or the
        // tags silently become empty.
        val genreValue: Any? = readField(m, "genre")
        val genreJoined: String = when (genreValue) {
            is List<*> -> genreValue.filterNotNull().joinToString(", ") { it.toString() }
            is String -> genreValue
            else -> ""
        }
        val out = mutableMapOf<String, Any?>()
        out["url"] = readField(m, "url") ?: ""
        out["title"] = readField(m, "title") ?: ""
        // Cover field name varies across lib versions: new libs `coverUrl`,
        // old libs `thumbnail_url` (getter getThumbnail_url). Try all.
        out["thumbnailUrl"] = readField(m, "coverUrl")
            ?: readField(m, "thumbnailUrl")
            ?: readField(m, "thumbnail_url")
        out["artist"] = readField(m, "artist")
        out["author"] = readField(m, "author")
        out["description"] = readField(m, "description")
        out["genre"] = genreJoined
        out["status"] = statusToInt(readField(m, "status"))
        return out
    }

    /**
     * nHentai.to 封面兜底：扩展的封面提取只认 CF Mirage 属性
     * (data-cfsrc/data-src/data-lazy-src/srcset)，而 nhentai.to 的缩略图用
     * `src + data-fallbacks` → thumbnailUrl 恒 null。从 manga.url
     * (/g/{id}/) 提取 gallery id，按图床惯例拼封面地址。
     */
    fun fillCoverFallback(items: List<Map<String, Any?>>, thumb: Boolean): List<Map<String, Any?>> =
        items.map { m ->
            if (m["thumbnailUrl"] != null) {
                m
            } else {
                val galleryId = Regex("""/g/(\d+)/""").find(m["url"] as? String ?: "")?.groupValues?.get(1)
                if (galleryId != null) {
                    val out = mutableMapOf<String, Any?>()
                    out.putAll(m)
                    out["thumbnailUrl"] = "https://zrocdn.xyz/galleries/$galleryId/${if (thumb) "thumb.jpg" else "cover.jpg"}"
                    out
                } else {
                    m
                }
            }
        }

    fun chapterToMap(c: Any?): Map<String, Any?> {
        val out = mutableMapOf<String, Any?>()
        out["url"] = readField(c, "url") ?: ""
        out["name"] = readField(c, "name") ?: ""
        // New libs store camelCase (`dateUpload`), old libs snake_case
        // (`date_upload`). Defaults must not shadow a real value, so prefer
        // whichever field holds a non-default number.
        val du1 = (readField(c, "dateUpload") as? Number)?.toLong() ?: 0L
        val du2 = (readField(c, "date_upload") as? Number)?.toLong() ?: 0L
        out["dateUpload"] = if (du1 != 0L) du1 else du2
        val cn1 = (readField(c, "chapterNumber") as? Number)?.toFloat() ?: -1f
        val cn2 = (readField(c, "chapter_number") as? Number)?.toFloat() ?: -1f
        out["chapterNumber"] = if (cn1 != -1f) cn1 else cn2
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

    fun getPopularManga(page: Int): Pair<List<Map<String, Any?>>, Boolean> {
        val (mangas, hasNext) = mangasPageToList(callMangasPageMethod("fetchPopularManga", "getPopularManga", page))
        return fillCoverFallback(mangas, thumb = true) to hasNext
    }

    fun getLatestUpdates(page: Int): Pair<List<Map<String, Any?>>, Boolean> {
        val (mangas, hasNext) = mangasPageToList(callMangasPageMethod("fetchLatestUpdates", "getLatestUpdates", page))
        return fillCoverFallback(mangas, thumb = true) to hasNext
    }

    fun search(query: String, page: Int): Pair<List<Map<String, Any?>>, Boolean> {
        val filters = emptyFilters()
        val (mangas, hasNext) =
            mangasPageToList(callMangasPageMethod("fetchSearchManga", "getSearchManga", page, query, filters))
        return fillCoverFallback(mangas, thumb = true) to hasNext
    }

    /**
     * Legacy keiyoushi/mihon extensions (lib 1.x) implement the rx.Observable
     * `fetchPopularManga(page)` / `fetchSearchManga(page, query, filters)` …;
     * newer ones (lib 2.x) implement the suspend `getPopularManga(page)` /
     * `getSearchManga(page, query, filters)`. R8 inlining also strips the
     * protected `popularMangaRequest` overrides of new extensions, so the
     * HttpSource template method `fetchPopularManga` falls through to the
     * sandbox default (UnsupportedOperationException) — fall back to the
     * suspend interface method in that case.
     */
    private fun callMangasPageMethod(oldName: String, newName: String, vararg args: Any?): Any? {
        return try {
            awaitObservable(callMethod(src.instance, oldName, *args))
        } catch (e: RuntimeException) {
            callSuspendMethod(src.instance, newName, *args)
        }
    }

    private fun emptyFilters(): Any? {
        return try {
            val cls = src.sourceCls.classLoader.loadClass("eu.kanade.tachiyomi.source.model.FilterList")
            // FilterList has no no-arg ctor: primary (List) + vararg (Filter[]) secondary.
            // Pick the (List) ctor; a null filters argument NPEs every HttpSource.fetchSearchManga.
            val ctor = cls.declaredConstructors.firstOrNull {
                it.parameterCount == 1 && it.parameterTypes[0] == List::class.java
            } ?: return null
            ctor.newInstance(emptyList<Any>())
        } catch (e: Exception) {
            null
        }
    }

    fun getMangaDetails(fields: Map<String, Any?>): Map<String, Any?> {
        val smanga = buildModel(src.smangaCls, fields)
        // new keiyoushi lib (tachiyomix 1.6): suspend getMangaUpdate(manga, chapters, fetchDetails, fetchChapters)
        val viaUpdate = try {
            val update = callSuspendMethod(src.instance, "getMangaUpdate", smanga, emptyList<Any>(), true, false)
            val m = readField(update, "manga")
            // mangaToMap 先合并，fields（含调用方传入的 url）再覆盖——扩展返回的
            // SManga.url 可能为空，保留请求时的 url 供封面兜底推导。
            mangaToMap(m) + fields
        } catch (e: Throwable) {
            null
        }
        if (viaUpdate != null) {
            return fillCoverFallback(listOf(viaUpdate), thumb = false).first()
        }
        // legacy fallback: rx fetchMangaDetails / suspend getMangaDetails
        val result = callFetchOrSuspend("fetchMangaDetails", "getMangaDetails", smanga) ?: return fields
        return fillCoverFallback(listOf(fields + mangaToMap(result)), thumb = false).first()
    }

    fun getChapterList(mangaFields: Map<String, Any?>): List<Map<String, Any?>> {
        val smanga = buildModel(src.smangaCls, mangaFields)
        // new lib: getMangaUpdate(..., fetchChapters=true) returns SMangaUpdate(chapters)
        val viaUpdate = try {
            val update = callSuspendMethod(src.instance, "getMangaUpdate", smanga, emptyList<Any>(), false, true)
            (readField(update, "chapters") as? List<Any>)?.map { chapterToMap(it) }
        } catch (e: Throwable) {
            null
        }
        if (viaUpdate != null) return viaUpdate
        // legacy fallback
        val obs = callFetchOrSuspend("fetchChapterList", "getChapterList", smanga)
        val chapters: List<Any> = (if (obs is rx.Observable<*>) awaitObservable(obs) else obs) as? List<Any> ?: emptyList()
        return chapters.map { chapterToMap(it) }
    }

    fun getPageList(chapterFields: Map<String, Any?>): List<Map<String, Any?>> {
        val schapter = buildModel(src.schapterCls, chapterFields)
        val obs = callFetchOrSuspend("fetchPageList", "getPageList", schapter)
        val pages: List<Any> = (if (obs is rx.Observable<*>) awaitObservable(obs) else obs) as? List<Any> ?: emptyList()
        return pages.map { pageToMap(it) }
    }

    /**
     * Tries the legacy rx `fetch*` method first (lib 1.x extensions), falls
     * back to the suspend `get*` interface method (new keiyoushi lib 2.x).
     */
    private fun callFetchOrSuspend(fetchName: String, getName: String, arg: Any): Any? {
        return try {
            val obs = callMethod(src.instance, fetchName, arg)
            if (obs is rx.Observable<*>) awaitObservable(obs) else obs
        } catch (e: RuntimeException) {
            callSuspendMethod(src.instance, getName, arg)
        }
    }

    /** Fills each page's imageUrl if the extension left it blank. */
    fun resolveImageUrls(pages: List<Map<String, Any?>>): List<Map<String, Any?>> =
        pages.map { p ->
            if (p["imageUrl"] != null) {
                p
            } else {
                val pageObj = buildModel(src.pageCls, p)
                val imageUrl = try {
                    // legacy: getImageUrl(page) — new: suspend getImageUrl(page)
                    callFetchOrSuspend("getImageUrl", "getImageUrl", pageObj)?.toString()
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

    // ---- search filters -------------------------------------------------------

    /** Reads the source's filter list (`getFilterList` / `fetchFilterList`). */
    fun getFilters(): List<Map<String, Any?>> {
        val obs = try {
            callMethod(src.instance, "getFilterList")
        } catch (e: Exception) {
            System.err.println("sandbox: getFilterList failed: " + e.stackTraceToString())
            try {
                callMethod(src.instance, "fetchFilterList")
            } catch (e2: Exception) {
                System.err.println("sandbox: fetchFilterList failed: " + e2.stackTraceToString())
                null
            }
        }
        val list = awaitObservable(obs) ?: return emptyList()
        val filters = readField(list, "list") as? List<Any> ?: emptyList()
        return filters.map { filterToMap(it) }
    }
}

/** Serializes a Filter subclass into the Tachidesk JSON shape. */
internal fun filterToMap(f: Any?): Map<String, Any?> {
    if (f == null) return emptyMap()
    val name = readField(f, "name") as? String ?: ""
    // 按类型层级判断（扩展 jar 里的 Filter 都是具名子类，simpleName 不可靠）
    val type = when (f) {
        is Filter.Header -> "title"
        is Filter.Separator -> "separator"
        is Filter.Select<*> -> "select"
        is Filter.Text -> "text"
        is Filter.CheckBox -> "check-box"
        is Filter.TriState -> "tri-state"
        is Filter.Sort -> "sort"
        is Filter.Group<*> -> "group"
        else -> "unknown"
    }
    val out = mutableMapOf<String, Any?>()
    out["type"] = type
    out["name"] = name
    when (type) {
        "title", "separator" -> {}
        "select" -> {
            out["state"] = (readField(f, "state") as? Number)?.toInt() ?: 0
            out["values"] = readField(f, "displayValues") as? List<Any> ?: emptyList<Any>()
        }
        "text" -> out["state"] = readField(f, "state") ?: ""
        "check-box" -> out["state"] = readField(f, "state") ?: false
        "tri-state" -> out["state"] = (readField(f, "state") as? Number)?.toInt() ?: 0
        "sort" -> {
            out["values"] = (readField(f, "values") as? Array<*>)?.map { it?.toString() } ?: emptyList<String>()
            val sel = readField(f, "state")
            out["state"] = if (sel == null) {
                null
            } else {
                mapOf<String, Any?>(
                    "index" to ((readField(sel, "index") as? Number)?.toInt() ?: 0),
                    "ascending" to ((readField(sel, "ascending") as? Boolean) ?: true),
                )
            }
        }
        "group" -> out["state"] = readField(f, "state") ?: emptyList<Any>()
    }
    return out
}
