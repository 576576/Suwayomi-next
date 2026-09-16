//! Android 侧扩展注册表：扩展来自**系统已安装**的 APK，直接在 ART 里加载。
//!
//! 与桌面 `ExtensionRegistry` 的分工：这里只实现「发现 + 加载」，
//! 路由与 JSON 契约由共享的 `sandbox.Router` 负责，输出必须与桌面逐字段一致。

package sandbox

import android.content.Context
import android.content.pm.ApplicationInfo
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.os.Build
import java.io.File
import java.util.concurrent.ConcurrentHashMap

/**
 * 用 `PackageManager` 发现 `tachiyomi.extension` 特性的包，再用 dalvik 的
 * ClassLoader 把它们的 dex 装进**本进程**，实例化 `Source` / `SourceFactory`。
 *
 * 与 Mihon 的取舍不同之处：这里不做签名信任校验（没有 `TrustExtension` 那套
 * 数据库），也不过滤 NSFW —— 因为这套界面的使用者就是服务器自己，
 * 而扩展能否安装本身已经由系统安装器把过关。
 */
class PackageManagerRegistry(private val context: Context) : SourceRegistry {

    private val extensions = ConcurrentHashMap<String, ExtensionInfo>() // pkgName -> info
    private val sources = ConcurrentHashMap<Long, LoadedSource>() // source id -> loaded
    private val sourcesByExtension = ConcurrentHashMap<Long, MutableList<LoadedSource>>()
    private var nextExtensionId = 1L

    @Volatile
    private var lastError: String? = null

    /** 上次 [scan] 时系统里已装扩展的指纹，供 [installedSignature] 比对。 */
    @Volatile
    internal var scannedSignature: String = ""
        private set

    override val extensionCount: Int get() = extensions.size
    override val sourceCount: Int get() = sources.size

    // ---- discovery ---------------------------------------------------------

    /**
     * `@Synchronized`：重扫会被两条线程路径触发 —— 宿主 HTTP 的 `POST /reload`
     * 与 App 侧「从系统安装器回来」的后台重扫，两者并发会撞坏 `nextExtensionId`
     * 的分配（ClassLoader 本身是并发安全的，但这个计数器不是）。
     * `reload()` 会回调 [scan]，Java 的同步块可重入，不会自锁。
     */
    @Synchronized
    fun scan() {
        val installed = installedExtensions()
        // 记下"这次扫描时系统长什么样"：判断要不要重扫要跟它比，而不是跟加载成功
        // 的那几个比 —— 某个扩展 dex 加载失败时它不在 `extensions` 里，拿后者比对
        // 会每次返回"变了"，于是每次切回前台都白扫一遍、还反复重试加载。
        scannedSignature = signatureOf(installed)
        installed.forEach { pkg ->
            try {
                loadExtension(pkg)
            } catch (t: Throwable) {
                lastError = "${pkg.packageName}: ${t.message}"
                System.err.println("extension-host: failed to load ${pkg.packageName}: $t")
            }
        }
    }

    /** 系统里现装的、带 `tachiyomi.extension` 特性的包（不加载，只枚举）。 */
    private fun installedExtensions(): List<PackageInfo> {
        val pm = context.packageManager
        val installed: List<PackageInfo> = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            pm.getInstalledPackages(PackageManager.PackageInfoFlags.of(PACKAGE_FLAGS.toLong()))
        } else {
            @Suppress("DEPRECATION")
            pm.getInstalledPackages(PACKAGE_FLAGS)
        }
        return installed.filter { isExtension(it) }
    }

    /** `pkg@versionCode` 排序拼接，作为"系统里装了哪些扩展"的指纹。 */
    private fun signatureOf(list: List<PackageInfo>): String =
        list.map { "${it.packageName}@${versionCodeOf(it)}" }.sorted().joinToString("|")

    /**
     * 系统侧已装扩展的指纹，用来判断**要不要重扫**。
     *
     * 之所以用指纹而不是生命周期回调：Android 的系统安装器是个**对话框样式**的
     * Activity，装扩展时宿主 Activity 只 `onPause` 不 `onStop`，靠"离开过 App"
     * 这类闸门判断不了；而指纹比对不依赖任何时序，也顺带覆盖了从 Mihon / adb
     * 装扩展的情况。
     */
    fun installedSignature(): String = signatureOf(installedExtensions())

    @Synchronized
    override fun reload() {
        // 先释放：清空引用后下一次 GC 才能回收旧的 ClassLoader（类的卸载靠它）
        extensions.clear()
        sources.clear()
        sourcesByExtension.clear()
        nextExtensionId = 1L
        lastError = null
        scan()
        android.util.Log.i(TAG, "reload complete — ${extensions.size} extension(s), ${sources.size} source(s)")
    }

    /**
     * 解析一个**未安装**的 APK 的元信息（`POST /inspect`）。
     * 用 `getPackageArchiveInfo` 读文件头，不需要装进系统。
     */
    override fun inspect(apkBytes: ByteArray): ExtensionInfo? {
        val tmp = File.createTempFile("ext-inspect-", ".apk", context.cacheDir)
        return try {
            tmp.writeBytes(apkBytes)
            val pm = context.packageManager
            val pkg = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                pm.getPackageArchiveInfo(tmp.absolutePath, PackageManager.PackageInfoFlags.of(PACKAGE_FLAGS.toLong()))
            } else {
                @Suppress("DEPRECATION")
                pm.getPackageArchiveInfo(tmp.absolutePath, PACKAGE_FLAGS)
            } ?: return null
            if (!isExtension(pkg)) return null
            readInfo(pkg, nextExtensionId)
        } finally {
            tmp.delete()
        }
    }

    // ---- loading -----------------------------------------------------------

    private fun loadExtension(pkg: PackageInfo) {
        val appInfo = pkg.applicationInfo ?: return
        val info = readInfo(pkg, nextExtensionId)
        val classLoader = DalvikExtensionClassLoader(appInfo.sourceDir, appInfo.nativeLibraryDir, context.classLoader)

        val loaded = ArrayList<LoadedSource>()
        for (raw in info.className.split(";")) {
            val name = raw.trim().let {
                if (it.startsWith(".")) pkg.packageName + it else it
            }
            if (name.isEmpty()) continue
            val instance = Class.forName(name, false, classLoader)
                .getDeclaredConstructor()
                .newInstance()
            // SourceFactory 一次产出多个 source；单个 Source 就是它自己
            val instances: List<Any> = when {
                findMethod(instance.javaClass, "createSources") != null ->
                    @Suppress("UNCHECKED_CAST")
                    (callMethod(instance, "createSources") as Collection<*>).map { it as Any }
                else -> listOf(instance)
            }
            for (src in instances) {
                val id = callGetter(src, "getId").toString().toLong()
                loaded += LoadedSource(
                    id = id,
                    name = callGetter(src, "getName")?.toString() ?: "",
                    lang = callGetter(src, "getLang")?.toString() ?: "all",
                    extensionId = info.extensionId,
                    instance = src,
                    // 扩展的模型类同样从扩展的 ClassLoader 里取（它自带一份）
                    sourceCls = classLoader.loadClass("eu.kanade.tachiyomi.source.Source"),
                    smangaCls = classLoader.loadClass("eu.kanade.tachiyomi.source.model.SMangaImpl"),
                    schapterCls = classLoader.loadClass("eu.kanade.tachiyomi.source.model.SChapterImpl"),
                    pageCls = classLoader.loadClass("eu.kanade.tachiyomi.source.model.Page"),
                    mangasPageCls = classLoader.loadClass("eu.kanade.tachiyomi.source.model.MangasPage"),
                )
            }
        }
        if (loaded.isEmpty()) return

        // 语言：单个 source 取它的 lang，多语言则记为 all（与 Mihon 一致）
        val langs = loaded.map { it.lang }.distinct()
        val resolved = info.copy(lang = if (langs.size == 1) langs.first() else "all")
        extensions[resolved.pkgName] = resolved
        sourcesByExtension[resolved.extensionId] = loaded.toMutableList()
        loaded.forEach { sources[it.id] = it }
        nextExtensionId++
        android.util.Log.i(TAG, "loaded ${loaded.size} source(s) from ${resolved.pkgName} (${resolved.name}/${resolved.versionName})")
    }

    /** 从 APK manifest 的 meta-data 读扩展元信息。 */
    private fun readInfo(pkg: PackageInfo, extensionId: Long): ExtensionInfo {
        val appInfo = pkg.applicationInfo
        val meta = appInfo?.metaData
        val pm = context.packageManager
        val label = appInfo?.let { pm.getApplicationLabel(it).toString() }
            ?.substringAfter("Tachiyomi: ") ?: pkg.packageName
        return ExtensionInfo(
            pkgName = pkg.packageName,
            name = meta?.getString(METADATA_NAME) ?: label,
            lang = "all",
            versionName = pkg.versionName ?: "0",
            className = meta?.getString(METADATA_SOURCE_CLASS) ?: "",
            extensionId = extensionId,
            versionCode = versionCodeOf(pkg),
            // Mihon 用 `tachiyomi.extension.nsfw` 标 NSFW；没有该 meta-data 视为 Safe
            contentWarning = nsfwFlag(meta),
        )
    }

    @Suppress("DEPRECATION")
    private fun versionCodeOf(pkg: PackageInfo): Long =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) pkg.longVersionCode else pkg.versionCode.toLong()

    /**
     * `tachiyomi.extension.nsfw` 的**值是 `true` 还是 `1` 不统一**（keiyoushi 的
     * 扩展写的是整数 1）。`Bundle.getBoolean` 碰到 Integer 不会转换，而是打一条
     * "expected Boolean but value was a java.lang.Integer" 警告后返回默认 false ——
     * 结果就是 NSFW 扩展被静默当成 Safe。所以这里自己判类型。
     */
    private fun nsfwFlag(meta: android.os.Bundle?): Int {
        val on = when (val v = meta?.get(METADATA_NSFW)) {
            is Boolean -> v
            is Number -> v.toInt() != 0
            is String -> v.equals("true", ignoreCase = true) || v == "1"
            else -> false
        }
        return if (on) 1 else 0
    }

    // ---- SourceRegistry ----------------------------------------------------

    override fun toExtensionsJson(): String {
        val parts = extensions.values.joinToString(",") { e ->
            val srcs = sourcesByExtension[e.extensionId].orEmpty()
            """{"pkgName":${jsonStr(e.pkgName)},"name":${jsonStr(e.name)},"lang":${jsonStr(e.lang)},"versionName":${jsonStr(e.versionName)},"className":${jsonStr(e.className)},"versionCode":${e.versionCode},"contentWarning":${e.contentWarning},"sources":[${srcs.joinToString(",") { """{"id":${it.id},"name":${jsonStr(it.name)},"lang":${jsonStr(it.lang)}}""" }}]}"""
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

    /** 最近一次加载失败的原因（`/health` 之外的排障入口，日志里也会打）。 */
    fun lastError(): String? = lastError

    // ---- helpers -----------------------------------------------------------

    private fun isExtension(pkg: PackageInfo): Boolean =
        pkg.reqFeatures.orEmpty().any { it.name == EXTENSION_FEATURE }

    private companion object {
        const val TAG = "suwayomi-ext"
        const val EXTENSION_FEATURE = "tachiyomi.extension"
        const val METADATA_SOURCE_CLASS = "tachiyomi.extension.class"
        const val METADATA_NAME = "tachiyomix.name"
        const val METADATA_NSFW = "tachiyomi.extension.nsfw"

        @Suppress("DEPRECATION")
        val PACKAGE_FLAGS = PackageManager.GET_CONFIGURATIONS or
            PackageManager.GET_META_DATA or
            (if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) 0 else PackageManager.GET_SIGNATURES)
    }
}
