//! Extension loading core.
//!
//! Loads a Mihon/Tachiyomi extension (APK -> dex -> jar) with a child-first
//! class loader and drives it reflectively — the sandbox never compiles
//! against `eu.kanade.tachiyomi.*`, so it is independent of the library
//! version baked into each extension. Android stubs come from AndroidCompat
//! (on the system classpath); third-party libs (okhttp/jsoup/gson/...) are
//! provided by this process.

package sandbox

import com.googlecode.dex2jar.tools.BaksmaliBaseDexExceptionHandler
import com.googlecode.d2j.reader.MultiDexFileReader
import com.googlecode.d2j.dex.Dex2jar
import java.lang.reflect.InvocationTargetException
import java.net.URL
import java.net.URLClassLoader
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths
import java.util.concurrent.ConcurrentHashMap

/** A loaded extension source, driven through reflection. */
class LoadedSource(
    val id: Long,
    val name: String,
    val lang: String,
    val extensionId: Long,
    val instance: Any,
    val sourceCls: Class<*>,
    val smangaCls: Class<*>,
    val schapterCls: Class<*>,
    val pageCls: Class<*>,
    val mangasPageCls: Class<*>,
)

class ExtensionLoader(private val rootDir: Path) {
    private val loaders = ConcurrentHashMap<String, ClassLoader>()
    private val cache = ConcurrentHashMap<String, List<LoadedSource>>()

    /** Convert an APK to a JAR in the same directory (dex -> jar). */
    fun apkToJar(apk: Path): Path {
        val jar = apk.resolveSibling(apk.fileName.toString().removeSuffix(".apk") + ".jar")
        if (Files.exists(jar) && Files.size(jar) > 0) {
            return jar
        }
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

// ---------------------------------------------------------------------------
// reflective helpers
// ---------------------------------------------------------------------------

fun findMethod(cls: Class<*>, name: String, vararg paramTypes: Class<*>): java.lang.reflect.Method? =
    try {
        cls.getMethod(name, *paramTypes)
    } catch (e: NoSuchMethodException) {
        null
    }

fun callGetter(obj: Any, getterName: String): Any? {
    val m = findMethod(obj.javaClass, getterName) ?: return null
    return m.invoke(obj)
}

fun callMethod(obj: Any, name: String, vararg args: Any?): Any? {
    val types = args.map { primitiveOf(it?.javaClass ?: Any::class.java) }.toTypedArray()
    // try exact match first, then walk up to superclass methods
    var cls: Class<*>? = obj.javaClass
    while (cls != null) {
        val m = findMethod(cls, name, *types)
        if (m != null) {
            return try {
                m.invoke(obj, *args)
            } catch (e: InvocationTargetException) {
                throw RuntimeException("$name failed: ${e.cause?.message ?: e}", e.cause)
            }
        }
        cls = cls.superclass
    }
    // interface methods may be declared on a parent interface — try by name only
    val any = obj.javaClass.methods.firstOrNull { it.name == name && it.parameterCount == args.size }
        ?: throw RuntimeException("no method $name(${args.size} args) on ${obj.javaClass.name}")
    return any.invoke(obj, *args)
}

/** Maps a wrapper type to its primitive, if any (JVM methods use `int` etc.). */
private fun primitiveOf(c: Class<*>): Class<*> = when (c) {
    java.lang.Integer::class.java -> java.lang.Integer.TYPE
    java.lang.Long::class.java -> java.lang.Long.TYPE
    java.lang.Float::class.java -> java.lang.Float.TYPE
    java.lang.Double::class.java -> java.lang.Double.TYPE
    java.lang.Boolean::class.java -> java.lang.Boolean.TYPE
    else -> c
}

/** Reflectively builds a tachiyomi SManga/SChapter/Page instance from a JSON-ish map. */
fun buildModel(cls: Class<*>, fields: Map<String, Any?>): Any {
    val ctor = try {
        cls.getDeclaredConstructor()
    } catch (e: NoSuchMethodException) {
        // data class with all-default params still exposes a no-arg ctor in Kotlin 1.9+
        val c = cls.declaredConstructors.firstOrNull { it.parameterCount == 0 }
            ?: cls.declaredConstructors.minByOrNull { it.parameterCount }!!
        c.isAccessible = true
        return c.newInstance()
    }
    ctor.isAccessible = true
    val obj = ctor.newInstance()
    for ((k, v) in fields) {
        setField(obj, k, v)
    }
    return obj
}

fun setField(obj: Any, name: String, value: Any?) {
    val cls = obj.javaClass
    // walk up the class hierarchy
    var c: Class<*>? = cls
    while (c != null) {
        val f = try {
            c.getDeclaredField(name)
        } catch (e: NoSuchFieldException) {
            null
        }
        if (f != null) {
            f.isAccessible = true
            val converted = convert(f.type, value)
            f.set(obj, converted)
            return
        }
        c = c.superclass
    }
    // Kotlin data classes compile fields as private + getter/setter — try the setter
    val setterName = "set" + name.replaceFirstChar { it.uppercase() }
    val setter = findMethod(cls, setterName) ?: return
    setter.invoke(obj, convert(setter.parameterTypes[0], value))
}

fun convert(target: Class<*>, value: Any?): Any? {
    if (value == null) return null
    return when {
        target.isInstance(value) -> value
        target == java.lang.Long::class.java || target == java.lang.Long.TYPE -> (value as? Number)?.toLong() ?: 0L
        target == java.lang.Integer::class.java || target == java.lang.Integer.TYPE -> (value as? Number)?.toInt() ?: 0
        target == java.lang.Float::class.java || target == java.lang.Float.TYPE -> (value as? Number)?.toFloat() ?: 0f
        target == java.lang.Double::class.java || target == java.lang.Double.TYPE -> (value as? Number)?.toDouble() ?: 0.0
        target == java.lang.Boolean::class.java || target == java.lang.Boolean.TYPE -> (value as? Boolean) ?: false
        else -> value.toString()
    }
}

/** Kotlin data-class getter field read (e.g. `getUrl()`), walking the hierarchy. */
fun readField(obj: Any?, name: String): Any? {
    if (obj == null) return null
    val getter = findMethod(obj.javaClass, "get" + name.replaceFirstChar { it.uppercase() })
        ?: findMethod(obj.javaClass, "is" + name.replaceFirstChar { it.uppercase() })
    if (getter != null) return try { getter.invoke(obj) } catch (e: Exception) { null }
    var c: Class<*>? = obj.javaClass
    while (c != null) {
        val f = try {
            c.getDeclaredField(name)
        } catch (e: NoSuchFieldException) {
            null
        }
        if (f != null) {
            f.isAccessible = true
            return try { f.get(obj) } catch (e: Exception) { null }
        }
        c = c.superclass
    }
    return null
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
