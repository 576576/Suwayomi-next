//! Application —— 进程级启动顺序都在这里，顺序不能换：
//!
//!   1. 先起扩展宿主（回环 HTTP）。**必须早于** server：server 启动时要连它做
//!      `/health` 探测并拉 `/extensions`、`/sources`；晚起会得到「一个扩展都没有」
//!      的空目录（server 不会自己重试）。
//!   2. 再装 WebUI（assets/webui.zip → filesDir/webui），把目录交给 server。
//!   3. 最后 `NativeServer.start(...)` 启动 Rust server（JNI，异步）。
//!
//! App 退到后台不主动停 server：Android 会按需回收进程，`onTerminate` 也不可靠。
//! 需要真正停止时用 [shutdown]（例如设置页里的「退出」）。

package org.suwayomi.next

import android.app.Application
import android.content.Intent
import android.util.Log
import sandbox.ExtensionHost

class SuwayomiApp : Application() {

    /** server 监听端口（WebUI 也在这里）。 */
    var serverPort: Int = 0
        private set

    /** 扩展宿主端口（只对本进程有意义）。 */
    var extensionHostPort: Int = 0
        private set

    /** 启动失败的原因，供界面显示。 */
    var lastError: String? = null
        private set

    override fun onCreate() {
        super.onCreate()
        start();
    }

    private fun start() {
        if (NativeServer.load().not()) {
            lastError = "cannot load libsuwayomi_android.so (ABI 不是 arm64-v8a？)"
            return
        }

        // 1) 扩展宿主
        extensionHostPort = try {
            ExtensionHost.start(this, EXTENSION_HOST_PORT)
        } catch (t: Throwable) {
            Log.e(TAG, "extension host failed to start: $t", t)
            lastError = "扩展宿主启动失败：${t.message}"
            0
        }

        // 2) WebUI（version.txt 一致则跳过解压）
        val webui = WebUiInstaller.ensure(this)

        // 3) Rust server
        val code = NativeServer.start(
            /* dataDir    = */ filesDir.absolutePath,
            /* webuiDir   = */ webui.absolutePath,
            /* ip         = */ "127.0.0.1",
            /* port       = */ SERVER_PORT,
            /* sandboxUrl = */ if (extensionHostPort > 0) "http://127.0.0.1:$extensionHostPort" else "",
        )
        if (code != 0) {
            lastError = "server 启动被拒绝 (code=$code)，详见 logcat tag $TAG"
            Log.e(TAG, lastError!!)
            return
        }
        serverPort = SERVER_PORT
        Log.i(TAG, "Suwayomi ${NativeServer.version()} starting on 127.0.0.1:$SERVER_PORT (extensions on $extensionHostPort)")
    }

    /** 打开 WebUI 的 Activity。 */
    fun uiIntent(): Intent = Intent(this, MainActivity::class.java)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

    /**
     * 与系统实际已装扩展对比，有变化才重扫 —— 由界面在回到前台时调用
     * （[MainActivity.onResume]）。走系统安装器装的扩展只能靠重扫才能被宿主看见，
     * 而系统安装器是对话框样式、宿主 Activity 不会 `onStop`，所以判据是
     * 「已装扩展指纹变了」而不是生命周期事件。
     *
     * @return 有变化时返回重扫后的扩展数；无变化或宿主未启动返回 null。
     */
    @Synchronized
    fun refreshExtensionsIfChanged(): Int? =
        if (extensionHostPort > 0) ExtensionHost.syncIfChanged() else null

    /** 优雅停止 server 与扩展宿主（进程随后可正常回收）。 */
    @Synchronized
    fun shutdown() {
        if (serverPort != 0) {
            NativeServer.stop()
            serverPort = 0
        }
        ExtensionHost.stop()
        extensionHostPort = 0
    }

    companion object {
        private const val TAG = "Suwayomi"

        /** server 端口。Android 上不存在 8081-8280 那种保留段问题，取 4567 起的好记值。 */
        private const val SERVER_PORT = 4567

        /** 扩展宿主端口（仅本进程回环）。 */
        private const val EXTENSION_HOST_PORT = 4570
    }
}
