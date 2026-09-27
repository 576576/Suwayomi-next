//! 加载扩展 APK 的 dalvik ClassLoader（照搬 Mihon 的做法），用 `DelegateLastClassLoader`
//! 是为了**子加载器优先**：宿主自己也带了 okhttp 之类的库，扩展内置的那份应当优先。
//! API 27 才有它，26 走下面的 backport；详见 `docs/migration/ANDROID_IMPL.md` §C2。

package sandbox

import android.os.Build
import dalvik.system.DelegateLastClassLoader
import dalvik.system.PathClassLoader
import java.net.URL
import java.util.Collections
import java.util.Enumeration

@Suppress("FunctionName")
fun DalvikExtensionClassLoader(
    dexPath: String,
    librarySearchPath: String?,
    parent: ClassLoader,
): ClassLoader = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
    DelegateLastClassLoader(dexPath, librarySearchPath, parent)
} else {
    DelegateLastPathClassLoader(dexPath, librarySearchPath, parent)
}

/** [DelegateLastClassLoader] 的 API 26 backport。 */
private class DelegateLastPathClassLoader(
    dexPath: String,
    librarySearchPath: String?,
    parent: ClassLoader,
) : PathClassLoader(dexPath, librarySearchPath, parent) {

    private val bootClassLoader: ClassLoader? = Any::class.java.classLoader

    override fun loadClass(name: String?, resolve: Boolean): Class<*> {
        findLoadedClass(name)?.let { return it }

        if (bootClassLoader != null) {
            try {
                return bootClassLoader.loadClass(name)
            } catch (_: ClassNotFoundException) {
            }
        }

        val fromSuper = try {
            return findClass(name)
        } catch (e: ClassNotFoundException) {
            e
        }

        return try {
            parent.loadClass(name)
        } catch (_: ClassNotFoundException) {
            throw fromSuper
        }
    }

    override fun getResource(name: String?): URL? =
        bootClassLoader?.getResource(name)
            ?: findResource(name)
            ?: parent?.getResource(name)

    override fun getResources(name: String?): Enumeration<URL> {
        val resources = buildList {
            bootClassLoader?.getResources(name)?.let { addAll(it.toList()) }
            findResources(name)?.let { addAll(it.toList()) }
            parent?.getResources(name)?.let { addAll(it.toList()) }
        }
        return Collections.enumeration(resources)
    }
}
