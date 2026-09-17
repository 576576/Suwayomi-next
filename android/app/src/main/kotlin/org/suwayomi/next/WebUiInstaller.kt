//! 把打包进 assets 的 `webui.zip` 装到 `filesDir/webui`。
//!
//! **version.txt 是唯一判据**：桌面端 server 也只读 WebUI 根目录的 version.txt
//! 决定「本地 WebUI 是哪个版本」（见 MEMORY「version.txt / about* 字段」）。
//! 所以这里同样先只读 zip 里的 version.txt 做比较，一致就整体跳过解压 ——
//! 每次启动都重解 40MB 既慢又白白磨损闪存。

package org.suwayomi.next

import android.content.Context
import android.util.Log
import java.io.File
import java.util.zip.ZipInputStream

internal object WebUiInstaller {
    private const val TAG = "Suwayomi"
    private const val ASSET = "webui.zip"
    private const val VERSION_ENTRY = "version.txt"
    private const val DIR = "webui"

    /** 确保 `filesDir/webui` 与 assets 内的一致，返回该目录。 */
    fun ensure(context: Context): File {
        val target = File(context.filesDir, DIR)
        return try {
            val assetVersion = readAssetEntry(context, VERSION_ENTRY)
            // zip 里的 version.txt 结尾带换行，直接比会永远不相等（表现为每次启动
            // 都重解一遍 WebUI），所以两边都 trim
            val currentVersion = File(target, VERSION_ENTRY)
                .takeIf { it.isFile }
                ?.readText()
                ?.trim()
            if (assetVersion != null && assetVersion == currentVersion) {
                target
            } else {
                extract(context, target)
                target
            }
        } catch (t: Throwable) {
            Log.e(TAG, "cannot install bundled WebUI: $t")
            target
        }
    }

    /** 流式解压，不把整个 zip 读进内存。 */
    private fun extract(context: Context, target: File) {
        target.deleteRecursively()
        target.mkdirs()
        context.assets.open(ASSET).use { raw ->
            ZipInputStream(raw).use { zip ->
                var entry = zip.nextEntry
                while (entry != null) {
                    // 防 zip slip：解析后的路径必须仍在 target 内
                    val out = File(target, entry.name)
                    if (!out.canonicalPath.startsWith(target.canonicalPath + File.separator)) {
                        Log.w(TAG, "skipping suspicious zip entry ${entry.name}")
                    } else if (entry.isDirectory) {
                        out.mkdirs()
                    } else {
                        out.parentFile?.mkdirs()
                        out.outputStream().use { zip.copyTo(it) }
                    }
                    zip.closeEntry()
                    entry = zip.nextEntry
                }
            }
        }
        Log.i(TAG, "bundled WebUI extracted to ${target.absolutePath}")
    }

    /** 只取 zip 中第一个名为 [name] 的条目内容（不解压整个包）。 */
    private fun readAssetEntry(context: Context, name: String): String? =
        context.assets.open(ASSET).use { raw ->
            ZipInputStream(raw).use { zip ->
                var entry = zip.nextEntry
                while (entry != null) {
                    if (!entry.isDirectory && entry.name == name) {
                        return@use zip.readBytes().toString(Charsets.UTF_8).trim()
                    }
                    zip.closeEntry()
                    entry = zip.nextEntry
                }
                null
            }
        }
}
