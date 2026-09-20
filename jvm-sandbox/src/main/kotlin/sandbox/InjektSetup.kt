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
import eu.kanade.tachiyomi.source.sourcePreferences
import kotlinx.serialization.json.Json
import kotlinx.serialization.protobuf.ProtoBuf
import org.koin.core.context.startKoin
import org.koin.dsl.module
import uy.kohesive.injekt.api.InjektScope
import uy.kohesive.injekt.api.KoinRegistrar
import xyz.nulldev.androidcompat.androidimpl.CustomContext
import xyz.nulldev.androidcompat.androidimpl.FakePackageManager
import xyz.nulldev.androidcompat.info.ApplicationInfoImpl
import xyz.nulldev.androidcompat.io.AndroidFiles
import xyz.nulldev.androidcompat.pm.PackageController
import xyz.nulldev.androidcompat.service.ServiceSupport
import xyz.nulldev.ts.config.GlobalConfigManager

/** Application stub backed by the sandbox's persistent preference stores. */
class SandboxApp : Application() {
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

    // 扩展有两套拿到偏好的写法：`ConfigurableSource.getSourcePreferences()`（走
    // `PreferenceStores`）和 `Injekt.get<Application>().getSharedPreferences("source_$id", 0)`
    // （走这里）。两份存储的后果是「设置填了、请求仍说未登录」，所以两边都指向同一份。
    override fun getSharedPreferences(name: String, mode: Int): SharedPreferences = sourcePreferences(name)
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
        // 也要给 protobuf 编解码器：MangaPlus / Manga Million / Peppercarrot 用
        // `Injekt.get<ProtoBuf>()` 读站点接口。不注册时 Koin 抛 NoDefinitionFoundException，
        // 而它发生在扩展的 `<clinit>` 里 → 异常记在类上，之后该类永久 erroneous。
        // 必须写 `single<ProtoBuf>`：Koin 按 lambda 的推断类型注册，不写类型参数会把
        // `ProtoBuf.Companion` 注册进去，`get<ProtoBuf>()` 照样找不到。
        single<ProtoBuf> { ProtoBuf }
        // `CustomContext`（见 AndroidEnv.installSandboxContext）构造期按类型从 Koin 取这几
        // 个。上游由 `androidCompatModule()` 提供，但那个模块还带一条 `single<Context>`，
        // 会和上面的 SandboxApp 撞定义，所以这里只挑它要的几条。
        single { AndroidFiles() }
        single { ApplicationInfoImpl(GlobalConfigManager) }
        single { ServiceSupport() }
        single { PackageController() }
        single { FakePackageManager() }
        single { CustomContext() }
    }
    startKoin { modules(m) }
    uy.kohesive.injekt.Injekt = InjektScope(KoinRegistrar())
}
