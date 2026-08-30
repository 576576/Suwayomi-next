//! HttpExchange helpers.
package sandbox

import com.sun.net.httpserver.HttpExchange
import java.nio.charset.StandardCharsets

fun HttpExchange.respond(code: Int, body: String) {
    val bytes = body.toByteArray(StandardCharsets.UTF_8)
    this.responseHeaders.set("Content-Type", "application/json; charset=utf-8")
    this.sendResponseHeaders(code, bytes.size.toLong())
    this.responseBody.use { it.write(bytes) }
}

fun HttpExchange.respond404() = respond(404, """{"error":"not found"}""")
