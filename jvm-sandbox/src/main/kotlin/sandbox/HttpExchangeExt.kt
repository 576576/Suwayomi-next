//! HttpExchange ↔ 平台无关 HttpRequest/HttpResponse 的适配。
//!
//! 桌面走 `com.sun.net.httpserver`（keep-alive、分块传输都由它处理）；
//! Android 没有这个类，那边的适配器见 `android/extension-host/SimpleHttpServer.kt`。
//! Router 只认共享侧的 HttpRequest/HttpResponse，所以两端共用同一份路由代码。

package sandbox

import com.sun.net.httpserver.HttpExchange
import java.nio.charset.StandardCharsets

/** 读全请求体后交给共享 Router，再把结果写回。 */
fun HttpExchange.dispatch(router: Router) {
    val body = requestBody.use { it.readBytes() }
    val response = router.handle(
        HttpRequest(
            method = requestMethod,
            // rawPath 不含 query；rawQuery 已是纯 query 串（不含 `?`）
            rawPath = requestURI.rawPath ?: "/",
            rawQuery = requestURI.rawQuery ?: "",
            body = body,
        ),
    )
    respond(response.status, response.body)
}

fun HttpExchange.respond(code: Int, body: String) {
    val bytes = body.toByteArray(StandardCharsets.UTF_8)
    this.responseHeaders.set("Content-Type", "application/json; charset=utf-8")
    this.sendResponseHeaders(code, bytes.size.toLong())
    this.responseBody.use { it.write(bytes) }
}
