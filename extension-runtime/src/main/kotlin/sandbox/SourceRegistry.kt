//! Router 依赖的最小扩展注册面。
//!
//! 两端各有一份实现，差异**只在扩展怎么被发现、怎么被加载**：
//!  - 桌面 `ExtensionRegistry`：扫 `extensions/` 目录 → dex2jar → ASM 修字节码 → 自建 ClassLoader
//!  - Android `PackageManagerRegistry`：`PackageManager` 查已安装包 → `PathClassLoader` 装进 ART
//! 路由、JSON 契约、字段名映射全部在共享侧，两端输出必须逐字节一致。

package sandbox

/**
 * 一个已加载扩展的元信息（桌面来自 APK 解析，Android 来自 PackageManager）。
 *
 * `versionCode` / `contentWarning` 存在的唯一理由是**让 server 侧能替扩展建
 * `extension` 表行**：桌面上这张表是仓库索引的镜像，Android 上没有索引
 * （扩展来自系统 PackageManager），只能靠这里报上去的元信息补建。
 */
data class ExtensionInfo(
    val pkgName: String,
    val name: String,
    val lang: String,
    val versionName: String,
    val className: String,
    val extensionId: Long,
    /** 仓库排序/升级判定用；解析不出来时为 0。 */
    val versionCode: Long = 0,
    /** 0 = Safe，1 = NSFW（APK manifest 的 `tachiyomi.extension.nsfw`）。 */
    val contentWarning: Int = 0,
)

interface SourceRegistry {
    val extensionCount: Int
    val sourceCount: Int

    fun toExtensionsJson(): String

    fun toSourcesJson(): String

    fun driver(sourceId: Long): SourceDriver?

    /** 重新发现扩展（桌面=重扫目录，Android=重查 PackageManager）。 */
    fun reload()

    /**
     * 解析一个**尚未安装**的 APK 的元信息（`POST /inspect`）。
     * 端上不支持该操作时返回 null —— Android 的安装走系统安装器，不经这条路径。
     */
    fun inspect(apkBytes: ByteArray): ExtensionInfo? = null
}
