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
import com.googlecode.d2j.dex.Dex2jar
import java.net.URL
import java.net.URLClassLoader
import java.nio.file.Files
import java.nio.file.Path
import java.util.concurrent.ConcurrentHashMap

class ExtensionLoader(private val rootDir: Path, private val jarDir: Path) {
    private val loaders = ConcurrentHashMap<String, ClassLoader>()
    private val cache = ConcurrentHashMap<String, List<LoadedSource>>()

    /** Convert an APK to a JAR under [jarDir] (dex -> jar). */
    fun apkToJar(apk: Path): Path {
        val name = apk.fileName.toString().removeSuffix(".apk") + ".jar"
        val jar = jarDir.resolve(name)
        if (Files.exists(jar) && Files.size(jar) > 0) {
            return jar
        }
        Files.createDirectories(jarDir)
        // dex2jar (femtopedia 2.4.38, same pipeline as Suwayomi-Server). Its
        // output loads fine except a few <clinit>s that BytecodeFixer repairs
        // at class-load time. (enjarify trips the verifier on method bodies.)
        val reader = MultiDexFileReader.open(Files.readAllBytes(apk))
        val handler = BaksmaliBaseDexExceptionHandler()
        Dex2jar.from(reader)
            .withExceptionHandler(handler)
            .reUseReg(false)
            .topoLogicalSort()
            .skipDebug(true)
            .optimizeSynchronized(false)
            .printIR(false)
            .noCode(false)
            .skipExceptions(false)
            .dontSanitizeNames(true)
            .computeFrames(true)
            .to(jar)
        return jar
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
            BytecodeFixer.fix(bytes) { t -> hasDefaultCtor(t) }
        } catch (e: Exception) {
            bytes // leave untouched if ASM can't parse it
        }
        return defineClass(name, fixed, 0, fixed.size)
    }

    /** True when [internalName] declares a no-arg `<init>` (probes without initializing). */
    private fun hasDefaultCtor(internalName: String): Boolean {
        val binary = internalName.replace('/', '.')
        return try {
            Class.forName(binary, false, this).getDeclaredConstructor().also { it.isAccessible = true }
            true
        } catch (e: NoSuchMethodException) {
            false
        } catch (e: Throwable) {
            false
        }
    }

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
