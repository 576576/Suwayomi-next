//! Extension registry — scans the APK directory for APKs, converts them to
//! jars under the jar directory (dex2jar), loads the Source classes and
//! exposes the sources to the HTTP router.
//!
//! **桌面专有**：扩展来自服务器自己管理的 `extensions/` 目录。Android 端的
//! 对应实现是 `android/extension-host` 里的 `PackageManagerRegistry`（扩展来自
//! 系统已安装的包），两者实现共享的 `SourceRegistry`，JSON 输出必须一致。

package sandbox

import net.dongliu.apk.parser.ApkFile
import java.nio.file.Files
import java.nio.file.Path
import java.util.concurrent.ConcurrentHashMap
import java.util.zip.ZipFile

class ExtensionRegistry(private val rootDir: Path, private val jarDir: Path) : SourceRegistry {
    val extensions = ConcurrentHashMap<String, ExtensionInfo>() // pkgName -> info
    val sources = ConcurrentHashMap<Long, LoadedSource>() // source id -> loaded

    /** pkgName -> 扩展的 APK 路径（`/icon/{pkg}` 要回读 APK）。与 `extensions` 一起在 reload() 里清。 */
    private val apkFiles = ConcurrentHashMap<String, Path>()

    private val loader = ExtensionLoader(rootDir, jarDir)

    override val extensionCount: Int get() = extensions.size
    override val sourceCount: Int get() = sources.size

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

    /** Drops all loaded extensions/sources and rescans the directory (hot reload). */
    override fun reload() {
        extensions.clear()
        sources.clear()
        apkFiles.clear()
        scan()
        println("sandbox: reload complete — ${extensions.size} extension(s), ${sources.size} source(s)")
    }

    /**
     * Parses an arbitrary APK without installing it (used for external installs).
     * 落临时文件后按路径解析 —— `apk-parser` 只吃 File。
     */
    override fun inspect(apkBytes: ByteArray): ExtensionInfo? {
        val tmp = Files.createTempFile("ext-inspect-", ".apk")
        return try {
            Files.write(tmp, apkBytes)
            readApkInfo(tmp)
        } finally {
            Files.deleteIfExists(tmp)
        }
    }

    private fun loadApk(apk: Path) {
        val info = readApkInfo(apk) ?: return
        val loaded = loader.load(apk, info.className, info.extensionId)
        extensions[info.pkgName] = info
        apkFiles[info.pkgName] = apk
        loaded.forEach { sources[it.id] = it }
        println("sandbox: loaded ${loaded.size} source(s) from ${apk.fileName} (${info.name}/${info.versionName})")
    }

    /** 扩展 APK 里的图标。 */
    @Suppress("DEPRECATION") // `ApkMeta.icon` 在 apk-parser 里被标了 deprecated，但没有替代品
    override fun icon(pkgName: String): ByteArray? {
        val apk = apkFiles[pkgName] ?: return null
        if (!Files.isRegularFile(apk)) return null

        // manifest 里 `android:icon` 那份：apk-parser 会把 `@mipmap/ic_launcher`
        // 这类引用解析成 APK 内的具体路径
        val declared = ApkFile(apk.toFile()).use { apkFile ->
            val path = apkFile.apkMeta?.icon
            // 显式写 getFileData：Kotlin 的合成属性会把 fileData(...) 解析到
            // ApkFile 那个无参的 protected fileData() 上
            if (path.isNullOrEmpty()) null else runCatching { apkFile.getFileData(path) }.getOrNull()
        }
        if (declared != null && isImage(declared)) return declared

        // 自适应图标的 `android:icon` 指向 XML，上面拿到的是文本不是图片；
        // 退回 zip 里翻 `ic_launcher` 的图片条目，取最大的那张
        return ZipFile(apk.toFile()).use { zip ->
            zip.entries()
                .asSequence()
                .filter { !it.isDirectory && it.name.startsWith("res/") && it.name.contains("ic_launcher") }
                .filter { IMAGE_SUFFIXES.any(it.name::endsWith) }
                .maxByOrNull { it.size }
                ?.let { entry -> runCatching { zip.getInputStream(entry).use { it.readBytes() } }.getOrNull() }
        }
    }

    /** 拿到的到底是不是图片（自适应图标那条路会拿到 XML 文本）。 */
    private fun isImage(bytes: ByteArray): Boolean =
        when {
            bytes.size > 8 && bytes[0] == 0x89.toByte() && bytes[1] == 'P'.code.toByte() -> true // PNG
            bytes.size > 3 && bytes[0] == 0xFF.toByte() && bytes[1] == 0xD8.toByte() -> true // JPEG
            bytes.size > 12 && String(bytes, 0, 4, Charsets.US_ASCII) == "RIFF" -> true // WebP
            else -> false
        }

    /** Reads pkg info + the Source class name from the APK manifest. */
    private fun readApkInfo(apk: Path): ExtensionInfo? {
        return ApkFile(apk.toFile()).use { apkFile ->
            val meta = apkFile.apkMeta ?: return@use null
            val manifest = apkFile.manifestXml ?: return@use null
            // tachiyomi.extension.class meta-data (attribute order varies)
            val className = metaValue(manifest, "tachiyomi.extension.class")
            if (className == null) {
                System.err.println("sandbox: ${apk.fileName} has no tachiyomi.extension.class meta-data; skipped")
                return@use null
            }
            val nsfw = metaValue(manifest, "tachiyomi.extension.nsfw")
            ExtensionInfo(
                pkgName = meta.packageName ?: apk.fileName.toString(),
                name = meta.label ?: apk.fileName.toString(),
                lang = extractLang(apk.fileName.toString()),
                versionName = meta.versionName ?: "0",
                className = className,
                extensionId = (extensions.size + 1).toLong(),
                versionCode = meta.versionCode,
                contentWarning = if (nsfw == "true" || nsfw == "1") 1 else 0,
            )
        }
    }

    /** 读一个 `<meta-data android:name="…">` 的 value —— XML 里属性顺序不固定。 */
    private fun metaValue(manifest: String, name: String): String? {
        val n = Regex.escape(name)
        return Regex("android:name=\"$n\"[^>]*android:value=\"([^\"]*)\"").find(manifest)?.groupValues?.get(1)
            ?: Regex("android:value=\"([^\"]*)\"[^>]*android:name=\"$n\"").find(manifest)?.groupValues?.get(1)
    }

    /** "tachiyomi-all.nhentaicom-v1.4.10.apk" -> "all" */
    private fun extractLang(fileName: String): String {
        val m = Regex("tachiyomi-([a-z0-9]+)\\.").find(fileName)
        return m?.groupValues?.get(1) ?: "all"
    }

    override fun toExtensionsJson(): String {
        val parts = extensions.values.joinToString(",") { e ->
            """{"pkgName":${jsonStr(e.pkgName)},"name":${jsonStr(e.name)},"lang":${jsonStr(e.lang)},"versionName":${jsonStr(e.versionName)},"className":${jsonStr(e.className)},"versionCode":${e.versionCode},"contentWarning":${e.contentWarning},"sources":[${sources.values.filter { it.extensionId == e.extensionId }.joinToString(",") { """{"id":${it.id},"name":${jsonStr(it.name)},"lang":${jsonStr(it.lang)}}""" }}]}"""
        }
        return "[$parts]"
    }

    override fun toSourcesJson(): String {
        val parts = sources.values.joinToString(",") { s ->
            """{"id":${s.id},"name":${jsonStr(s.name)},"lang":${jsonStr(s.lang)},"extension":${s.extensionId}}"""
        }
        return "[$parts]"
    }

    override fun driver(sourceId: Long): SourceDriver? = sources[sourceId]?.let { SourceDriver(it) }

    private companion object {
        val IMAGE_SUFFIXES = listOf(".png", ".webp", ".jpg", ".jpeg")
    }
}
