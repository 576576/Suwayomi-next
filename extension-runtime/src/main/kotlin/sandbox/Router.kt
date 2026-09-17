package sandbox

import java.net.URLDecoder
import java.nio.charset.StandardCharsets
import java.util.Base64

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
            else -> when {
                req.rawPath.startsWith("/source/") -> sourceDispatch(req)
                req.rawPath.startsWith("/icon/") -> icon(req)
                else -> notFound()
            }
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
        // 失败清单必须回给调用方：单个 APK 加载失败不再让 /reload 失败，
        // 不回传的话「装上了但没反应」就完全没有征兆。
        val failJson = registry.failures().entries.joinToString(",") {
            """{"apk":${jsonStr(it.key)},"error":${jsonStr(it.value)}}"""
        }
        HttpResponse(
            200,
            """{"ok":true,"extensions":${registry.extensionCount},"sources":${registry.sourceCount},"failures":[$failJson]}""",
        )
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
    } catch (e: IllegalStateException) {
        HttpResponse(400, """{"error":${jsonStr(e.message ?: e.toString())}}""")
    } catch (t: Throwable) {
        HttpResponse(500, """{"error":${jsonStr(t.message ?: t.toString())}}""")
    }

    /**
     * GET /icon/{pkg} — 扩展 APK 里的图标，base64 装在 JSON 里。
     *
     * `HttpResponse.body` 是 `String`，两端都按 UTF-8 写体，为一个几十 KB 的图标
     * 去加二进制体支持不划算。拿不到就 404，由 server 侧决定用什么兜底。
     */
    private fun icon(req: HttpRequest): HttpResponse {
        val pkg = URLDecoder.decode(req.rawPath.removePrefix("/icon/"), StandardCharsets.UTF_8)
        if (pkg.isBlank() || pkg.contains('/')) return notFound()
        val bytes = try {
            registry.icon(pkg)
        } catch (t: Throwable) {
            System.err.println("sandbox: read icon of $pkg failed: $t")
            return HttpResponse(500, """{"error":${jsonStr(t.message ?: t.toString())}}""")
        } ?: return HttpResponse(404, """{"error":"no icon for $pkg"}""")
        return HttpResponse(200, """{"mime":"image/png","data":"${Base64.getEncoder().encodeToString(bytes)}"}""")
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

    /**
     * 手写 JSON 序列化（`sandbox.Json` 的 `jsonStr` 只负责转义字符串）。
     *
     * [jsonValue] 对任意 `Any?` 分派 —— 不能假设列表元素都是 Map：图源 filter 的
     * `values` 就是 `List<String>`，旧写法在 `is List<*>` 分支里无条件强转 Map，
     * 于是 `/source/{id}/filters` 直接 500（`String cannot be cast to Map`）。
     */
    private fun mapToJson(m: Map<String, Any?>): String =
        m.entries.joinToString(",") { (k, v) -> """${jsonStr(k)}:${jsonValue(v)}""" }.let { "{${it}}" }

    private fun jsonValue(v: Any?): String = when (v) {
        null -> "null"
        is Number, is Boolean -> v.toString()
        is Map<*, *> -> mapToJson(@Suppress("UNCHECKED_CAST") (v as Map<String, Any?>))
        is List<*> -> "[${v.joinToString(",") { jsonValue(it) }}]"
        else -> jsonStr(v.toString())
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
