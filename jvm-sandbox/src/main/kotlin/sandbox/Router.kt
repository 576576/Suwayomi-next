package sandbox

import com.sun.net.httpserver.HttpExchange
import java.net.URLDecoder
import java.nio.charset.StandardCharsets

/**
 * Routes incoming requests to handlers. /source/ drives the loaded extension
 * sources reflectively through [SourceDriver].
 */
class Router(private val registry: ExtensionRegistry) {

    fun health(ex: HttpExchange) {
        ex.respond(200, """{"ok":true,"extensions":${registry.extensions.size},"sources":${registry.sources.size}}""")
    }

    fun extensions(ex: HttpExchange) {
        ex.respond(200, registry.toExtensionsJson())
    }

    fun sources(ex: HttpExchange) {
        ex.respond(200, registry.toSourcesJson())
    }

    /** POST /reload — rescans the extensions directory (install/uninstall hook). */
    fun reload(ex: HttpExchange) {
        try {
            registry.reload()
            ex.respond(200, """{"ok":true,"extensions":${registry.extensions.size},"sources":${registry.sources.size}}""")
        } catch (t: Throwable) {
            ex.respond(500, """{"error":${jsonStr(t.message ?: t.toString())}}""")
        }
    }

    /** POST /inspect — parse an uploaded APK (body bytes) and return its metadata. */
    fun inspect(ex: HttpExchange) {
        try {
            val bytes = ex.requestBody.readBytes()
            val tmp = java.nio.file.Files.createTempFile("ext-inspect-", ".apk")
            java.nio.file.Files.write(tmp, bytes)
            val info = registry.inspect(tmp) ?: run {
                java.nio.file.Files.deleteIfExists(tmp)
                ex.respond(400, """{"error":"not a tachiyomi extension (missing tachiyomi.extension.class)"}""")
                return
            }
            java.nio.file.Files.deleteIfExists(tmp)
            ex.respond(
                200,
                """{"pkgName":${jsonStr(info.pkgName)},"name":${jsonStr(info.name)},"lang":${jsonStr(info.lang)},"versionName":${jsonStr(info.versionName)},"className":${jsonStr(info.className)},"extensionId":${info.extensionId}}""",
            )
        } catch (t: Throwable) {
            ex.respond(500, """{"error":${jsonStr(t.message ?: t.toString())}}""")
        }
    }

    fun sourceDispatch(ex: HttpExchange) {
        val path = ex.requestURI.rawPath
        val segments = path.removePrefix("/source/").split("/").filter { it.isNotBlank() }
        if (segments.isEmpty()) {
            ex.respond404()
            return
        }
        val sourceId = segments[0].toLongOrNull()
        if (sourceId == null) {
            ex.respond(400, """{"error":"invalid source id"}""")
            return
        }
        val driver = registry.driver(sourceId)
        if (driver == null) {
            ex.respond(404, """{"error":"source $sourceId not loaded"}""")
            return
        }
        val params = parseQuery(ex.requestURI.rawQuery ?: "")

        try {
            when {
                // list: /source/{id} or /source/{id}/manga  (?page=&query=&mode=latest)
                (segments.size == 1 || (segments.size == 2 && segments[1] == "manga")) && ex.requestMethod == "GET" -> {
                    val page = params["page"]?.toIntOrNull() ?: 1
                    val mode = params["mode"]
                    val q = params["query"]
                    val (mangas, hasNext) = when {
                        mode == "latest" -> driver.getLatestUpdates(page)
                        q != null && q.isNotBlank() -> driver.search(q, page)
                        else -> driver.getPopularManga(page)
                    }
                    ex.respond(200, """{"mangas":[${mangas.joinToString(",") { mapToJson(it) }}],"hasNextPage":$hasNext,"page":$page}""")
                }
                // /source/{id}/manga/{mangaUrl}
                segments.size == 3 && segments[1] == "manga" -> {
                    val mangaUrl = decodeSeg(segments[2])
                    val details = driver.getMangaDetails(mapOf("url" to mangaUrl))
                    ex.respond(200, mapToJson(details))
                }
                // /source/{id}/manga/{mangaUrl}/chapters
                segments.size == 4 && segments[1] == "manga" && segments[3] == "chapters" -> {
                    val mangaUrl = decodeSeg(segments[2])
                    val chapters = driver.getChapterList(mapOf("url" to mangaUrl))
                    ex.respond(200, """{"chapters":[${chapters.joinToString(",") { mapToJson(it) }}]}""")
                }
                // /source/{id}/chapter/{chapterUrl}/pages?mangaUrl=
                segments.size == 4 && segments[1] == "chapter" && segments[3] == "pages" -> {
                    val chapterUrl = decodeSeg(segments[2])
                    val mangaUrl = params["mangaUrl"] ?: ""
                    val pages = driver.getPageList(mapOf("url" to chapterUrl, "mangaUrl" to mangaUrl))
                    val resolved = driver.resolveImageUrls(pages)
                    ex.respond(200, """{"pages":[${resolved.joinToString(",") { mapToJson(it) }}]}""")
                }
                // /source/{id}/filters
                segments.size == 3 && segments[1] == "filters" -> ex.respond(200, "[]")
                segments.size == 2 && segments[1] == "filters" -> ex.respond(200, "[]")
                else -> ex.respond404()
            }
        } catch (t: Throwable) {
            try {
                ex.respond(500, """{"error":${jsonStr(t.stackTraceToString())}}""")
            } catch (e2: Exception) {
                t.printStackTrace()
            }
        }
    }

    private fun mapToJson(m: Map<String, Any?>): String =
        m.entries.joinToString(",") { (k, v) ->
            when (v) {
                null -> """${jsonStr(k)}:null"""
                is Number, is Boolean -> """${jsonStr(k)}:$v"""
                is Map<*, *> -> """${jsonStr(k)}:${mapToJson(@Suppress("UNCHECKED_CAST") (v as Map<String, Any?>))}"""
                is List<*> -> """${jsonStr(k)}:[${v.joinToString(",") { mapToJson(@Suppress("UNCHECKED_CAST") (it as Map<String, Any?>)) }}]"""
                else -> """${jsonStr(k)}:${jsonStr(v.toString())}"""
            }
        }.let { "{${it}}" }

    private fun decodeSeg(s: String): String =
        URLDecoder.decode(s, StandardCharsets.UTF_8)

    private fun parseQuery(raw: String): Map<String, String> =
        raw.split("&").filter { it.isNotBlank() }.mapNotNull { pair ->
            val idx = pair.indexOf('=')
            if (idx < 0) null else {
                pair.substring(0, idx) to URLDecoder.decode(pair.substring(idx + 1), StandardCharsets.UTF_8)
            }
        }.toMap()
}
