//! 「编辑存储位置」在 Android 上走系统授权，不能靠手填路径。
//!
//! 两件事缺一不可：
//!
//!  1. **`ACTION_OPEN_DOCUMENT_TREE`** —— 用户挑目录，拿到 SAF tree URI，再映射回
//!     真实文件系统路径（server 只认路径，不认 URI）。
//!  2. **文件系统写权限** —— API 30+ 是「所有文件访问」（MANAGE_EXTERNAL_STORAGE），
//!     以下走 READ/WRITE_EXTERNAL_STORAGE。写盘的是**同进程里的 Rust 库**，用的是
//!     普通 POSIX 调用，SAF 的「按 URI 授权」对它无效：少了这一步，目录挑得动，
//!     一落盘就失败。
//!
//! 顺序是先要权限、再挑目录。

package org.suwayomi.next

import android.Manifest
import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.storage.StorageManager
import android.provider.DocumentsContract
import android.provider.Settings
import android.util.Log
import java.io.File

/** 一次目录选择的结局。 */
sealed interface PickResult {
    /** 用户选定的**真实路径**（已确认可写）。 */
    data class Picked(val path: String) : PickResult

    /** 用户按了返回/取消（或上一次请求还没回话就被新请求顶掉）。 */
    data object Cancelled : PickResult

    /** 拿不到权限、映射不出路径、目录写不进去…… 原因直接给用户看。 */
    data class Failed(val message: String) : PickResult
}

/**
 * 目录选择器的活动状态机。
 *
 * 状态跟着 Activity 走：一次选择要跨两次 Activity 跳转（先系统设置页要权限，再
 * DocumentsUI 挑目录）。本 App 只有一个 Activity（`singleTask`）。
 */
class DirectoryPicker(
    private val activity: Activity,
    /** 回话给 JS：`requestId` + 结局。 */
    private val reply: (String, PickResult) -> Unit,
) {
    private class Pending(val requestId: String, val initial: String)

    private var pending: Pending? = null

    /** SAF 是否已经拉起过 —— 权限回来与 `onResume` 都可能触发继续，只能挑一次。 */
    private var pickerLaunched = false

    /** JS 侧发起的请求。 */
    fun request(requestId: String, initial: String) {
        // 上一次没回话的（连点两次铅笔）先当取消，别让 JS 侧永远挂着
        pending?.let { reply(it.requestId, PickResult.Cancelled) }
        pending = Pending(requestId, initial)
        pickerLaunched = false

        when {
            hasWriteAccess() -> launchPicker()
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.R -> openAllFilesAccessSettings()
            else -> requestLegacyStorage()
        }
    }

    /**
     * `Activity.onResume` 里调用。
     *
     * 「所有文件访问」设置页不一定把结果回给 `onActivityResult`（各家 ROM 行为
     * 不一），所以每次回到前台补一次判断。
     */
    fun resume() {
        if (pending == null || pickerLaunched) return
        if (hasWriteAccess()) launchPicker()
    }

    /** `Activity.onActivityResult` 里调用；返回 true 表示这次结果由本类消费了。 */
    fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?): Boolean = when (requestCode) {
        REQUEST_PICK_DIR -> {
            onPicked(resultCode, data)
            true
        }
        REQUEST_ALL_FILES_ACCESS, REQUEST_LEGACY_STORAGE -> {
            // 用户回来了：有权限就继续挑目录，没有就如实回话（不装作成功）
            if (hasWriteAccess()) {
                launchPicker()
            } else {
                finish(PickResult.Failed("没有文件访问权限，无法把数据写到自选目录"))
            }
            true
        }
        else -> false
    }

    /** `Activity.onRequestPermissionsResult` 里调用。 */
    fun onRequestPermissionsResult(requestCode: Int, grantResults: IntArray): Boolean {
        if (requestCode != REQUEST_LEGACY_STORAGE) return false
        if (grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) {
            launchPicker()
        } else {
            finish(PickResult.Failed("没有存储权限，无法把数据写到自选目录"))
        }
        return true
    }

    /** Activity 销毁时调用：别把结果投给一个已经没了的 WebView。 */
    fun detach() {
        pending = null
    }

    // ---- 权限 -------------------------------------------------------------

    private fun hasWriteAccess(): Boolean =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            Environment.isExternalStorageManager()
        } else {
            activity.checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) ==
                PackageManager.PERMISSION_GRANTED
        }

    /**
     * 「所有文件访问」授权页。
     *
     * 先试带包名的那种（直接跳到本 App 的开关），ROM 上不一定有，拿不到就退到
     * 全局列表页。
     */
    private fun openAllFilesAccessSettings() {
        val pkgUri = Uri.parse("package:${activity.packageName}")
        val intents = listOf(
            Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, pkgUri),
            Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION, pkgUri),
            Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION),
        )
        for (intent in intents) {
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            try {
                activity.startActivityForResult(intent, REQUEST_ALL_FILES_ACCESS)
                return
            } catch (_: ActivityNotFoundException) {
                // 换下一个
            }
        }
        finish(PickResult.Failed("系统里找不到「所有文件访问」授权页"))
    }

    @Suppress("DEPRECATION")
    private fun requestLegacyStorage() {
        activity.requestPermissions(
            arrayOf(Manifest.permission.WRITE_EXTERNAL_STORAGE),
            REQUEST_LEGACY_STORAGE,
        )
    }

    // ---- 挑目录 -----------------------------------------------------------

    private fun launchPicker() {
        val req = pending ?: return
        if (pickerLaunched) return
        pickerLaunched = true

        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            addFlags(Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
            // 从当前目录开始（API 26+）。DocumentsUI 只认 doc URI，file:// 多半被
            // 忽略 —— 忽略了只是"从默认位置开始"，不影响选择结果。
            if (req.initial.isNotEmpty()) {
                runCatching { putExtra(DocumentsContract.EXTRA_INITIAL_URI, Uri.fromFile(File(req.initial))) }
            }
        }
        try {
            activity.startActivityForResult(intent, REQUEST_PICK_DIR)
        } catch (e: ActivityNotFoundException) {
            finish(PickResult.Failed("系统里没有目录选择器（DocumentsUI 被裁剪过？）：$e"))
        }
    }

    private fun onPicked(resultCode: Int, data: Intent?) {
        val uri = data?.data
        if (resultCode != Activity.RESULT_OK || uri == null) {
            finish(PickResult.Cancelled)
            return
        }
        // 把授权记下来（跨重启有效）。真正写盘靠的是「所有文件访问」，留着它是让
        // 后续以该目录为起点的交互不至于因为没授权而失败。
        runCatching {
            activity.contentResolver.takePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
            )
        }.onFailure { Log.w(TAG, "cannot take persistable permission on $uri: $it") }

        val path = treeUriToPath(activity, uri)
        if (path == null) {
            finish(PickResult.Failed("${uri.lastPathSegment ?: uri} 不是本机存储上的目录，请换一个位置"))
            return
        }
        val dir = File(path)
        // 目录可能还不存在（用户新建的），先建出来；建不出来/不可写就别假装成功，
        // 否则会在下载/备份时才失败，用户对不上因果。
        if (!dir.isDirectory && !dir.mkdirs()) {
            finish(PickResult.Failed("无法创建目录 $path"))
            return
        }
        if (!dir.canWrite()) {
            finish(PickResult.Failed("$path 不可写（拿到「所有文件访问」权限了吗？）"))
            return
        }
        finish(PickResult.Picked(dir.absolutePath))
    }

    private fun finish(result: PickResult) {
        val req = pending ?: return
        pending = null
        pickerLaunched = false
        reply(req.requestId, result)
    }

    companion object {
        private const val TAG = "Suwayomi"

        private const val REQUEST_PICK_DIR = 0x5101
        private const val REQUEST_ALL_FILES_ACCESS = 0x5102
        private const val REQUEST_LEGACY_STORAGE = 0x5103

        /**
         * SAF 的 tree URI → 真实路径。
         *
         * `com.android.externalstorage.documents` 的 document id 可解析：
         * `content://com.android.externalstorage.documents/tree/primary%3ADownload%2FSuwayomi`
         * 的 id 是 `primary:Download/Suwayomi` —— 冒号前是**卷标识**（内置存储是
         * `primary`，外置卡是 uuid），冒号后是卷内相对路径。卷标识 → 挂载点从
         * `StorageManager.storageVolumes` 查，不自己拼 `/storage/emulated/0`。
         *
         * **只对这个 provider 成立**：网盘类 provider 的 doc id 与文件路径无关，
         * 只能回 null，由调用方提示用户换个位置。
         */
        fun treeUriToPath(context: Context, uri: Uri): String? {
            if (uri.authority != EXTERNAL_STORAGE_AUTHORITY) return null
            val docId = runCatching { DocumentsContract.getTreeDocumentId(uri) }.getOrNull() ?: return null
            val volumeId = docId.substringBefore(':', "")
            if (volumeId.isEmpty()) return null
            val relative = docId.substringAfter(':', "")

            val root = volumeRoot(context, volumeId) ?: return null
            val dir = if (relative.isEmpty()) root else File(root, relative)
            return dir.absolutePath
        }

        private fun volumeRoot(context: Context, volumeId: String): File? {
            val sm = context.getSystemService(Context.STORAGE_SERVICE) as? StorageManager ?: return null
            for (volume in sm.storageVolumes) {
                val matches = if (volumeId.equals(PRIMARY_VOLUME, ignoreCase = true)) {
                    volume.isPrimary
                } else {
                    volume.uuid.equals(volumeId, ignoreCase = true)
                }
                if (matches) volume.directory?.let { return it }
            }
            return null
        }

        private const val EXTERNAL_STORAGE_AUTHORITY = "com.android.externalstorage.documents"
        private const val PRIMARY_VOLUME = "primary"
    }
}
