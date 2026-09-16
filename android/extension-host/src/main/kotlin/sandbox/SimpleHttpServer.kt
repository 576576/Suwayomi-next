//! 平台无关的极简 HTTP 服务器（Android 侧）。
//!
//! Android 的 bootclasspath 里**没有** `com.sun.net.httpserver`（那是 JDK 的
//! jdk.httpserver 模块），所以桌面那套宿主用不了。这里用 `ServerSocket` 写一个
//! 只够本项目用的 HTTP/1.1 服务端：
//!  * 支持 GET / POST，读 `Content-Length` 定长的 body（`/inspect` 会用到）；
//!  * 支持 keep-alive（Rust 侧 reqwest 的连接池默认复用连接）；
//!  * 只监听 127.0.0.1 —— 这是一个**同进程回环**通道，不对外暴露。
//!
//! 契约与桌面完全一致（同一个 `sandbox.Router`），因此 Rust 侧的
//! `HttpSandboxFetcher` 不需要知道对端是 JVM 还是 ART。

package sandbox

import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.io.EOFException
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketException
import java.nio.charset.StandardCharsets
import java.util.concurrent.Executors

/**
 * 极简单线程 accept + 线程池处理的 HTTP 服务端。
 *
 * @param port 监听端口；传 0 让系统分配，之后用 [boundPort] 读取真实端口。
 * @param handler 处理函数（通常是 `Router::handle`）。
 */
class SimpleHttpServer(
    port: Int,
    private val handler: HttpHandler,
    host: String = LOOPBACK,
) {
    private val server = ServerSocket(port, 64, InetAddress.getByName(host))
    private val pool = Executors.newCachedThreadPool { r ->
        Thread(r, "suwayomi-http").apply { isDaemon = true }
    }

    /** 实际监听的端口（传入 0 时由系统分配）。 */
    val boundPort: Int get() = server.localPort

    fun start() {
        pool.execute {
            while (!server.isClosed) {
                val socket = try {
                    server.accept()
                } catch (e: SocketException) {
                    break // server 已关闭
                }
                pool.execute { serve(socket) }
            }
        }
    }

    fun stop() {
        runCatching { server.close() }
        pool.shutdownNow()
    }

    private fun serve(socket: Socket) {
        socket.use { s ->
            s.tcpNoDelay = true
            val input = BufferedInputStream(s.getInputStream())
            val output = BufferedOutputStream(s.getOutputStream())
            try {
                // keep-alive：一条连接上连续处理多个请求，直到对端关闭或要求 close
                while (true) {
                    val head = readHead(input) ?: return
                    val req = parseRequest(head, input) ?: return
                    val res = try {
                        handler.handle(req)
                    } catch (t: Throwable) {
                        HttpResponse(500, """{"error":${jsonStr(t.stackTraceToString())}}""")
                    }
                    val close = wantsClose(head)
                    writeResponse(output, res, close)
                    if (close) return
                }
            } catch (e: EOFException) {
                // 对端提前关闭，正常
            } catch (e: SocketException) {
                // 连接被重置，正常
            } catch (t: Throwable) {
                System.err.println("suwayomi-http: $t")
            }
        }
    }

    /** 读到空行结束的请求头；对端关闭时返回 null。 */
    private fun readHead(input: BufferedInputStream): List<String>? {
        val lines = ArrayList<String>(16)
        val sb = StringBuilder()
        while (true) {
            val b = input.read()
            if (b == -1) return if (lines.isEmpty() && sb.isEmpty()) null else lines
            when (b) {
                '\r'.code -> {}
                '\n'.code -> {
                    if (sb.isEmpty()) return lines
                    lines.add(sb.toString())
                    sb.setLength(0)
                }
                else -> sb.append(b.toChar())
            }
        }
    }

    private fun parseRequest(head: List<String>, input: BufferedInputStream): HttpRequest? {
        if (head.isEmpty()) return null
        val parts = head[0].split(' ')
        if (parts.size < 2) return null
        val method = parts[0]
        val target = parts[1]
        val q = target.indexOf('?')
        val rawPath = if (q >= 0) target.substring(0, q) else target
        val rawQuery = if (q >= 0) target.substring(q + 1) else ""

        val contentLength = head.drop(1)
            .firstOrNull { it.startsWith("Content-Length:", ignoreCase = true) }
            ?.substringAfter(':')?.trim()?.toIntOrNull() ?: 0
        val body = if (contentLength > 0) {
            val buf = ByteArray(contentLength)
            var read = 0
            while (read < contentLength) {
                val n = input.read(buf, read, contentLength - read)
                if (n < 0) break
                read += n
            }
            buf
        } else {
            ByteArray(0)
        }
        return HttpRequest(method, rawPath, rawQuery, body)
    }

    private fun wantsClose(head: List<String>): Boolean {
        val conn = head.drop(1)
            .firstOrNull { it.startsWith("Connection:", ignoreCase = true) }
            ?.substringAfter(':')?.trim()?.lowercase()
        // HTTP/1.1 默认 keep-alive；只有显式 close 才断开
        return conn != null && conn.contains("close")
    }

    private fun writeResponse(out: BufferedOutputStream, res: HttpResponse, close: Boolean) {
        val body = res.body.toByteArray(StandardCharsets.UTF_8)
        val head = buildString {
            append("HTTP/1.1 ").append(res.status).append(' ').append(reason(res.status)).append("\r\n")
            append("Content-Type: ").append(res.contentType).append("\r\n")
            append("Content-Length: ").append(body.size).append("\r\n")
            append("Connection: ").append(if (close) "close" else "keep-alive").append("\r\n")
            append("\r\n")
        }
        out.write(head.toByteArray(StandardCharsets.US_ASCII))
        out.write(body)
        out.flush()
    }

    private fun reason(status: Int): String = when (status) {
        200 -> "OK"
        400 -> "Bad Request"
        404 -> "Not Found"
        500 -> "Internal Server Error"
        else -> "Status"
    }

    private companion object {
        /** 只绑回环：这是同进程通道，绝不能对外暴露 server 的扩展接口。 */
        const val LOOPBACK = "127.0.0.1"
    }
}
