//! 主界面：一个全屏 WebView，指向同进程 server 的 127.0.0.1。
//!
//! 用裸 `android.app.Activity`：这里没有任何 AppCompat 特性依赖，少一层依赖就少
//! 一层体积与启动开销。
//!
//! 除了「打开 WebUI」，本 Activity 还负责三件事：
//!  * 扩展 APK 的入口 —— 用户在文件管理器里点开 APK、或在分享菜单里选 Suwayomi，
//!    带 `ACTION_VIEW` / `ACTION_SEND` 进来，App 只把它转交系统安装器（见
//!    ExtensionInstaller），自己不装；
//!  * **等 server 真的开始监听**再加载 WebUI（见 [waitForServerThenLoad]）；
//!  * WebView 的原生桥 —— 「编辑存储位置」时要唤起系统的目录授权对话框
//!    （见 [DirectoryPicker]）。

package org.suwayomi.next

import android.annotation.SuppressLint
import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.util.Log
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.JavascriptInterface
import android.webkit.RenderProcessGoneDetail
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import android.widget.TextView
import org.json.JSONObject
import java.net.InetSocketAddress
import java.net.Socket

class MainActivity : Activity() {
    private var webView: WebView? = null
    private var container: FrameLayout? = null

    /** WebView 上方那层「正在启动 / 打不开」的提示（有它就不会是白屏）。 */
    private var overlay: View? = null

    private var directoryPicker: DirectoryPicker? = null

    /** 本页加载失败重试了几次（换页/重建会清零）。 */
    private var loadAttempts = 0

    /**
     * 加载代号：等 server 是异步的，期间用户可能又触发一次重建/重试。
     * 代号对不上就丢弃那次等待的结果，避免旧任务把新 WebView 覆盖掉。
     */
    private var loadGeneration = 0

    /** 等 server 起来之后再加载的那个 URL（`null` = 恢复上次的浏览位置）。 */
    private var pendingUrl: String? = null
    private var pendingState: Bundle? = null

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

        val frame = FrameLayout(this)
        setContentView(frame)
        container = frame

        // 只有 debug 包开 WebView 远程调试：配合
        // `adb forward tcp:9222 localabstract:webview_devtools_remote_<pid>`
        // 可以用 CDP 把 WebUI 当普通网页查。正式包不开。
        if ((applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE) != 0) {
            WebView.setWebContentsDebuggingEnabled(true)
        }

        directoryPicker = DirectoryPicker(this, ::replyPickResult)

        // 重建（旋转/换主题）时恢复上次浏览的位置，否则 WebUI 每次重建都掉回书架首页
        pendingState = savedInstanceState
        pendingUrl = if (savedInstanceState == null) "http://127.0.0.1:$port/" else null

        createWebView()
        waitForServerThenLoad()

        handleExtensionApk(intent)
    }

    // ---- 生命周期 ----------------------------------------------------------

    /**
     * `launchMode="singleTask"`：App 已在运行时再点一个 APK，走这里而不是新开实例。
     */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleExtensionApk(intent)
    }

    /**
     * 回到前台时做两件事：
     *  1. 扩展若被系统装/卸过就重扫一次（见 [SuwayomiApp.refreshExtensionsIfChanged]）；
     *  2. 补一次「所有文件访问」的授权判断 —— 那个系统设置页不一定把结果回给
     *     `onActivityResult`。
     */
    override fun onResume() {
        super.onResume()
        directoryPicker?.resume()
        Thread({
            val count = (application as SuwayomiApp).refreshExtensionsIfChanged()
            if (count != null) Log.i(TAG, "installed extensions changed; rescan found $count extension(s)")
        }, "extension-rescan").start()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        webView?.saveState(outState)
    }

    override fun onDestroy() {
        directoryPicker?.detach()
        // WebView 必须显式销毁：它是原生资源 + 持有 Activity 引用，交给 GC 会泄漏
        container?.removeAllViews()
        webView?.destroy()
        webView = null
        container = null
        overlay = null
        super.onDestroy()
    }

    // ---- 返回手势 ----------------------------------------------------------

    @Suppress("DEPRECATION", "OVERRIDE_DEPRECATION")
    override fun onBackPressed() {
        // API 33 以下走这条；33+ 由 OnBackInvokedCallback 接管（见 syncBackCallback）。
        // targetSdk ≥ 35 时 predictive back 强制开启，onBackPressed 不会被调用。
        if (!navigateBack()) super.onBackPressed()
    }

    /** WebView 能回退就回退；返回 true 表示这次返回被消费了。 */
    private fun navigateBack(): Boolean {
        val wv = webView ?: return false
        if (!wv.canGoBack()) return false
        wv.goBack()
        return true
    }

    /**
     * 注册/注销系统的返回回调。
     *
     * 只在 WebView 能回退时注册：不能回退就把手势交还给系统，才有预测式返回的关闭
     * 动画。历史变化时（`doUpdateVisitedHistory`）重新同步一次。
     */
    private fun syncBackCallback() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
        val callback = backCallback ?: return
        runCatching {
            if (webView?.canGoBack() == true) {
                onBackInvokedDispatcher.registerOnBackInvokedCallback(
                    android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT,
                    callback,
                )
            } else {
                onBackInvokedDispatcher.unregisterOnBackInvokedCallback(callback)
            }
        }.onFailure { Log.w(TAG, "cannot sync the back callback: $it") }
    }

    private val backCallback: android.window.OnBackInvokedCallback? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            android.window.OnBackInvokedCallback { navigateBack() }
        } else {
            null
        }

    // ---- 目录选择（编辑存储位置）--------------------------------------------

    /**
     * 原生桥给 WebUI 用。名字 `SuwayomiAndroid` 与
     * `src/lib/platform/AndroidBridge.ts` 一一对应，改一处必须改另一处。
     */
    private inner class Bridge {
        /** 供 WebUI 判断「我是不是跑在 Android 宿主里」。 */
        @JavascriptInterface
        fun platform(): String = "android"

        /**
         * 唤起系统「使用此文件夹」授权对话框。
         *
         * 本方法跑在 WebView 的 JavaBridge 线程上，而起 Activity、读权限都要在主
         * 线程，所以转一手。
         */
        @JavascriptInterface
        fun pickDirectory(requestId: String, initial: String) {
            runOnUiThread { directoryPicker?.request(requestId, initial) }
        }
    }

    /** [DirectoryPicker] 的回话：结果经 JS 全局函数交回 await 中的 Promise。 */
    private fun replyPickResult(requestId: String, result: PickResult) {
        val (path, error) = when (result) {
            is PickResult.Picked -> result.path to ""
            PickResult.Cancelled -> "" to ""
            is PickResult.Failed -> "" to result.message
        }
        val js = "window.__suwayomiPickDirectory(" +
            "${JSONObject.quote(requestId)},${JSONObject.quote(path)},${JSONObject.quote(error)})"
        webView?.post { webView?.evaluateJavascript(js, null) }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        directoryPicker?.onActivityResult(requestCode, resultCode, data)
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        directoryPicker?.onRequestPermissionsResult(requestCode, grantResults)
    }

    // ---- 扩展 APK ----------------------------------------------------------

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

    // ---- WebView -----------------------------------------------------------

    private fun createWebView() {
        val frame = container ?: return
        val wv = buildWebView()
        frame.addView(
            wv,
            0,
            FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            ),
        )
        webView = wv
    }

    /**
     * 等 server 真的在监听，再加载 WebUI。
     *
     * `NativeServer.start()` 是异步的（JNI 返回 0 只代表已受理），建 HTTP 监听之前
     * 还要连库、跑迁移、同步扩展，几秒起步。不等就 loadUrl 会连接被拒并停在白屏。
     */
    private fun waitForServerThenLoad() {
        val port = (application as SuwayomiApp).serverPort
        if (port == 0) return
        val generation = ++loadGeneration
        showOverlay("正在启动服务器…")

        Thread({
            val deadline = System.currentTimeMillis() + SERVER_WAIT_MS
            var reachable = false
            while (System.currentTimeMillis() < deadline && generation == loadGeneration) {
                reachable = try {
                    Socket().use { it.connect(InetSocketAddress(LOOPBACK, port), SOCKET_PROBE_MS) }
                    true
                } catch (_: Throwable) {
                    false
                }
                if (reachable) break
                Thread.sleep(PROBE_INTERVAL_MS)
            }
            runOnUiThread {
                if (generation != loadGeneration || isFinishing || isDestroyed) return@runOnUiThread
                if (!reachable) {
                    showFallback("服务器未在 ${SERVER_WAIT_MS / 1000} 秒内开始监听 127.0.0.1:$port")
                    return@runOnUiThread
                }
                loadPendingPage(port)
            }
        }, "server-wait").start()
    }

    private fun loadPendingPage(port: Int) {
        val wv = webView ?: return
        loadAttempts = 0
        val state = pendingState
        pendingState = null
        hideOverlay()
        if (state != null && wv.restoreState(state) != null) {
            return
        }
        wv.loadUrl(pendingUrl ?: "http://127.0.0.1:$port/")
    }

    /** 重试一次加载（server 起来得晚、或上一次加载被拒）。 */
    private fun retryLoad() {
        if (isFinishing || isDestroyed) return
        loadGeneration++
        waitForServerThenLoad()
    }

    /**
     * 渲染进程被杀之后**整套重建** WebView —— 这种情况下 `reload()` 救不回来，
     * 只能 destroy 掉换一个新的。
     */
    private fun rebuildWebView() {
        val frame = container ?: return
        overlay?.let { frame.removeView(it) }
        overlay = null
        webView?.let { old ->
            frame.removeView(old)
            old.destroy()
        }
        webView = null
        pendingState = null
        pendingUrl = "http://127.0.0.1:${(application as SuwayomiApp).serverPort}/"
        createWebView()
        syncBackCallback()
        waitForServerThenLoad()
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
            // 每次从服务端取 index.html：WebUI 换版本后文件名带 hash，缓存里的老
            // index.html 会指向已不存在的 assets
            cacheMode = WebSettings.LOAD_DEFAULT
        }
        // 页面绘制之前先铺上主题背景，别让深色模式下闪一下白
        wv.setBackgroundColor(windowBackgroundColor())

        CookieManager.getInstance().setAcceptCookie(true)
        wv.addJavascriptInterface(Bridge(), BRIDGE_NAME)
        wv.webViewClient = object : WebViewClient() {
            /**
             * 只在本机 server 内部导航；其余交给系统浏览器 —— 扩展站点/下载链接
             * 这样做登录态与下载器都能用上。
             */
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                val url = request.url
                if (url.host == LOOPBACK || url.host == "localhost") return false
                return try {
                    startActivity(Intent(Intent.ACTION_VIEW, url))
                    true
                } catch (e: ActivityNotFoundException) {
                    Log.w(TAG, "no activity for $url")
                    false
                }
            }

            override fun doUpdateVisitedHistory(view: WebView, url: String?, isReload: Boolean) {
                // 历史变了 → 「能不能返回」也变了 → 重新同步系统的返回回调
                syncBackCallback()
            }

            override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
                if (!request.isForMainFrame) return
                Log.w(TAG, "main frame failed: ${error.errorCode} ${error.description} (${request.url})")
                val port = (application as SuwayomiApp).serverPort
                if (loadAttempts < MAX_LOAD_ATTEMPTS) {
                    loadAttempts++
                    showOverlay("连接不上服务器，正在重试（$loadAttempts/$MAX_LOAD_ATTEMPTS）…")
                    view.postDelayed({ retryLoad() }, RETRY_DELAY_MS)
                } else {
                    showFallback("打不开 127.0.0.1:$port（${error.description}）")
                }
            }

            /**
             * 渲染进程被杀 —— 返回 true 表示「我自己处理了」，否则系统会直接
             * 杀掉整个 App（默认行为）。
             */
            override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
                Log.w(TAG, "render process gone (crashed=${detail.didCrash()}); rebuilding the WebView")
                rebuildWebView()
                return true
            }
        }
        return wv
    }

    // ---- 兜底界面（绝不给用户一片白）---------------------------------------

    /** 在 WebView 之上盖一层提示。 */
    private fun showOverlay(message: String) {
        val frame = container ?: return
        overlay?.let { frame.removeView(it) }
        val view = overlayTextView(message)
        frame.addView(
            view,
            FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            ),
        )
        overlay = view
    }

    private fun hideOverlay() {
        overlay?.let { container?.removeView(it) }
        overlay = null
    }

    /** server 起不来 / 页面打不开时的兜底界面：说明原因，点一下重试。 */
    private fun fallbackView(message: String): TextView = overlayTextView(message).apply {
        text = "$message\n\n请检查 logcat（tag $TAG）后重启应用。\n点这里重试。"
        setOnClickListener { retryLoad() }
    }

    private fun showFallback(message: String) {
        val frame = container ?: return
        overlay?.let { frame.removeView(it) }
        val view = fallbackView(message)
        frame.addView(
            view,
            FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            ),
        )
        overlay = view
    }

    private fun overlayTextView(message: String): TextView = TextView(this).apply {
        text = message
        setPadding(48, 96, 48, 48)
        textSize = 16f
        gravity = Gravity.CENTER
        setBackgroundColor(windowBackgroundColor())
        setTextColor(if (isNightMode()) Color.LTGRAY else Color.DKGRAY)
    }

    /** 当前主题的窗口背景色（`values-night` 里是深色）；解析不出来就按深浅色兜底。 */
    private fun windowBackgroundColor(): Int {
        val value = TypedValue()
        return if (theme.resolveAttribute(android.R.attr.windowBackground, value, true) &&
            value.type >= TypedValue.TYPE_FIRST_COLOR_INT &&
            value.type <= TypedValue.TYPE_LAST_COLOR_INT
        ) {
            value.data
        } else {
            if (isNightMode()) NIGHT_BACKGROUND else Color.WHITE
        }
    }

    private fun isNightMode(): Boolean =
        (resources.configuration.uiMode and android.content.res.Configuration.UI_MODE_NIGHT_MASK) ==
            android.content.res.Configuration.UI_MODE_NIGHT_YES

    companion object {
        private const val TAG = "Suwayomi"

        /** 与 WebUI 侧的 `window.SuwayomiAndroid` 对应。 */
        private const val BRIDGE_NAME = "SuwayomiAndroid"

        private const val APK_MIME = "application/vnd.android.package-archive"

        private const val LOOPBACK = "127.0.0.1"

        /** 等 server 起监听的上限。 */
        private const val SERVER_WAIT_MS = 30_000L

        private const val SOCKET_PROBE_MS = 300
        private const val PROBE_INTERVAL_MS = 150L

        /** 主框架加载失败后的重试次数/间隔。 */
        private const val MAX_LOAD_ATTEMPTS = 3
        private const val RETRY_DELAY_MS = 800L

        private const val NIGHT_BACKGROUND = 0xFF121212.toInt()
    }
}
