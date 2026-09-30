//! WebView 里 `<input type="file">` 的宿主侧实现 —— 「恢复备份」要靠它。
//! WebView 把「弹文件选择器」外包给宿主，只有实现了 `onShowFileChooser` 才有反应；
//! 缺了它点击是**静默无响应**。细节见 `docs/agent/android.md` §「选文件」。

package org.suwayomi.next

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.util.Log
import android.webkit.ValueCallback
import android.webkit.WebChromeClient

/**
 * 一次文件选择的待办。结果必须**恰好回话一次** —— `ValueCallback` 没被回调时，WebView
 * 会把这次请求永久挂起（之后再点同一个 input 也没反应，刷新页面前救不回来）。
 */
class FileChooser(private val activity: Activity) {
    /** 对应 [WebChromeClient.onShowFileChooser] 的 `filePathCallback` 入参。 */
    private var callback: ValueCallback<Array<Uri>>? = null

    /** 发起选择。必须返回 true，否则 WebView 按「宿主不管」处理，选择器同样不弹。 */
    fun show(callback: ValueCallback<Array<Uri>>, params: WebChromeClient.FileChooserParams): Boolean {
        // 上一次还没回话的先当取消结掉（用户连点两次、或上一次的 Activity 被回收），
        // 别把它挂在那儿等一个永不到来的回调。
        finish(null)
        this.callback = callback

        // 用平台给的 createIntent()，它已按 `<input accept=… multiple>` 补上
        // CATEGORY_OPENABLE 与 EXTRA_MIME_TYPES。**别加 MIME 过滤**：`.tachibk` 没有注册过
        // 类型，一过滤反而把用户要选的那个文件藏起来。
        val intent = try {
            params.createIntent()
        } catch (e: Exception) {
            Log.w(TAG, "cannot build a file chooser intent: $e")
            finish(null)
            return true
        }
        try {
            activity.startActivityForResult(intent, REQUEST_PICK_FILE)
        } catch (e: ActivityNotFoundException) {
            // 极简 ROM 里可能没有 DocumentsUI：如实回「取消」，但一定要回话
            Log.w(TAG, "no activity handles ${intent.action}: $e")
            finish(null)
        }
        return true
    }

    /** `Activity.onActivityResult` 里调用；返回 true 表示这次结果由本类消费了。 */
    fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        if (requestCode != REQUEST_PICK_FILE) return false
        // parseResult 把「确定 / 取消 / data 里是 clipData 而非 data」归一成 URI 数组，
        // 取消时给 null —— 正是 onReceiveValue 想要的形状。
        finish(WebChromeClient.FileChooserParams.parseResult(resultCode, data))
        return true
    }

    /** Activity 销毁时调用。**故意不回话**：WebView 紧接着就被 destroy 了，丢掉引用即可。 */
    fun detach() {
        callback = null
    }

    private fun finish(uris: Array<Uri>?) {
        val cb = callback ?: return
        callback = null
        cb.onReceiveValue(uris)
    }

    companion object {
        private const val TAG = "Suwayomi"

        /** 与 [DownloadSaver] 的 `0x5202` 同属 `0x52xx` 段，各占一个码。 */
        private const val REQUEST_PICK_FILE = 0x5201
    }
}
