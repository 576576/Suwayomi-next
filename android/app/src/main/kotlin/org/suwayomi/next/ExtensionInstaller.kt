//! 扩展的安装/卸载 —— 全部交给**系统安装器/卸载器**，本项目不写任何扩展文件。
//!
//! 与桌面的根本差别（docs/migration/ANDROID_IMPL.md D4）：
//!  * 桌面：server 下载 APK 写进 `extensions/`，重启沙盒；
//!  * Android：把 APK 落到 `cacheDir` 再用 FileProvider 交给系统安装器，
//!    用户在系统界面确认后由 `PackageManager` 安装。卸载同理走 `ACTION_DELETE`。
//!
//! 这么做的收益是「扩展的所有权在系统」：Mihon/Komikku 之类装了或卸了扩展，
//! 本 App 下一次 `/reload` 立刻能看到，不需要也不存在一份属于自己的副本。

package org.suwayomi.next

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.util.Log
import androidx.core.content.FileProvider
import java.io.File

object ExtensionInstaller {
    private const val TAG = "Suwayomi"

    /** `cacheDir` 下存放待安装 APK 副本的子目录（同时也是 FileProvider 暴露的范围）。 */
    private const val IMPORT_DIR = "apk-import"

    /**
     * 把 [apk] 交给系统安装器。
     *
     * @return true 表示已成功唤起（用户还要在系统界面点确认）；
     *         false 表示缺少「安装未知来源」授权或没有安装器。
     */
    fun install(context: Context, apk: File): Boolean {
        if (!apk.isFile || apk.length() == 0L) {
            Log.w(TAG, "install: $apk is missing or empty")
            return false
        }
        if (!canRequestInstall(context)) {
            Log.w(TAG, "install: REQUEST_INSTALL_PACKAGES not granted; asking the user to allow it")
            openUnknownSourcesSettings(context)
            return false
        }
        val uri: Uri = FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", apk)
        val intent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "application/vnd.android.package-archive")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        // 本 App 自己也注册了 APK 的 VIEW filter（作为入口），不排除掉的话系统会弹
        // 「打开方式」选择器，用户再点回 Suwayomi 就原地打转。显式挑一个不是自己的
        // 处理者 —— `AndroidManifest` 里 `<queries>` 的 VIEW+apk intent 就是为此
        // 让这次查询可见（Android 11+ 包可见性）。
        val installer = context.packageManager
            .queryIntentActivities(intent, 0)
            .firstOrNull { it.activityInfo.packageName != context.packageName }
        if (installer != null) {
            intent.setClassName(installer.activityInfo.packageName, installer.activityInfo.name)
        }
        return try {
            context.startActivity(intent)
            true
        } catch (e: ActivityNotFoundException) {
            Log.w(TAG, "install: no package installer available", e)
            false
        }
    }

    /** 唤起系统卸载器。 */
    fun uninstall(context: Context, pkgName: String): Boolean {
        val intent = Intent(Intent.ACTION_DELETE).apply {
            data = Uri.parse("package:$pkgName")
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        return try {
            context.startActivity(intent)
            true
        } catch (e: ActivityNotFoundException) {
            Log.w(TAG, "uninstall: no uninstaller available", e)
            false
        }
    }

    /** 本 App 是否被允许安装未知来源应用（API 26+ 需要用户显式授权）。 */
    fun canRequestInstall(context: Context): Boolean =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.packageManager.canRequestPackageInstalls()
        } else {
            true
        }

    /** 跳到「安装未知应用」授权页。 */
    fun openUnknownSourcesSettings(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val intent = Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES)
            .setData(Uri.parse("package:${context.packageName}"))
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try {
            context.startActivity(intent)
        } catch (e: ActivityNotFoundException) {
            Log.w(TAG, "cannot open unknown-sources settings", e)
        }
    }

    /**
     * 从 `ACTION_VIEW` / `ACTION_SEND` 拿到待安装的 APK，落一份到 `cacheDir`。
     *
     * 源一定是 `content://`（DocumentsUI、下载管理器、FileProvider 给的都是），
     * 所以统一走 `contentResolver`。
     *
     * 目标放进独立子目录并加时间戳前缀，**不能直接用原名**：源文件有时就落在
     * `cacheDir` 里（例如上一次导入的副本），同名会让 `outputStream()` 先把源
     * 截断成 0 字节，复制出来的是空文件，而且是静默失败。
     */
    fun apkFromViewIntent(activity: Activity, data: Uri?): File? {
        data ?: return null
        val dir = File(activity.cacheDir, IMPORT_DIR).apply { mkdirs() }
        // 顺手丢掉过期的导入副本（cacheDir 系统也会清，这里只保证自己不太脏）
        pruneOldImports(dir)
        val base = data.lastPathSegment?.substringAfterLast('/')?.ifEmpty { null } ?: "extension.apk"
        val out = File(dir, "${System.currentTimeMillis()}-$base")
        return try {
            activity.contentResolver.openInputStream(data)?.use { input ->
                out.outputStream().use { input.copyTo(it) }
            }
            if (out.length() > 0) {
                out
            } else {
                Log.w(TAG, "imported $data is empty (source missing?)")
                out.delete()
                null
            }
        } catch (t: Throwable) {
            Log.w(TAG, "cannot copy $data", t)
            out.delete()
            null
        }
    }

    /** 删掉 [dir] 里超过一天的导入副本。 */
    private fun pruneOldImports(dir: File) {
        val cutoff = System.currentTimeMillis() - 24 * 60 * 60 * 1000
        dir.listFiles()?.forEach { f ->
            if (f.isFile && f.lastModified() < cutoff) f.delete()
        }
    }
}
