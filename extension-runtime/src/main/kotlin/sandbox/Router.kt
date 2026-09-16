package sandbox

import java.net.URLDecoder
import java.nio.charset.StandardCharsets

/**
 * Routes incoming requests to handlers. /source/ drives the loaded extension
 * sources reflectively through [SourceDriver].
 *
 * 这一层是 Rust 侧 `HttpSandboxFetcher` 的**契约实现**：路由形状、字段名、
 * 错误格式（500 + `{"error": <stackTrace>}`）都必须保持稳定，桌面与 Android
 * 共用同一份代码就不会漂移。
 */
class Router(private val registry: SourceRegistry) : HttpHandler {

    override fun handle(req: HttpRequest): HttpResponse {
        // rawPath 不含 query，按整段精确匹配即可（原实现用 createContext 前缀匹配，
        // 对同一组路径等价，且不会再出现 `/sources` 与 `/source/` 抢前缀的问题）。
        return when (req.rawPath) {
            "/health" -> health()
            "/jvm" -> jvm()
            "/extensions" -> HttpResponse(200, registry.toExtensionsJson())
            "/sources" -> HttpResponse(200, registry.toSourcesJson())
            "/reload" -> reload()
            "/inspect" -> inspect(req)
            else -> if (req.rawPath.startsWith("/source/")) sourceDispatch(req) else notFound()
        }
    }

    private fun health(): HttpResponse = HttpResponse(
        200,
        """{"ok":true,"extensions":${registry.extensionCount},"sources":${registry.sourceCount}}""",
    )

    /** GET /jvm — JVM runtime info reported to the server (About → debug info). */
    private fun jvm(): HttpResponse = HttpResponse(
        200,
        """{"javaVersion":${jsonStr(System.getProperty("java.version") ?: "")},"vmName":${jsonStr(System.getProperty("java.vm.name") ?: "")},"vmVendor":${jsonStr(System.getProperty("java.vendor") ?: "")},"vmVersion":${jsonStr(System.getProperty("java.vm.version") ?: "")}}""",
    )

    /** POST /reload — rescans the extensions directory (install/uninstall hook). */
    private fun reload(): HttpResponse = try {
        registry.reload()
        HttpResponse(200, """{"ok":true,"extensions":${registry.extensionCount},"sources":${registry.sourceCount}}""")
    } catch (t: Throwable) {
        HttpResponse(500, """{"error":${jsonStr(t.message ?: t.toString())}}""")
    }

    /** POST /inspect — parse an uploaded APK (body bytes) and return its metadata. */
    private fun inspect(req: HttpRequest): HttpResponse = try {
        val info = registry.inspect(req.body)
        if (info == null) {
            HttpResponse(400, """{"error":"not a tachiyomi extension (missing tachiyomi.extension.class)"}""")
        } else {
            HttpResponse(
                200,
                """{"pkgName":${jsonStr(info.pkgName)},"name":${jsonStr(info.name)},"lang":${jsonStr(info.lang)},"versionName":${jsonStr(info.versionName)},"className":${jsonStr(info.className)},"extensionId":${info.extensionId},"versionCode":${info.versionCode},"contentWarning":${info.contentWarning}}""",
            )
        }
    } catch (t: Throwable) {
        HttpResponse(500, """{"error":${jsonStr(t.message ?: t.toString())}}""")
    }

    private fun sourceDispatch(req: HttpRequest): HttpResponse {
        val segments = req.rawPath.removePrefix("/source/").split("/").filter { it.isNotBlank() }
        if (segments.isEmpty()) return notFound()
        val sourceId = segments[0].toLongOrNull()
            ?: return HttpResponse(400, """{"error":"invalid source id"}""")
        val driver = registry.driver(sourceId)
            ?: return HttpResponse(404, """{"error":"source $sourceId not loaded"}""")
        val params = parseQuery(req.rawQuery)

        return try {
            when {
                // list: /source/{id} or /source/{id}/manga  (?page=&query=&mode=latest)
                (segments.size == 1 || (segments.size == 2 && segments[1] == "manga")) && req.method == "GET" -> {
                    val page = params["page"]?.toIntOrNull() ?: 1
                    val mode = params["mode"]
                    val q = params["query"]
                    val (mangas, hasNext) = when {
                        mode == "latest" -> driver.getLatestUpdates(page)
                        q != null && q.isNotBlank() -> driver.search(q, page)
                        else -> driver.getPopularManga(page)
                    }
                    HttpResponse(200, """{"mangas":[${mangas.joinToString(",") { mapToJson(it) }}],"hasNextPage":$hasNext,"page":$page}""")
                }
                // /source/{id}/manga/{mangaUrl}
                segments.size == 3 && segments[1] == "manga" -> {
                    val mangaUrl = decodeSeg(segments[2])
                    HttpResponse(200, mapToJson(driver.getMangaDetails(mapOf("url" to mangaUrl))))
                }
                // /source/{id}/manga/{mangaUrl}/chapters
                segments.size == 4 && segments[1] == "manga" && segments[3] == "chapters" -> {
                    val mangaUrl = decodeSeg(segments[2])
                    val chapters = driver.getChapterList(mapOf("url" to mangaUrl))
                    HttpResponse(200, """{"chapters":[${chapters.joinToString(",") { mapToJson(it) }}]}""")
                }
                // /source/{id}/chapter/{chapterUrl}/pages?mangaUrl=
                segments.size == 4 && segments[1] == "chapter" && segments[3] == "pages" -> {
                    val chapterUrl = decodeSeg(segments[2])
                    val mangaUrl = params["mangaUrl"] ?: ""
                    val pages = driver.getPageList(mapOf("url" to chapterUrl, "mangaUrl" to mangaUrl))
                    val resolved = driver.resolveImageUrls(pages)
                    HttpResponse(200, """{"pages":[${resolved.joinToString(",") { mapToJson(it) }}]}""")
                }
                // /source/{id}/filters
                segments.size == 3 && segments[1] == "filters" ->
                    HttpResponse(200, """{"filters":[${driver.getFilters().joinToString(",") { mapToJson(it) }}]}""")
                segments.size == 2 && segments[1] == "filters" ->
                    HttpResponse(200, """{"filters":[${driver.getFilters().joinToString(",") { mapToJson(it) }}]}""")
                else -> notFound()
            }
        } catch (t: Throwable) {
            HttpResponse(500, """{"error":${jsonStr(t.stackTraceToString())}}""")
        }
    }

    private fun notFound(): HttpResponse = HttpResponse(404, """{"error":"not found"}""")

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
