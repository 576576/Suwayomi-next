//! WebView 里触发的下载由宿主代劳 —— 「创建备份」走的就是这条路。
//! 没设 `DownloadListener` 时点击是**静默无响应**（不报错、不回调、没有日志）。
//! 细节与两个坑见 `docs/migration/ANDROID_IMPL.md` §「创建备份」。

package org.suwayomi.next

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import android.util.Log
import android.webkit.CookieManager
import android.widget.Toast
import java.net.HttpURLConnection
import java.net.URL

/**
 * 把 WebView 的一次下载存到用户选定的位置（SAF 的 `ACTION_CREATE_DOCUMENT` = 「另存为」）。
 *
 * 落盘要自己回服务端再取一次文件，所以**必须带上 WebView 的 cookie**：设置页开了认证
 * 时裸请求只会拿到 302 登录页，那样写出来的 .tachibk 是一份打不开的 HTML。
 */
class DownloadSaver(private val activity: Activity) {
    /** 一次下载的待办；SAF 选完落点后按它去取文件。 */
    private class Pending(val url: String, val cookie: String?, val userAgent: String?, val filename: String)

    private var pending: Pending? = null

    /** `WebView.setDownloadListener` 里调用；返回 true 表示本类接管了这次下载。 */
    fun start(
        url: String,
        userAgent: String?,
        contentDisposition: String?,
        mimeType: String?,
    ): Boolean {
        // 上一次没选完落点的（用户连点两次）先丢掉，别让 SAF 回来时写错文件
        pending = null
        val filename = filenameOf(contentDisposition, url)
        // cookie 在主线程取（CookieManager 不是线程安全的），落盘交给后台线程
        pending = Pending(url, CookieManager.getInstance().getCookie(url), userAgent, filename)

        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = mimeType?.takeIf { it.isNotBlank() } ?: FALLBACK_MIME
            putExtra(Intent.EXTRA_TITLE, filename)
        }
        try {
            activity.startActivityForResult(intent, REQUEST_CREATE_FILE)
        } catch (e: ActivityNotFoundException) {
            // 极简 ROM 里可能没有 DocumentsUI：如实报错，但一定要清掉待办
            Log.w(TAG, "no activity handles ACTION_CREATE_DOCUMENT: $e")
            pending = null
            toast("系统里找不到「保存文件」的位置选择器")
        }
        return true
    }

    /** `Activity.onActivityResult` 里调用；返回 true 表示这次结果由本类消费了。 */
    fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        if (requestCode != REQUEST_CREATE_FILE) return false
        val job = pending
        pending = null
        val target = data?.data
        // 用户取消、或这次跳转期间 Activity 被重建过（pending 已空）—— 什么都不做
        if (job == null || resultCode != Activity.RESULT_OK || target == null) return true
        toast("正在保存 ${job.filename}…")
        Thread({ write(job, target) }, "backup-save").start()
        return true
    }

    /** Activity 销毁时调用：结果回来时 WebView 已经没了，别再落盘。 */
    fun detach() {
        pending = null
    }

    /** 回服务端取文件写进 SAF 给的 URI。失败一律报出来，并把空壳删掉。 */
    private fun write(job: Pending, target: Uri) {
        var conn: HttpURLConnection? = null
        try {
            conn = (URL(job.url).openConnection() as HttpURLConnection).apply {
                connectTimeout = TIMEOUT_MS
                readTimeout = TIMEOUT_MS
                // 302 是认证挑战（设置页开了密码），跟着跳会把登录页写成备份文件
                instanceFollowRedirects = false
                job.cookie?.let { setRequestProperty("Cookie", it) }
                job.userAgent?.let { setRequestProperty("User-Agent", it) }
            }
            if (conn.responseCode != HttpURLConnection.HTTP_OK) {
                fail(target, "保存失败：服务端返回 HTTP ${conn.responseCode}")
                return
            }
            val out = activity.contentResolver.openOutputStream(target)
            if (out == null) {
                fail(target, "保存失败：拿不到写入位置")
                return
            }
            out.use { sink -> conn.inputStream.use { source -> source.copyTo(sink) } }
            Log.i(TAG, "saved ${job.filename} from ${job.url}")
            toast("备份已保存：${job.filename}")
        } catch (t: Throwable) {
            Log.w(TAG, "cannot save ${job.url} to $target", t)
            fail(target, "保存失败：${t.message}")
        } finally {
            conn?.disconnect()
        }
    }

    /** 失败时删掉 SAF 建出的空壳：半截的 `.tachibk` 看着像一份真备份。 */
    private fun fail(target: Uri, message: String) {
        runCatching { DocumentsContract.deleteDocument(activity.contentResolver, target) }
        toast(message)
    }

    private fun toast(message: String) {
        activity.runOnUiThread { Toast.makeText(activity, message, Toast.LENGTH_LONG).show() }
    }

    /** 文件名优先取 `Content-Disposition`（服务端给的是 `org.suwayomi.next_<时间>.tachibk`）。 */
    private fun filenameOf(contentDisposition: String?, url: String): String {
        val fromHeader = contentDisposition?.let { FILENAME.find(it)?.groupValues?.get(1) }
        return fromHeader?.trim()?.takeIf { it.isNotEmpty() }
            ?: url.substringAfterLast('/').substringBefore('?').takeIf { it.isNotEmpty() }
            ?: FALLBACK_NAME
    }

    companion object {
        private const val TAG = "Suwayomi"

        /** 与 [FileChooser] 的 `0x5201` 同属 `0x52xx` 段，各占一个码。 */
        private const val REQUEST_CREATE_FILE = 0x5202

        /** 备份要现打包，比一般下载慢，给宽一点。 */
        private const val TIMEOUT_MS = 120_000

        private const val FALLBACK_MIME = "application/octet-stream"
        private const val FALLBACK_NAME = "suwayomi-backup.tachibk"

        /** `filename="x"` / `filename*=UTF-8''x` 都吃；服务端只发前者。 */
        private val FILENAME = Regex("""filename\*?=(?:UTF-8'')?"?([^";]+)""")
    }
}
