//! 平台无关的极简 HTTP 类型。
//!
//! 桌面端由 `com.sun.net.httpserver` 适配，Android 端由自带的 `ServerSocket`
//! 适配（Android 的 bootclasspath 里**没有** `com.sun.net.httpserver`）。
//! Router 只依赖这里的类型，因此两端共享同一份路由与契约实现。

package sandbox

/** 一次请求：方法与路径已拆好，body 已读全。 */
class HttpRequest(
    val method: String,
    /** rawPath，如 `/source/1/chapter/abc%2Fdef/pages`（未解码） */
    val rawPath: String,
    /** 未解码的原始 query（不含前导 `?`）；无 query 时为空串 */
    val rawQuery: String = "",
    val body: ByteArray = ByteArray(0),
)

/** 一次响应：状态码 + 文本体。 */
class HttpResponse(
    val status: Int,
    val body: String,
    val contentType: String = "application/json; charset=utf-8",
)

/** 处理一个请求；实现方不需要关心底层是哪种 HTTP 服务器。 */
fun interface HttpHandler {
    fun handle(req: HttpRequest): HttpResponse
}
