//! 加载扩展 APK 的 dalvik ClassLoader（照搬 Mihon 的做法）。
//!
//! 为什么用 `DelegateLastClassLoader` 而不是 `PathClassLoader`：
//! **父加载器优先 vs 子加载器优先**。扩展的 `eu.kanade.tachiyomi.**` 类必须由
//! 宿主提供（扩展编译时链接的就是这组类名，且不同扩展的 lib 版本不同），
//! 而 dalvik 默认的 `PathClassLoader` 是父优先 —— 它会把 `android.*` 等交给
//! bootclasspath，这一点正确；但对扩展自己的类也会先问父加载器，父加载器没有
//! 才回落到自己，行为上等价。真正需要 delegate-last 的场景是：宿主自己也带了
//! 一份同名的第三方库（如 okhttp），扩展内置的那份应当优先，避免版本错配。
//!
//! API 27 才有 `DelegateLastClassLoader`，minSdk 26 因此需要那 1 个版本的
//! backport（`PathClassLoader` + 手写 `loadClass` 顺序），与 Mihon 一致。

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
