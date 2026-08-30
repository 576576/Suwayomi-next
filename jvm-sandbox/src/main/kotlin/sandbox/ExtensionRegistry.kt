//! Extension registry — scans the extensions directory for APKs, converts
//! them to jars (dex2jar), loads the Source classes and exposes the sources
//! to the HTTP router.

package sandbox

import net.dongliu.apk.parser.ApkFile
import java.nio.file.Files
import java.nio.file.Path
import java.util.concurrent.ConcurrentHashMap

class ExtensionRegistry(private val rootDir: Path) {
    val extensions = ConcurrentHashMap<String, ExtensionInfo>() // pkgName -> info
    val sources = ConcurrentHashMap<Long, LoadedSource>() // source id -> loaded

    private val loader = ExtensionLoader(rootDir)

    fun scan() {
        if (!Files.isDirectory(rootDir)) {
            Files.createDirectories(rootDir)
        }
        Files.list(rootDir).use { stream ->
            stream.filter { it.fileName.toString().endsWith(".apk") }.forEach { apk ->
                try {
                    loadApk(apk)
                } catch (e: Exception) {
                    System.err.println("sandbox: failed to load $apk: ${e.message}")
                    e.printStackTrace()
                }
            }
        }
    }

    private fun loadApk(apk: Path) {
        val info = readApkInfo(apk) ?: return
        val loaded = loader.load(apk, info.className, info.extensionId)
        extensions[info.pkgName] = info
        loaded.forEach { sources[it.id] = it }
        println("sandbox: loaded ${loaded.size} source(s) from ${apk.fileName} (${info.name}/${info.versionName})")
    }

    /** Reads pkg info + the Source class name from the APK manifest. */
    private fun readApkInfo(apk: Path): ExtensionInfo? {
        return ApkFile(apk.toFile()).use { apkFile ->
            val meta = apkFile.apkMeta ?: return@use null
            val manifest = apkFile.manifestXml ?: return@use null
            // tachiyomi.extension.class meta-data (attribute order varies)
            val className = Regex("android:name=\"tachiyomi\\.extension\\.class\"[^>]*android:value=\"([^\"]+)\"")
                .find(manifest)?.groupValues?.get(1)
                ?: Regex("android:value=\"([^\"]+)\"[^>]*android:name=\"tachiyomi\\.extension\\.class\"")
                    .find(manifest)?.groupValues?.get(1)
            if (className == null) {
                System.err.println("sandbox: ${apk.fileName} has no tachiyomi.extension.class meta-data; skipped")
                return@use null
            }
            ExtensionInfo(
                pkgName = meta.packageName ?: apk.fileName.toString(),
                name = meta.label ?: apk.fileName.toString(),
                lang = extractLang(apk.fileName.toString()),
                versionName = meta.versionName ?: "0",
                className = className,
                extensionId = (extensions.size + 1).toLong(),
            )
        }
    }

    /** "tachiyomi-all.nhentaicom-v1.4.10.apk" -> "all" */
    private fun extractLang(fileName: String): String {
        val m = Regex("tachiyomi-([a-z0-9]+)\\.").find(fileName)
        return m?.groupValues?.get(1) ?: "all"
    }

    fun toExtensionsJson(): String {
        val parts = extensions.values.joinToString(",") { e ->
            """{"pkgName":${jsonStr(e.pkgName)},"name":${jsonStr(e.name)},"lang":${jsonStr(e.lang)},"versionName":${jsonStr(e.versionName)},"className":${jsonStr(e.className)},"sources":[${sources.values.filter { it.extensionId == e.extensionId }.joinToString(",") { """{"id":${it.id},"name":${jsonStr(it.name)},"lang":${jsonStr(it.lang)}}""" }}]}"""
        }
        return "[$parts]"
    }

    fun toSourcesJson(): String {
        val parts = sources.values.joinToString(",") { s ->
            """{"id":${s.id},"name":${jsonStr(s.name)},"lang":${jsonStr(s.lang)},"extension":${s.extensionId}}"""
        }
        return "[$parts]"
    }

    fun driver(sourceId: Long): SourceDriver? = sources[sourceId]?.let { SourceDriver(it) }
}

data class ExtensionInfo(
    val pkgName: String,
    val name: String,
    val lang: String,
    val versionName: String,
    val className: String,
    val extensionId: Long,
)
