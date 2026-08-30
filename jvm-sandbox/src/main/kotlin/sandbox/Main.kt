package sandbox

import com.sun.net.httpserver.HttpServer
import java.net.InetSocketAddress
import java.nio.file.Files
import java.nio.file.Paths

/**
 * Suwayomi extension sandbox — runs Mihon/Tachiyomi extensions in a JVM
 * process isolated from the Rust server. Exposes a stable HTTP/JSON contract:
 *
 *   GET  /health                     -> {"ok":true}
 *   GET  /extensions                 -> [{pkgName, name, lang, versionName, className, sources:[{id,name,lang}]}]
 *   GET  /sources                    -> [{id,name,lang,extension}]
 *   GET  /source/{id}/manga?query=&page=   -> {mangas:[{url,title,thumbnailUrl,...}], hasNextPage}
 *   GET  /source/{id}/manga/{mangaUrl}     -> SManga json
 *   GET  /source/{id}/manga/{mangaUrl}/chapters -> [SChapter json]
 *   GET  /source/{id}/chapter/{chapterUrl}/pages -> [String urls]
 *   GET  /source/{id}/filters         -> [Filter json]
 *
 * Phase 5 skeleton: the HTTP contract, extension registry and process
 * lifecycle are wired; loading real extension JARs through the child-first
 * classloader (with the full eu.kanade.tachiyomi.source.* interface set and
 * AndroidCompat) is the next increment.
 */
fun main() {
    val port = System.getenv("SUWAYOMI_SANDBOX_PORT")?.toIntOrNull() ?: 4569
    val extensionsDir = System.getenv("SUWAYOMI_EXTENSIONS_DIR") ?: "extensions"
    Files.createDirectories(Paths.get(extensionsDir))

    val registry = ExtensionRegistry(Paths.get(extensionsDir))
    registry.scan()

    setupInjekt()
    val server = HttpServer.create(InetSocketAddress("127.0.0.1", port), 0)
    val router = Router(registry)

    server.createContext("/health") { router.health(it) }
    server.createContext("/extensions") { router.extensions(it) }
    server.createContext("/sources") { router.sources(it) }
    server.createContext("/source/") { router.sourceDispatch(it) }
    server.executor = null
    server.start()
    println("suwayomi-jvm-sandbox listening on 127.0.0.1:$port (extensions dir: $extensionsDir)")
}

fun jsonStr(s: String): String = "\"" + s
    .replace("\\", "\\\\")
    .replace("\"", "\\\"")
    .replace("\n", "\\n")
    .replace("\r", "\\r")
    .replace("\t", "\\t") + "\""

fun jsonOpt(s: String?): String = if (s == null) "null" else jsonStr(s)
