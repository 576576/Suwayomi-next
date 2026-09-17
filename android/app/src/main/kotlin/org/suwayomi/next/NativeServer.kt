//! Rust server 的 JNI 绑定。
//!
//! 导出的符号名由 JNI 规则决定（`Java_org_suwayomi_next_NativeServer_*`），与
//! `crates/suwayomi-android/src/lib.rs` 一一对应 —— **类名/包名/method 名任何一处
//! 不一致都会变成 UnsatisfiedLinkError**。
//!
//! 该 object 的 `init` 不主动 load：由 `SuwayomiApp` 在合适时机调用 [load]，
//! 好把 `loadLibrary` 的失败包成可上报的错误而不是静态初始化异常。

package org.suwayomi.next

import android.util.Log

object NativeServer {
    private const val TAG = "Suwayomi"
    private const val LIBRARY = "suwayomi_android"

    @Volatile
    private var loaded = false

    /** 加载 cdylib；重复调用无副作用。 */
    @Synchronized
    fun load(): Boolean {
        if (loaded) return true
        return try {
            System.loadLibrary(LIBRARY)
            loaded = true
            true
        } catch (t: Throwable) {
            Log.e(TAG, "cannot load lib$LIBRARY.so: $t")
            false
        }
    }

    /**
     * 启动 server（异步——返回 0 只代表已受理）。
     *
     * @return 0 已受理；2 字符串读取失败；3 端口非法；4 数据目录创建失败；
     *         5 重复 start；9 Rust 侧 panic。具体原因见 logcat（tag `Suwayomi`）。
     */
    external fun start(
        dataDir: String,
        webuiDir: String,
        ip: String,
        port: Int,
        sandboxUrl: String,
    ): Int

    /** 请求优雅关闭；0 = 已发出，1 = 尚未 start。 */
    external fun stop(): Int

    /** 编译期版本名（`r{versionCode}`；release/beta 是 `3.y.z`）。与 APK 的 `versionName` 同源。 */
    external fun version(): String
}
