//! 扫扩展之前要先把宿主环境补齐的三件事。
//!
//! AndroidCompat 的桩在桌面 JVM 上缺了「Android 运行时本来就该提供」的部分：主 Looper 没人
//! prepare，配置模块没人注册。两处都会让扩展在**构造期**直接倒下——而构造期抛出的异常会被
//! JVM 记在类上，之后该类永久 erroneous，重扫也救不回来，所以必须在 `scan()` 之前做完。
//! 宿主版本（`AppInfo`）也在这里注入：扩展会把它拼进 User-Agent，值必须来自宿主。
package sandbox

import android.content.Context
import android.os.Looper
import eu.kanade.tachiyomi.AppInfo
import java.util.concurrent.CountDownLatch
import org.koin.mp.KoinPlatformTools
import xyz.nulldev.androidcompat.androidimpl.CustomContext
import xyz.nulldev.androidcompat.config.ApplicationInfoConfigModule
import xyz.nulldev.androidcompat.config.FilesConfigModule
import xyz.nulldev.androidcompat.config.SystemConfigModule
import xyz.nulldev.ts.config.GlobalConfigManager

/**
 * 起一个跑真实 `Looper.loop()` 的线程，把 `Looper.getMainLooper()` 挂上去。
 *
 * 扩展会用 `Handler(Looper.getMainLooper())` 建 Handler（Komga 就是）。桌面 JVM 没有
 * Android 运行时、没人 prepare 过主 Looper，`getMainLooper()` 恒 null，`new Handler(null)`
 * 在 `Handler.<init>` 里解引用 `looper.mQueue` 直接 NPE。
 *
 * AndroidCompat 的 MessageQueue 是能跑的（poll 走 `Object.wait`，不会空转），所以这里真开一个
 * looper 线程：post 进去的任务会被执行，而不只是让 Looper 非 null。必须等它就绪再扫扩展。
 */
// Android 只在「由 Android 运行时创建主 Looper」这一层意义上把 prepareMainLooper 标成
// deprecated；桌面沙盒没有那个运行时，只能自己建。
@Suppress("DEPRECATION")
fun startMainLooper() {
    val ready = CountDownLatch(1)
    Thread(
        {
            Looper.prepareMainLooper()
            ready.countDown()
            // AndroidCompat 的 loopOnce 只 catch(Exception)，Error 会直接穿出 loop()。一个扩展
            // post 的任务里抛 Error（如 NoClassDefFoundError），主 looper 线程就整个死掉：之后
            // 所有 post 都没人执行，在 latch/await 上等的扩展只能等到超时。这里兜住并重新进入
            // 循环，把失败限制在那一次 post 上。队列被 quit 时 loop() 正常返回，此时不再重启。
            while (true) {
                try {
                    Looper.loop()
                    break
                } catch (t: Throwable) {
                    System.err.println("sandbox: exception escaped the main looper, continuing: $t")
                    t.printStackTrace()
                }
            }
        },
        "android-main-looper",
    ).apply { isDaemon = true }.start()
    ready.await()
}

/**
 * 把 AndroidCompat 的几个 config 模块注册进它的 `GlobalConfigManager`。
 *
 * `android.os.Build` / `android.os.SystemProperties` 的静态初始化会
 * `GlobalConfigManager.INSTANCE.module(SystemConfigModule::class.java)`，没注册就是空指针；
 * 于是凡是在 `headersBuilder()` 里读 `Build.MANUFACTURER` / `Build.VERSION.RELEASE` 的扩展
 * 都倒在 `NoClassDefFoundError: xyz.nulldev.ts.config.ConfigManager` 上。
 * 上游由 server 侧的 `AndroidCompatInitializer` 做这件事，沙盒里没有那个入口。
 *
 * `FilesConfigModule` / `ApplicationInfoConfigModule` 是 [installSandboxContext] 要用的
 * `CustomContext` 拉起来的（`AndroidFiles` / `ApplicationInfoImpl` 构造期就取配置）。
 *
 * 键值来自 AndroidCompat jar 自带的 `compat-reference.conf`（`android.system.isDebuggable`
 * 等）；`ConfigManager` 构造期还会读 `server.debugLogsEnabled`，那个键由本模块
 * `src/main/resources/server-reference.conf` 补上（真正的 server 默认配置不在这里）。
 */
fun registerAndroidCompatConfig() {
    val config = GlobalConfigManager.config
    GlobalConfigManager.registerModules(
        SystemConfigModule.register(config),
        FilesConfigModule.register(config),
        ApplicationInfoConfigModule.register(config),
    )
}

/**
 * 偏好项拿到的那个 `Context`。
 *
 * `Preference(context)` 只是把它存进字段，但扩展普遍会先 `screen.context` 做一次非空断言
 * 再传进去（Kotlin 的 `Intrinsics.checkNotNullParameter` 被 R8 内联成 `getClass()`），
 * 传 null 就是一句 `Cannot invoke "Object.getClass()" because ... is null` —— 设置页整个 500。
 *
 * 用 AndroidCompat 自带的 `CustomContext`：它是为这个宿主写的完整实现，代价是要在 Koin 里
 * 装好它依赖的四个单例（[setupInjekt] 里注册）。取不到就留 null，退回「扩展自己断言失败」
 * 的老行为，而不是让整个沙盒起不来。
 */
var sandboxContext: Context? = null
    private set

fun installSandboxContext() {
    sandboxContext = try {
        KoinPlatformTools.defaultContext().get().get<CustomContext>()
    } catch (t: Throwable) {
        // Koin 把构造失败包成一句没有因果链的 InstanceCreationException，不打全栈就只能猜。
        System.err.println("sandbox: CustomContext unavailable, preferences will run without a context")
        t.printStackTrace()
        null
    }
}

/**
 * 补上 Android 运行时会给 JVM 设的 `http.agent` 系统属性。
 *
 * 扩展普遍这么取 User-Agent：`System.getProperty("http.agent")` 交给
 * `Intrinsics.checkNotNull`（copy manga 的 `headersBuilder()` 就是），桌面 JVM 没有这个属性、
 * 拿到 null，扩展在**构造期**就 NPE —— 和主 Looper 一样属于「AndroidRuntime 本来就提供、
 * 桩里没有」的东西，必须在 `scan()` 之前补。
 *
 * 值按 Android 默认那个形状（真机上 Mihon 发的也是这个）；已经有值就不动。
 */
fun installHttpAgent() {
    if (System.getProperty("http.agent") == null) {
        System.setProperty("http.agent", "Dalvik/2.1.0 (Linux; U; Android 13; Suwayomi-next jvm-sandbox)")
    }
}

/**
 * 把宿主版本注入扩展面（`AppInfo`）。
 *
 * 值由 server spawn 本进程时从 `suwayomi_core::version` 传进来（环境变量名与 `build.rs`
 * 注入的同名），直接跑 jar（没带环境变量）时保持 `0` / `"0"`：扩展只是拿它拼 User-Agent，
 * 编造一个版本号比报 0 更容易误导。
 */
fun installHostVersion(versionCode: String?, versionName: String?) {
    AppInfo.installVersion(versionCode?.toIntOrNull() ?: 0, versionName ?: "0")
}
