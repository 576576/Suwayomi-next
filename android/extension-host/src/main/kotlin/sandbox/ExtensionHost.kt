//! Android 扩展宿主的对外门面。
//!
//! 职责：起一个只监听 127.0.0.1 的 HTTP 服务，把共享 `Router` 挂上去，
//! 供同进程内的 Rust server（经 `SUWAYOMI_SANDBOX_URL`）调用。
//!
//! 之所以是「同进程回环 HTTP」而不是直接 JNI 回调：契约（`/health`、`/extensions`、
//! `/sources`、`/source/{id}/…`）与 Rust 侧 `HttpSandboxFetcher` 已经在桌面被真实
//! 扩展验证过，Android 直接复用，Rust 端一行都不用改；而且它可以脱离 Rust 单独
//! 用 `adb shell curl` 调，排障成本低。见 docs/migration/ANDROID_IMPL.md D3。

package sandbox

import android.app.Application
import android.util.Log

object ExtensionHost {
    private const val TAG = "suwayomi-ext-host"

    @Volatile
    private var server: SimpleHttpServer? = null

    @Volatile
    private var registry: PackageManagerRegistry? = null

    /** 宿主 HTTP 是否已在监听。 */
    val isRunning: Boolean get() = server != null

    /** 实际监听的端口；未启动时为 0。 */
    val port: Int get() = server?.boundPort ?: 0

    /**
     * 启动扩展宿主。
     *
     * @param preferredPort 期望端口；被占用时由系统分配（用 [port] 读回真实值）。
     * @return 实际监听端口。
     */
    @Synchronized
    fun start(app: Application, preferredPort: Int): Int {
        server?.let { return it.boundPort }

        setupInjekt(app)
        val reg = PackageManagerRegistry(app)
        reg.scan()
        val srv = SimpleHttpServer(preferredPort, Router(reg))
        srv.start()

        registry = reg
        server = srv
        Log.i(TAG, "extension host listening on 127.0.0.1:${srv.boundPort} (${reg.extensionCount} extension(s), ${reg.sourceCount} source(s))")
        return srv.boundPort
    }

    /** 重新发现扩展（等价桌面的 `/reload`；不必重启 HTTP 服务）。 */
    @Synchronized
    fun reload(): Int {
        val reg = registry ?: return 0
        reg.reload()
        return reg.extensionCount
    }

    /**
     * 与系统实际已装的扩展对比，**有变化才重扫**（App 每次回到前台调一次）。
     *
     * 加载 dex 是几百毫秒的活，不能每次切回前台都做；枚举 `PackageManager`
     * 只是一次查询，代价可忽略。
     *
     * @return 有变化时返回重扫后的扩展数；无变化返回 null。
     */
    @Synchronized
    fun syncIfChanged(): Int? {
        val reg = registry ?: return null
        if (reg.installedSignature() == reg.scannedSignature) return null
        reg.reload()
        return reg.extensionCount
    }

    @Synchronized
    fun stop() {
        server?.stop()
        server = null
        registry = null
    }

    /** 最近一次扩展加载失败的原因，供界面/日志排障。 */
    fun lastError(): String? = registry?.lastError()
}
