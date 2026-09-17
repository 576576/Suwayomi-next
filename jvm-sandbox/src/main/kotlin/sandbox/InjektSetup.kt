//! Injekt bootstrap for Mihon/Tachiyomi extensions.
//!
//! Extensions compiled against the Mihon lib call `injektLazy()` /
//! `Injekt.get<T>()` for their dependencies (network helper, preferences).
//! The host must provide those instances. Suwayomi uses the
//! injekt-koin bridge (`com.github.null2264:injekt-koin`), whose
//! `KoinRegistrar` resolves every injection through the Koin global context.
//! We mirror that: register a Koin module, then swap the global Injekt scope.
package sandbox

import android.app.Application
import android.content.SharedPreferences
import eu.kanade.tachiyomi.network.NetworkHelper
import kotlinx.serialization.json.Json
import org.koin.core.context.startKoin
import org.koin.dsl.module
import uy.kohesive.injekt.api.InjektScope
import uy.kohesive.injekt.api.KoinRegistrar
import java.lang.reflect.InvocationHandler
import java.lang.reflect.Method
import java.lang.reflect.Proxy
import java.util.concurrent.ConcurrentHashMap

/** Application stub with an in-memory SharedPreferences store. */
class SandboxApp : Application() {
    private val stores = ConcurrentHashMap<String, SharedPreferences>()
    // Application 继承 ContextWrapper，`mBase` 没 attach 过（桌面没有 Activity 宿主），
    // 所有默认实现都会在 `mBase.xxx()` 上 NPE。keiyoushi 的库和不少扩展会直接用
    // getCacheDir/getFilesDir/getExternalCacheDir 做磁盘缓存，这里逐个给真实临时目录。
    private val cacheDir = java.io.File(System.getProperty("java.io.tmpdir"), "suwayomi-cache")
        .apply { mkdirs() }
    private val filesDir = java.io.File(System.getProperty("java.io.tmpdir"), "suwayomi-files")
        .apply { mkdirs() }
    private val externalCacheDir = java.io.File(System.getProperty("java.io.tmpdir"), "suwayomi-external-cache")
        .apply { mkdirs() }
    private val externalFilesDir = java.io.File(System.getProperty("java.io.tmpdir"), "suwayomi-external-files")
        .apply { mkdirs() }

    override fun getCacheDir(): java.io.File = cacheDir

    override fun getFilesDir(): java.io.File = filesDir

    override fun getExternalCacheDir(): java.io.File = externalCacheDir

    override fun getExternalCacheDirs(): Array<java.io.File> = arrayOf(externalCacheDir)

    override fun getExternalFilesDir(type: String?): java.io.File = externalFilesDir

    override fun getExternalFilesDirs(type: String?): Array<java.io.File> = arrayOf(externalFilesDir)

    override fun getSharedPreferences(name: String, mode: Int): SharedPreferences =
        stores.computeIfAbsent(name) { memorySharedPreferences() }
}

/** Builds a `SharedPreferences` implemented as an in-memory map via dynamic proxy. */
fun memorySharedPreferences(): SharedPreferences {
    val data = ConcurrentHashMap<String, Any?>()
    val handler = InvocationHandler { _, method, args ->
        when (method.name) {
            "getString" -> data[args[0] as String] as? String ?: (args[1] as? String)
            "getStringSet" -> data[args[0] as String] as? Set<String> ?: (args[1] as? Set<String>)
            "getInt" -> (data[args[0] as String] as? Number)?.toInt() ?: (args[1] as? Int) ?: 0
            "getLong" -> (data[args[0] as String] as? Number)?.toLong() ?: (args[1] as? Long) ?: 0L
            "getFloat" -> (data[args[0] as String] as? Number)?.toFloat() ?: (args[1] as? Float) ?: 0f
            "getBoolean" -> data[args[0] as String] as? Boolean ?: (args[1] as? Boolean) ?: false
            "contains" -> data.containsKey(args[0] as String)
            "getAll" -> data
            "edit" -> memoryEditor(data)
            "registerOnSharedPreferenceChangeListener" -> null
            "unregisterOnSharedPreferenceChangeListener" -> null
            else -> null
        }
    }
    return Proxy.newProxyInstance(
        SharedPreferences::class.java.classLoader,
        arrayOf(SharedPreferences::class.java),
        handler,
    ) as SharedPreferences
}

/** Builds a `SharedPreferences.Editor` writing into [data]. */
private fun memoryEditor(data: MutableMap<String, Any?>): SharedPreferences.Editor {
    lateinit var editor: SharedPreferences.Editor
    val handler = InvocationHandler { _, method, args ->
        when (method.name) {
            // put*/remove/clear 按契约返回自己（`edit().remove(k).putString(…)` 这么链），
            // 返回 null 会让链上的下一个调用直接 NPE。
            "putString" -> { data[args[0] as String] = args[1] as String; editor }
            "putStringSet" -> { data[args[0] as String] = args[1] as Set<String>; editor }
            "putInt" -> { data[args[0] as String] = args[1] as Int; editor }
            "putLong" -> { data[args[0] as String] = args[1] as Long; editor }
            "putFloat" -> { data[args[0] as String] = args[1] as Float; editor }
            "putBoolean" -> { data[args[0] as String] = args[1] as Boolean; editor }
            "remove" -> { data.remove(args[0] as String); editor }
            "clear" -> { data.clear(); editor }
            "apply" -> { null }
            "commit" -> { true }
            else -> null
        }
    }
    editor = Proxy.newProxyInstance(
        SharedPreferences.Editor::class.java.classLoader,
        arrayOf(SharedPreferences.Editor::class.java),
        handler,
    ) as SharedPreferences.Editor
    return editor
}

/** Installs the injekt scope backed by a Koin module with sandbox singletons. */
fun setupInjekt() {
    val m = module {
        single { NetworkHelper() }
        val app = SandboxApp()
        single<Application> { app }
        single<android.content.Context> { app }
        // Extensions (Mihon lib) inject their JSON codec through injekt.
        single {
            Json {
                ignoreUnknownKeys = true
                coerceInputValues = true
                explicitNulls = false
            }
        }
    }
    startKoin { modules(m) }
    uy.kohesive.injekt.Injekt = InjektScope(KoinRegistrar())
}
