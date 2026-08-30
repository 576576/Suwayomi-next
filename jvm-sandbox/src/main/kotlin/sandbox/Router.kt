package sandbox

import com.sun.net.httpserver.HttpExchange
import java.net.URLDecoder
import java.nio.charset.StandardCharsets

/**
 * Routes incoming requests to handlers. The /source/ subtree dispatch is a
 * placeholder for the source invocation layer (next Phase 5 increment).
 */
class Router(private val registry: ExtensionRegistry) {

    fun health(ex: HttpExchange) {
        ex.respond(200, """{"ok":true,"extensions":${registry.extensions.size}}""")
    }

    fun extensions(ex: HttpExchange) {
        ex.respond(200, registry.toJson())
    }

    fun sources(ex: HttpExchange) {
        val parts = registry.sources.joinToString(",") { s ->
            """{"id":${s.id},"name":${jsonStr(s.name)},"lang":${jsonStr(s.lang)},"extension":${s.extension}}"""
        }
        ex.respond(200, "[$parts]")
    }

    fun sourceDispatch(ex: HttpExchange) {
        val path = ex.requestURI.path
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
        when {
            segments.size == 1 && ex.requestMethod == "GET" -> {
                val query = ex.requestURI.rawQuery ?: ""
                val params = parseQuery(query)
                val page = params["page"]?.toIntOrNull() ?: 1
                val q = params["query"]
                // Phase 5 skeleton: no source runtime loaded yet.
                ex.respond(200, """{"mangas":[],"hasNextPage":false,"query":${jsonOpt(q)},"page":$page}""")
            }
            segments.size == 2 -> {
                // manga details: /source/{id}/manga/{url}
                val mangaUrl = decodeSeg(segments[1])
                ex.respond(200, """{"url":${jsonStr(mangaUrl)},"title":"","status":"UNKNOWN"}""")
            }
            segments.size == 3 && segments[2] == "chapters" -> {
                val mangaUrl = decodeSeg(segments[1])
                ex.respond(200, """{"mangaUrl":${jsonStr(mangaUrl)},"chapters":[]}""")
            }
            segments.size == 3 && segments[2] == "filters" -> {
                ex.respond(200, "[]")
            }
            segments.size == 3 && segments[2] == "pages" -> {
                val chapterUrl = decodeSeg(segments[1])
                ex.respond(200, """{"chapterUrl":${jsonStr(chapterUrl)},"pages":[]}""")
            }
            else -> ex.respond404()
        }
    }

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
