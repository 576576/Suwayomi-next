//! 主界面：一个全屏 WebView，指向同进程 server 的 127.0.0.1。
//!
//! 为什么不做原生界面：WebUI 已经是完整的管理界面（书架、扩展、设置……），
//! 在 Android 上再实现一遍没有意义，而且会让上游 WebUI 的改动无法自动同步过来。
//!
//! 用裸 `android.app.Activity` 而不是 AppCompat：这里没有任何 AppCompat 特性
//! 依赖（无主题兼容需求、无 AppCompat 控件），少一层依赖就少一层体积与启动开销。
//!
//! 除了「打开 WebUI」，本 Activity 还是**扩展 APK 的入口**：用户在文件管理器里
//! 点开 APK、或在分享菜单里选 Suwayomi，都会带 `ACTION_VIEW` / `ACTION_SEND`
//! 进来，App 只把它转交系统安装器，自己不装（见 ExtensionInstaller）。

package suwayomi.android

import android.annotation.SuppressLint
import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.util.Log
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.WebResourceRequest
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import android.widget.TextView

class MainActivity : Activity() {
    private var webView: WebView? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val app = application as SuwayomiApp

        val error = app.lastError
        if (error != null) {
            setContentView(fallbackView(error))
            return
        }
        val port = app.serverPort
        if (port == 0) {
            setContentView(fallbackView("server 未启动"))
            return
        }

        val container = FrameLayout(this)
        val wv = buildWebView()
        container.addView(
            wv,
            FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            ),
        )
        setContentView(container)
        webView = wv
        wv.loadUrl("http://127.0.0.1:$port/")
        handleExtensionApk(intent)
    }

    /**
     * `launchMode="singleTask"`：App 已在运行时再点一个 APK，走这里而不是新开实例。
     */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleExtensionApk(intent)
    }

    /**
     * 把进来的扩展 APK 转交系统安装器。
     *
     * `ACTION_SEND`（分享菜单）带的是 `EXTRA_STREAM`，`ACTION_VIEW`（点开文件）
     * 带的是 `data`；两者都需要 `content://`（FileProvider / DocumentsUI 给的
     * 都是 content URI）—— `apkFromViewIntent` 负责落一份到 cacheDir。
     */
    private fun handleExtensionApk(intent: Intent?) {
        intent ?: return
        val data = when (intent.action) {
            Intent.ACTION_VIEW -> intent.data
            Intent.ACTION_SEND -> {
                @Suppress("DEPRECATION")
                intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)
            }
            else -> return
        } ?: return
        // 只有明确标成 APK 的才处理（别的 VIEW 请求照常忽略）
        if (intent.type != APK_MIME && intent.type != null) return

        val apk = ExtensionInstaller.apkFromViewIntent(this, data) ?: return
        Log.i(TAG, "handing ${apk.name} (${apk.length()} bytes) to the system installer")
        ExtensionInstaller.install(this, apk)
    }

    /**
     * 回到前台时对一次账：系统里已装的扩展和宿主已加载的是否一致，不一致就重扫。
     *
     * 装/卸扩展都会经过系统**对话框样式**的安装器（宿主 Activity 只 `onPause`、
     * 不 `onStop`），所以判断依据只能是「已装扩展指纹变了」，不能是生命周期事件。
     * 放后台线程：枚举 + 可能的 dex 重载都不该卡住 WebView。
     */
    override fun onResume() {
        super.onResume()
        Thread({
            val count = (application as SuwayomiApp).refreshExtensionsIfChanged()
            if (count != null) Log.i(TAG, "installed extensions changed; rescan found $count extension(s)")
        }, "extension-rescan").start()
    }

    @SuppressLint("SetJavaScriptEnabled")
    private fun buildWebView(): WebView {
        val wv = WebView(this)
        wv.settings.apply {
            javaScriptEnabled = true
            domStorageEnabled = true
            databaseEnabled = true
            // WebUI 是本地文件服务，不需要被当成「不安全内容」拦下
            mixedContentMode = WebSettings.MIXED_CONTENT_COMPATIBILITY_MODE
            // 图片/章节内容可能来自 http:// 的图床
            @Suppress("DEPRECATION")
            allowFileAccess = false
            loadWithOverviewMode = true
            useWideViewPort = true
        }
        CookieManager.getInstance().setAcceptCookie(true)
        wv.webViewClient = object : WebViewClient() {
            /**
             * 只在本机 server 内部导航；其余交给系统浏览器 —— 扩展站点/下载链接
             * 交给外部处理比在 WebView 里更靠谱（登录态、下载器都能用）。
             */
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                val url = request.url
                if (url.host == "127.0.0.1" || url.host == "localhost") return false
                return try {
                    startActivity(Intent(Intent.ACTION_VIEW, url))
                    true
                } catch (e: ActivityNotFoundException) {
                    Log.w(TAG, "no activity for $url")
                    false
                }
            }
        }
        return wv
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        webView?.saveState(outState)
    }

    /** server 还没起来/起不来时的兜底界面（比一片白屏有用）。 */
    private fun fallbackView(message: String): TextView = TextView(this).apply {
        text = "$message\n\n请检查 logcat（tag Suwayomi）后重启应用。"
        setPadding(48, 96, 48, 48)
        textSize = 16f
    }

    companion object {
        private const val TAG = "Suwayomi"
        private const val APK_MIME = "application/vnd.android.package-archive"
    }
}
