//! Extension loading core.
//!
//! Loads a Mihon/Tachiyomi extension (APK -> dex -> jar) with a child-first
//! class loader and drives it reflectively — the sandbox never compiles
//! against `eu.kanade.tachiyomi.*`, so it is independent of the library
//! version baked into each extension. Android stubs come from AndroidCompat
//! (on the system classpath); third-party libs (okhttp/jsoup/gson/...) are
//! provided by this process.
//!
//! **桌面专有**：APK→dex2jar→ASM 修复→自建 ClassLoader 这套管线只在桌面 JVM
//! 上成立。Android 宿主直接在 ART 里加载扩展 APK 的 dex（见 `android/extension-host`），
//! 不引用本文件。反射工具与 `LoadedSource` 已抽到 `extension-runtime` 共享。

package sandbox

import com.googlecode.dex2jar.tools.BaksmaliBaseDexExceptionHandler
import com.googlecode.d2j.reader.MultiDexFileReader
import org.objectweb.asm.ClassReader
import org.objectweb.asm.ClassVisitor
import org.objectweb.asm.MethodVisitor
import org.objectweb.asm.Opcodes
import org.objectweb.asm.Type
import java.net.URL
import java.net.URLClassLoader
import java.nio.file.FileSystems
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption
import java.util.concurrent.ConcurrentHashMap
import java.util.zip.ZipInputStream

private const val ASSETS_PREFIX = "assets/"

/**
 * dex→jar 产出内容的版本。**改动产出内容时必须 +1。**
 *
 * jar 按文件名缓存在 `bin/extensions`，上一版转换链转出来的 jar 不会自动被覆盖；不失效的话
 * 只有重装/升级过的扩展会吃到新产物，其余一直用旧 jar。启动时按戳清一次。
 */
private const val CONVERTER_VERSION = 1

private const val CONVERTER_STAMP = ".converter-version"

class ExtensionLoader(private val rootDir: Path, private val jarDir: Path) {
    private val loaders = ConcurrentHashMap<String, ClassLoader>()
    private val cache = ConcurrentHashMap<String, List<LoadedSource>>()

    init {
        Files.createDirectories(jarDir)
        val stamp = jarDir.resolve(CONVERTER_STAMP)
        val upToDate = try {
            Files.readString(stamp).trim() == CONVERTER_VERSION.toString()
        } catch (_: java.io.IOException) {
            false
        }
        if (!upToDate) {
            val stale = Files.list(jarDir).use { s ->
                s.filter { it.fileName.toString().let { n -> n.endsWith(".jar") || n.endsWith(".part") } }.toList()
            }
            stale.forEach { Files.deleteIfExists(it) }
            Files.writeString(stamp, CONVERTER_VERSION.toString())
        }
    }

    /** Convert an APK to a JAR under [jarDir] (dex -> jar). */
    fun apkToJar(apk: Path): Path {
        val name = apk.fileName.toString().removeSuffix(".apk") + ".jar"
        val jar = jarDir.resolve(name)
        if (Files.exists(jar) && Files.size(jar) > 0) {
            return jar
        }
        // 先写临时文件再原子替换：`DexTranslator` 直接写目标路径，进程中途崩掉会留下
        // 半截 jar，而缓存判据只看「存在且非空」，于是这个扩展此后永久报
        // ClassNotFoundException，重扫也救不回来。
        val tmp = jarDir.resolve("$name.part")
        val reader = MultiDexFileReader.open(Files.readAllBytes(apk))
        // dex2jar (femtopedia 2.4.38, same pipeline as Suwayomi-Server). Enjarify's
        // output trips the JVM verifier on method bodies; dex2jar's loads as long as
        // DexTranslator/BytecodeFixer repair what R8 leaves behind.
        DexTranslator.translate(reader, tmp, BaksmaliBaseDexExceptionHandler())
        copyAssets(apk, tmp)
        Files.move(tmp, jar, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE)
        return jar
    }

    /**
     * 把 APK 里 `assets/` 下的文件按**原路径**搬进 [jar]。
     *
     * dex2jar 只产出 class，不搬资源。扩展的多语言文案是靠
     * `classLoader.getResourceAsStream("assets/i18n/messages_xx.properties")` 从自己包里
     * 读的（keiyoushi 1.6 的 i18n 走这条路，前缀 `assets/` 也在查找名里），缺了资源
     * 拿到 null，往下就是 `InputStreamReader(null)` 的 NPE。
     */
    private fun copyAssets(apk: Path, jar: Path) {
        FileSystems.newFileSystem(jar).use { fs ->
            ZipInputStream(Files.newInputStream(apk)).use { zip ->
                var entry = zip.nextEntry
                while (entry != null) {
                    if (!entry.isDirectory && entry.name.startsWith(ASSETS_PREFIX)) {
                        val target = fs.getPath("/" + entry.name)
                        target.parent?.let { Files.createDirectories(it) }
                        Files.newOutputStream(target).use { zip.copyTo(it) }
                    }
                    entry = zip.nextEntry
                }
            }
        }
    }

    /** Loads the extension's sources (single Source or SourceFactory). */
    fun load(apk: Path, className: String, extensionId: Long): List<LoadedSource> {
        val key = apk.toString()
        cache[key]?.let { return it }

        val jar = apkToJar(apk)
        val classLoader = loaders[key] ?: ChildFirstURLClassLoader(arrayOf(jar.toUri().toURL()))
        loaders[key] = classLoader

        val clazz = Class.forName(className, false, classLoader)
        val instance = clazz.getDeclaredConstructor().newInstance()

        // SourceFactory? -> createSources()
        val factory = findMethod(clazz, "createSources")
        val sources: List<Any> = if (factory != null) {
            @Suppress("UNCHECKED_CAST")
            (factory.invoke(instance) as Collection<*>).map { it as Any }
        } else {
            listOf(instance)
        }

        val sourceIf = try {
            classLoader.loadClass("eu.kanade.tachiyomi.source.Source")
        } catch (e: ClassNotFoundException) {
            throw IllegalStateException("extension jar has no eu.kanade.tachiyomi.source.Source (bad dex2jar output?)", e)
        }
        val smangaCls = classLoader.loadClass("eu.kanade.tachiyomi.source.model.SMangaImpl")
        val schapterCls = classLoader.loadClass("eu.kanade.tachiyomi.source.model.SChapterImpl")
        val pageCls = classLoader.loadClass("eu.kanade.tachiyomi.source.model.Page")
        val mangasPageCls = classLoader.loadClass("eu.kanade.tachiyomi.source.model.MangasPage")

        val loaded = sources.map { src ->
            LoadedSource(
                id = callGetter(src, "getId").toString().toLong(),
                name = callGetter(src, "getName")?.toString() ?: "",
                lang = callGetter(src, "getLang")?.toString() ?: "all",
                extensionId = extensionId,
                instance = src,
                sourceCls = sourceIf,
                smangaCls = smangaCls,
                schapterCls = schapterCls,
                pageCls = pageCls,
                mangasPageCls = mangasPageCls,
            )
        }
        cache[key] = loaded
        return loaded
    }

    fun unload(apk: Path) {
        cache.remove(apk.toString())
        loaders.remove(apk.toString())
    }

    /** 丢弃所有已加载的类：重扫前调用，让每个扩展回到全新类加载器。 */
    fun reset() {
        cache.clear()
        loaders.clear()
    }
}

/**
 * Parent-last class loader.
 *
 * `eu.kanade.tachiyomi.*` (the extension interface set): the extension jar's
 * own copy wins when present (child-first), falling back to this process's
 * compiled interfaces. Everything else (android stubs, okhttp/jsoup/gson,
 * kotlin/java stdlib): resolved from the system class loader first, then the
 * child jar, then the parent — extensions do not bundle third-party libs.
 */
class ChildFirstURLClassLoader(
    urls: Array<URL>,
    parent: ClassLoader? = null,
) : URLClassLoader(urls, parent) {
    private val systemClassLoader: ClassLoader? = getSystemClassLoader()

    override fun findClass(name: String): Class<*> {
        // Read the raw bytes ourselves so R8-broken bytecode can be repaired.
        val resource = name.replace('.', '/') + ".class"
        val stream = getResourceAsStream(resource) ?: throw ClassNotFoundException(name)
        val bytes = stream.use { it.readBytes() }
        val fixed = try {
            BytecodeFixer.fix(bytes, hierarchy)
        } catch (e: Exception) {
            bytes // leave untouched if ASM can't parse it
        }
        return defineClass(name, fixed, 0, fixed.size)
    }

    /**
     * 回答字节码修复要问的类层次问题。
     *
     * **按字节回答，不加载类**：在 `findClass` 里 `Class.forName(父类)` 会形成
     * `loadClass→findClass→loadClass` 的环，父类还没 `defineClass` 就被再次
     * `findClass`，JVM 直接抛 ClassCircularityError。
     */
    private val hierarchy: BytecodeFixer.Hierarchy = BytesHierarchy(this, systemClassLoader)

    override fun loadClass(name: String?, resolve: Boolean): Class<*> {
        val n = name ?: throw ClassNotFoundException("null class name")
        var c = findLoadedClass(n)

        if (c == null && n.startsWith("eu.kanade.tachiyomi")) {
            // child-first for the extension API
            c = try {
                findClass(n)
            } catch (_: ClassNotFoundException) {
                null
            }
            if (c == null && systemClassLoader != null) {
                try {
                    c = systemClassLoader.loadClass(n)
                } catch (_: ClassNotFoundException) {
                }
            }
        }
        if (c == null && systemClassLoader != null) {
            try {
                c = systemClassLoader.loadClass(n)
            } catch (_: ClassNotFoundException) {
            }
        }
        if (c == null) {
            c = try {
                findClass(n)
            } catch (_: ClassNotFoundException) {
                super.loadClass(n, resolve)
            }
        }
        if (resolve) resolveClass(c)
        return c!!
    }
}

/**
 * 从 class 字节回答「某类会声明哪些可继承构造器」「某类是否在另一类的父类链上」。
 *
 * 结果必须与 [BytecodeFixer] 的实际行为一致：类自己没有构造器时，它会按父类构造器
 * 镜像补齐，所以这里的视图也要按同一条规则递归。
 *
 * 两条取数路径各有各的必要性：
 *  - 扩展 jar 里的类只按字节解析。反射加载会和 `findClass` 撞成
 *    `loadClass→findClass→loadClass` 的环（父类尚未 `defineClass` 就被再次
 *    `findClass`），JVM 抛 ClassCircularityError。
 *  - 宿主类（java/kotlin/okhttp/suwayomi 自身）优先按字节解析以省掉类加载，解析失败
 *    再反射——本项目编译出的 class 是 JDK 25（major 69），打包进来的 ASM 9.7 读不了。
 */
private class BytesHierarchy(private val loader: java.net.URLClassLoader, private val system: ClassLoader?) :
    BytecodeFixer.Hierarchy {

    private class Info(val superName: String?, val inits: List<Pair<String, Int>>)

    private val infos = java.util.concurrent.ConcurrentHashMap<String, Info>()
    private val views = java.util.concurrent.ConcurrentHashMap<String, List<String>>()
    private val missing = Info(null, emptyList())

    override fun constructorsOf(internalName: String): List<String> {
        views[internalName]?.let { return it }
        val info = info(internalName)
        val declared = info.inits.map { it.first }
        val inheritable = info.inits.filter { inheritable(it.second) }.map { it.first }
        // 有构造器就不用镜像；没有则整份继承父类的视图（与镜像规则一致）
        val view = if (declared.isNotEmpty()) inheritable else info.superName?.let { constructorsOf(it) } ?: emptyList()
        views[internalName] = view
        return view
    }

    override fun isAncestor(ancestor: String, of: String): Boolean {
        var cur = info(of).superName
        var guard = 0
        while (cur != null && guard++ < 128) {
            if (cur == ancestor) return true
            cur = info(cur).superName
        }
        return false
    }

    private fun info(internalName: String): Info {
        infos[internalName]?.let { return it }
        val resource = "$internalName.class"
        // `findResource` 只看扩展 jar 自己；`getResource` 会一路走到 jimage，
        // 那就不该按「扩展类」处理（读不了也没有理由反射加载）。
        val ownUrl = loader.findResource(resource)
        val info = if (ownUrl != null) {
            read(ownUrl)?.let { parse(it) } ?: missing
        } else {
            read(system?.getResource(resource))?.let { parse(it) } ?: reflected(internalName) ?: missing
        }
        infos[internalName] = info
        return info
    }

    private fun inheritable(modifiers: Int): Boolean =
        java.lang.reflect.Modifier.isPublic(modifiers) || java.lang.reflect.Modifier.isProtected(modifiers)

    private fun parse(bytes: ByteArray): Info? = try {
        val cr = ClassReader(bytes)
        val inits = ArrayList<Pair<String, Int>>()
        cr.accept(
            object : ClassVisitor(Opcodes.ASM9) {
                override fun visitMethod(
                    access: Int,
                    name: String?,
                    descriptor: String?,
                    signature: String?,
                    exceptions: Array<out String>?,
                ): MethodVisitor? {
                    if (name == "<init>" && descriptor != null) inits.add(descriptor to access)
                    return null
                }
            },
            ClassReader.SKIP_CODE or ClassReader.SKIP_DEBUG or ClassReader.SKIP_FRAMES,
        )
        Info(cr.superName, inits)
    } catch (e: Throwable) {
        null
    }

    /** 只用于**不在扩展 jar 里**的类，避免与本加载器的 `findClass` 形成环。 */
    private fun reflected(internalName: String): Info? = try {
        val cls = Class.forName(internalName.replace('/', '.'), false, loader)
        Info(
            cls.superclass?.name?.replace('.', '/'),
            cls.declaredConstructors.map { Type.getConstructorDescriptor(it) to it.modifiers },
        )
    } catch (e: Throwable) {
        null
    }

    private fun read(url: java.net.URL?): ByteArray? {
        if (url == null) return null
        return try {
            url.openStream().use { it.readBytes() }
        } catch (e: Throwable) {
            null
        }
    }
}
