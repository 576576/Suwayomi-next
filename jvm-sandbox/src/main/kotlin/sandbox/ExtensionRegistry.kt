package sandbox

import com.sun.net.httpserver.HttpExchange
import java.nio.file.Files
import java.nio.file.Path
import java.util.jar.JarFile

/** A loaded extension: metadata + its declared source classes. */
data class SandboxExtension(
    val pkgName: String,
    val name: String,
    val lang: String,
    val versionName: String,
    val className: String,
    val jarPath: Path,
    val sourceIds: MutableList<Long> = mutableListOf(),
)

/**
 * Scans the extensions directory for installed extension jars and keeps the
 * registry that the Rust server queries via /extensions and /sources.
 *
 * Phase 5 skeleton: jar metadata (manifest) is parsed; instantiating source
 * objects through the child-first classloader is the next increment.
 */
class ExtensionRegistry(private val extensionsDir: Path) {
    val extensions = mutableListOf<SandboxExtension>()
    val sources = mutableListOf<SandboxSource>()

    fun scan() {
        extensions.clear()
        sources.clear()
        if (!Files.isDirectory(extensionsDir)) return
        Files.list(extensionsDir).use { stream ->
            stream.filter { it.toString().endsWith(".jar") }
                .sorted()
                .forEach { scanJar(it) }
        }
    }

    private fun scanJar(jarPath: Path) {
        try {
            JarFile(jarPath.toFile()).use { jar ->
                val manifest = jar.manifest ?: return
                val attrs = manifest.mainAttributes
                val feature = attrs.getValue("Tachiyomi-Extension") ?: attrs.getValue("tachiyomi.extension") ?: return
                if (feature != "true") return
                val pkgName = attrs.getValue("Tachiyomi-Extension-Pkg") ?: attrs.getValue("tachiyomi.extension.pkg") ?: jarPath.fileName.toString().removeSuffix(".jar")
                val className = attrs.getValue("Tachiyomi-Extension-Class") ?: attrs.getValue("tachiyomi.extension.class")
                val name = attrs.getValue("Tachiyomi-Extension-Name") ?: pkgName
                val lang = attrs.getValue("Tachiyomi-Extension-Lang") ?: "en"
                val versionName = attrs.getValue("Tachiyomi-Extension-Version") ?: ""
                val ext = SandboxExtension(pkgName, name, lang, versionName, className ?: "", jarPath)
                extensions.add(ext)
            }
        } catch (_: Exception) {
            // unreadable jar -> skip
        }
    }

    fun toJson(): String {
        val parts = extensions.joinToString(",") { e ->
            """{"pkgName":${jsonStr(e.pkgName)},"name":${jsonStr(e.name)},"lang":${jsonStr(e.lang)},"versionName":${jsonStr(e.versionName)},"className":${jsonStr(e.className)},"sources":[${e.sourceIds.joinToString(",")}]}"""
        }
        return "[$parts]"
    }
}

/** A registered source (Phase 5 skeleton: metadata only). */
data class SandboxSource(
    val id: Long,
    val name: String,
    val lang: String,
    val extension: Int,
)

fun HttpExchange.respond(code: Int, body: String) {
    val bytes = body.toByteArray(Charsets.UTF_8)
    responseHeaders.set("Content-Type", "application/json; charset=utf-8")
    sendResponseHeaders(code, bytes.size.toLong())
    responseBody.use { it.write(bytes) }
}

fun HttpExchange.respond404() = respond(404, """{"error":"not found"}""")
