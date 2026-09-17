package sandbox

import android.os.Looper
import com.sun.net.httpserver.HttpServer
import java.net.InetSocketAddress
import java.nio.file.Files
import java.nio.file.Paths
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors

/**
 * Suwayomi extension sandbox — runs Mihon/Tachiyomi extensions in a JVM
 * process isolated from the Rust server. Exposes a stable HTTP/JSON contract:
 *
 *   GET  /health                     -> {"ok":true}
 *   GET  /extensions                 -> [{pkgName, name, lang, versionName, className, sources:[{id,name,lang}]}]
 *   GET  /sources                    -> [{id,name,lang,extension}]
 *   GET  /source/{id}/manga?query=&page=   -> {mangas:[{url,title,thumbnailUrl,...}], hasNextPage}
 *   GET  /source/{id}/manga/{mangaUrl}     -> SManga json
 *   GET  /source/{id}/manga/{mangaUrl}/chapters -> [SChapter json]
 *   GET  /source/{id}/chapter/{chapterUrl}/pages -> [String urls]
 *   GET  /source/{id}/filters         -> [Filter json]
 *
 * 路由与 JSON 契约在 `extension-runtime` 共享；这里只负责**桌面侧**的两件事：
 * 进程入口（读环境变量、扫 `extensions/` 目录）与 `com.sun.net.httpserver` 宿主。
 */
fun main() {
    val port = System.getenv("SUWAYOMI_SANDBOX_PORT")?.toIntOrNull() ?: 4569
    val extensionsDir = System.getenv("SUWAYOMI_EXTENSIONS_DIR") ?: "extensions"
    // Converted jars live in a separate directory (bin/extensions in the
    // release layout) so the extensions dir only holds the downloaded APKs.
    val jarDir = System.getenv("SUWAYOMI_JAR_DIR")
        ?: Paths.get(extensionsDir).parent?.resolve("bin/extensions")?.toString()
        ?: "bin/extensions"
    Files.createDirectories(Paths.get(extensionsDir))
    Files.createDirectories(Paths.get(jarDir))

    val registry = ExtensionRegistry(Paths.get(extensionsDir), Paths.get(jarDir))
    // 先装 injekt/Koin 再扫：扩展的 <clinit> 会用 injekt 取依赖，Koin 没起来
    // 会以 ExceptionInInitializerError 记在类上，之后该类永久不可用。
    setupInjekt()
    startMainLooper()
    registry.scan()
    val server = HttpServer.create(InetSocketAddress("127.0.0.1", port), 0)
    val router = Router(registry)

    // 所有路径都交给共享 Router 做整段匹配（/health、/jvm、/extensions、/sources、
    // /reload、/inspect、/source/{id}/…）。用根 context 而不是逐个 createContext，
    // 避免 `/sources` 与 `/source/` 的 longest-prefix 匹配歧义。
    server.createContext("/") { exchange -> exchange.dispatch(router) }
    // 多线程 executor：默认单线程会把所有请求（含 /health）串行排队——某个
    // 扩展的网络调用阻塞（慢/超时最长 30s）时 health 也卡死，Rust 监视器
    // 误判 sandbox 挂掉而反复 kill/重启。线程池让慢请求独占线程，health 常驻可响应。
    server.executor = Executors.newCachedThreadPool()
    server.start()
    println("suwayomi-jvm-sandbox listening on 127.0.0.1:$port (extensions dir: $extensionsDir, jar dir: $jarDir)")
}

/**
 * 起一个跑真实 `Looper.loop()` 的线程，把 `Looper.getMainLooper()` 挂上去。
 *
 * 扩展在构造期会用 `Handler(Looper.getMainLooper())` 建 Handler（Komga 就是）。桌面
 * JVM 没有 Android 运行时、没人 prepare 过主 Looper，`getMainLooper()` 恒 null，
 * `new Handler(null)` 在 `Handler.<init>` 里解引用 `looper.mQueue` 直接 NPE。
 *
 * AndroidCompat 的 MessageQueue 是能跑的（poll 走 `Object.wait`，不会空转），所以这里
 * 真开一个 looper 线程：post 进去的任务会被执行，而不只是让 Looper 非 null。
 * 必须等它就绪再扫扩展，否则拿到 null 的时序没保证。
 */
// Android 只在「由 Android 运行时创建主 Looper」这一层意义上把 prepareMainLooper 标成
// deprecated；桌面沙盒没有那个运行时，只能自己建。
@Suppress("DEPRECATION")
private fun startMainLooper() {
    val ready = CountDownLatch(1)
    Thread(
        {
            Looper.prepareMainLooper()
            ready.countDown()
            Looper.loop()
        },
        "android-main-looper",
    ).apply { isDaemon = true }.start()
    ready.await()
}
