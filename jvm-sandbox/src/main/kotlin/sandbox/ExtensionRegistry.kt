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

    /** extensionId -> 该扩展这次产出的源。归属在加载时就定下来，序列化时不再靠 id 反查。 */
    private val sourcesByExtension = ConcurrentHashMap<Long, List<LoadedSource>>()

    /** pkgName -> 扩展的 APK 路径（`/icon/{pkg}` 要回读 APK）。与 `extensions` 一起在 reload() 里清。 */
    private val apkFiles = ConcurrentHashMap<String, Path>()

    /** APK 文件名 -> 最近一次扫描的失败根因。见 `failures()`。 */
    private val loadFailures = ConcurrentHashMap<String, String>()

    private val loader = ExtensionLoader(rootDir, jarDir)

    /** 下一个可用的 extensionId；只在扩展真正注册成功时推进（见 `loadApk`）。`inspect()` 无锁读它。 */
    @Volatile
    private var nextExtensionId = 1L

    override val extensionCount: Int get() = extensions.size
    override val sourceCount: Int get() = sources.size

    override fun failures(): Map<String, String> = loadFailures.toMap()

    /**
     * `@Synchronized`：`scan()` 会清空重填 `extensions` / `sources` / `nextExtensionId` 三份
     * 状态，两轮扫描交错时后一轮清的正是前一轮刚写下的东西 —— 表现为扩展 id 重复、
     * 源挂到别的扩展名下。HTTP 宿主是多线程的（见 `Main` 的 executor），
     * 「扩展装好立刻 reload」叠上任何一次定时/手动 reload 就能撞上。
     * `reload()` 会回调本方法，Java 的同步块可重入，不会自锁。
     */
    @Synchronized
    fun scan() {
        if (!Files.isDirectory(rootDir)) {
            Files.createDirectories(rootDir)
        }
        // 排序 + catch Throwable：
        //  - 排序让「哪些 APK 加载失败」可复现（Files.list 的顺序不保证）；
        //  - dex2jar 产物被 JVM 校验器拒时抛的是 VerifyError / NoClassDefFoundError
        //    这类 **Error 而非 Exception**，只 catch Exception 会让单个坏 APK 顺着
        //    forEach 冒泡打断整轮扫描 —— 它之后的扩展全部不加载，`/reload` 还会回
        //    500 连带让安装失败。
        Files.list(rootDir).use { stream ->
            stream.filter { it.fileName.toString().endsWith(".apk") }
                .sorted(Comparator.comparing<Path, String> { it.fileName.toString() })
                .forEach { apk -> loadApkSafely(apk) }
        }
    }

    /** Drops all loaded extensions/sources and rescans the directory (hot reload). */
    @Synchronized
    override fun reload() {
        extensions.clear()
        sources.clear()
        sourcesByExtension.clear()
        apkFiles.clear()
        loadFailures.clear()
        nextExtensionId = 1L
        // 换掉类加载器：上一轮初始化失败的类在 JVM 里处于 erroneous 状态，之后每次
        // 触碰都直接抛 NoClassDefFoundError，重扫也救不回来（比如上一轮 Koin 还没起来）。
        loader.reset()
        scan()
        println(
            "sandbox: reload complete — ${extensions.size} extension(s), ${sources.size} source(s)" +
                if (loadFailures.isEmpty()) "" else ", ${loadFailures.size} failed",
        )
    }

    private fun loadApkSafely(apk: Path) {
        try {
            loadApk(apk)
        } catch (t: Throwable) {
            // 记最深一层 cause：顶层恒是 InvocationTargetException 这类包装，
            // 真因（NullPointerException / VerifyError / NoClassDefFoundError）
            // 只在 cause 链末尾。
            val root = generateSequence(t) { it.cause }.last()
            val msg = root.javaClass.name + (root.message?.let { ": $it" } ?: "")
            loadFailures[apk.fileName.toString()] = msg
            System.err.println("sandbox: failed to load $apk: $msg")
            t.printStackTrace()
        }
    }

    /**
     * Parses an arbitrary APK without installing it (used for external installs).
     * 落临时文件后按路径解析 —— `apk-parser` 只吃 File。
     */
    override fun inspect(apkBytes: ByteArray): ExtensionInfo? {
        val tmp = Files.createTempFile("ext-inspect-", ".apk")
        return try {
            Files.write(tmp, apkBytes)
            // 没注册过就不占号，报「下一个可用号」，与 Android 侧同形。
            readApkInfo(tmp, nextExtensionId)
        } finally {
            Files.deleteIfExists(tmp)
        }
    }

    private fun loadApk(apk: Path) {
        // 传 0 只是占位：这个包的 id 要等确定是「新包」还是「重扫到的老包」之后才定。
        val parsed = readApkInfo(apk, 0L)
            ?: throw IllegalStateException("not a tachiyomi extension (manifest unreadable)")
        // 重扫到已注册的包时**沿用**它原来的号。`extensions[pkgName] = info` 是覆盖写，
        // 换成新号会让旧号名下的源变成孤儿：`/sources` 里那些源还写着旧号，server 侧
        // 按号回查就找不到扩展，源列表整个挂空。
        val existing = extensions[parsed.pkgName]
        val info = parsed.copy(extensionId = existing?.extensionId ?: nextExtensionId)
        val loaded = loader.load(apk, info.className, info.extensionId)
        extensions[info.pkgName] = info
        apkFiles[info.pkgName] = apk
        sourcesByExtension[info.extensionId] = loaded
        loaded.forEach { sources[it.id] = it }
        // 加载失败会从这里抛出去，号不推进，号段里不留空洞
        if (existing == null) nextExtensionId++
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
    private fun readApkInfo(apk: Path, extensionId: Long): ExtensionInfo? {
        return ApkFile(apk.toFile()).use { apkFile ->
            val meta = apkFile.apkMeta ?: return@use null
            val manifest = apkFile.manifestXml ?: return@use null
            // tachiyomi.extension.class meta-data (attribute order varies)
            val className = metaValue(manifest, "tachiyomi.extension.class")
                ?: throw IllegalStateException("no tachiyomi.extension.class meta-data")
            val nsfw = metaValue(manifest, "tachiyomi.extension.nsfw")
            ExtensionInfo(
                pkgName = meta.packageName ?: apk.fileName.toString(),
                name = meta.label ?: apk.fileName.toString(),
                lang = extractLang(apk.fileName.toString()),
                versionName = meta.versionName ?: "0",
                className = className,
                extensionId = extensionId,
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
            val srcs = sourcesByExtension[e.extensionId].orEmpty()
            """{"pkgName":${jsonStr(e.pkgName)},"name":${jsonStr(e.name)},"lang":${jsonStr(e.lang)},"versionName":${jsonStr(e.versionName)},"className":${jsonStr(e.className)},"extensionId":${e.extensionId},"versionCode":${e.versionCode},"contentWarning":${e.contentWarning},"sources":[${srcs.joinToString(",") { """{"id":${it.id},"name":${jsonStr(it.name)},"lang":${jsonStr(it.lang)}}""" }}]}"""
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
