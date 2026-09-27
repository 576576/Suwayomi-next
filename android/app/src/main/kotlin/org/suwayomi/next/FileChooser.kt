//! WebView 里 `<input type="file">` 的宿主侧实现 —— 「恢复备份」要靠它。
//!
//! WebUI 的「恢复备份」是点一个**隐藏的** `<input type="file">`
//! （`src/features/backup/screens/Backup.tsx` 里 `inputRef.current?.click()`）。
//! 在浏览器里这一步由浏览器自己弹选择器；在 WebView 里**不会** —— WebView 把「弹
//! 文件选择器」外包给宿主，只有 `WebChromeClient` 实现了 `onShowFileChooser` 才有
//! 反应。缺了它，点击是**静默无响应**：不报错、不回调、logcat 里也没有任何异常，
//! 表现就是按钮按下去什么都没发生。
//!
//! 与 [DirectoryPicker] 的分工：「编辑存储位置」选的是**目录**，要 SAF 的 tree URI
//! 再映射回真实路径，还得先拿到全盘写权限（写盘的是同进程的 Rust 库），因此是跨两次
//! Activity 跳转的状态机；这里选的是**文件**，选完把 content URI 交回 WebView 即可 ——
//! 真正的读盘由 WebView 自己做，不需要任何存储权限，也不做（也不该做）路径映射。
//!
//! 不叫 `FilePicker` 是为了与 [DirectoryPicker] 一眼区分开。

package org.suwayomi.next

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.util.Log
import android.webkit.ValueCallback
import android.webkit.WebChromeClient

/**
 * 一次文件选择的待办。
 *
 * 结果必须**恰好回话一次**：`ValueCallback` 没被回调时，WebView 会把这次请求永久
 * 挂起 —— 之后再点同一个 `<input type="file">` 都不会有任何反应，页面刷新前救不
 * 回来，而且同样没有任何日志。所以下面每个失败分支都必须走到 [finish]。
 */
class FileChooser(private val activity: Activity) {
    /** 对应 [WebChromeClient.onShowFileChooser] 的 `filePathCallback` 入参。 */
    private var callback: ValueCallback<Array<Uri>>? = null

    /**
     * 发起选择。返回 `true` 表示本类接管了这次请求 —— 必须回 `true`，否则 WebView
     * 按「宿主不管」处理，选择器同样不会弹出。
     */
    fun show(callback: ValueCallback<Array<Uri>>, params: WebChromeClient.FileChooserParams): Boolean {
        // 上一次还没回话的先当取消结掉（用户连点两次、或上一次的 Activity 被回收），
        // 别把它挂在那儿等一个永不到来的回调。
        finish(null)
        this.callback = callback

        // 用平台给的 createIntent()，不要自己拼 ACTION_GET_CONTENT：它已经按
        // `<input accept=… multiple>` 补上了 CATEGORY_OPENABLE（少了它，某些 provider
        // 会给出宿主打不开的 URI）与 EXTRA_MIME_TYPES。
        //
        // 也**不要**为了「只让选备份文件」而在这里加 MIME 过滤：`.tachibk` 没有注册过
        // MIME 类型，一过滤反而会把用户要选的那个文件藏起来。WebUI 那边的 input 本来
        // 就没写 accept，全量放开才是对的。
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
        // parseResult 把「确定 / 取消 / data 里是 clipData 而非 data」几种情况归一成
        // URI 数组，取消时给 null —— 正是 onReceiveValue 想要的形状。
        //
        // 若这次跳转期间 Activity 被重建过（切深浅色会走 uiMode 重建，见
        // AndroidManifest 的 configChanges 注释），新实例的 callback 是空的，
        // [finish] 会静默丢弃 —— 用户重点一次即可。
        finish(WebChromeClient.FileChooserParams.parseResult(resultCode, data))
        return true
    }

    /**
     * Activity 销毁时调用。
     *
     * 这里**故意不回话**：WebView 紧接着就被 `destroy()` 了，回调过去只是往一个死
     * 对象里写。丢掉引用即可 —— 与 [DirectoryPicker.detach] 同一个道理。
     */
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

        /** 与 [DirectoryPicker] 的 `0x51xx` 分开一段，将来两边各加请求码也不会撞。 */
        private const val REQUEST_PICK_FILE = 0x5201
    }
}
