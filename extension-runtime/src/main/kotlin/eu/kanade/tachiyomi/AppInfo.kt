package eu.kanade.tachiyomi

/**
 * 宿主应用信息（extension-lib 1.3 / 1.5 契约）。
 *
 * 扩展把这些值拼进 User-Agent，所以**必须来自宿主自己的版本**而不能写死常量：
 * 桌面沙盒由 `main()` 从 `SUWAYOMI_VERSION_NAME` / `SUWAYOMI_VERSION_CODE` 环境变量
 * 注入（server 侧 spawn JVM 时从 `suwayomi_core::version` 传入，与 WebUI「关于」页
 * 同源），Android 由 `ExtensionHost.start` 用 PackageManager 读回本应用包信息。
 * 两端都没注入时保持 `0` / `"0"`，不会抛异常。
 */
object AppInfo {
    @Volatile
    private var versionCode: Int = 0

    @Volatile
    private var versionName: String = "0"

    /** 宿主启动、加载扩展之前调用一次。 */
    fun installVersion(versionCode: Int, versionName: String) {
        this.versionCode = versionCode
        this.versionName = versionName
    }

    /**
     * 宿主版本号（如 3064）。
     *
     * @since extension-lib 1.3
     */
    fun getVersionCode(): Int = versionCode

    /**
     * 宿主版本名（如 `r3064` 或 `2.0.0`）。
     *
     * @since extension-lib 1.3
     */
    fun getVersionName(): String = versionName

    /**
     * 阅读器能渲染的图片格式。
     *
     * 只列服务器会给出正确 Content-Type 的那几种（server 的 `image_content_type`）：
     * 图片是原样透传给阅读器的，声明 JXL / HEIF 之类却无人能解码，扩展就会按这份
     * 清单挑到渲染不出来的地址。
     *
     * @since extension-lib 1.5
     */
    fun getSupportedImageMimeTypes(): List<String> = SUPPORTED_IMAGE_MIME_TYPES

    private val SUPPORTED_IMAGE_MIME_TYPES =
        listOf("image/jpeg", "image/png", "image/gif", "image/webp", "image/avif")
}
