//! 扩展抛出的异常 → 能直接给用户看的一句话。

package sandbox

/**
 * 取因果链最深一层异常的 `简单类名: message`。
 *
 * 扩展失败时真正的原因在最里层：外层一律是宿主包的那句
 * `getPopularManga failed: …` / `setupPreferenceScreen failed: …`（见 `Reflect.kt`），
 * 类名也只是 `RuntimeException`。把最里层挑出来，用户看到的才是
 * `IOException: 请在扩展设置界面输入用户名和密码` 这种能照着办的文案，
 * 而不是一句宿主内部方法名。
 *
 * Kagane 的 `InstantiationError: y0` 这类只有类名的也要给全，message 为空时退回类名。
 */
fun readableError(t: Throwable): String {
    var cur = t
    // 因果链可能有环（扩展自己 `initCause` 成环并不违法），限步避免死循环。
    var guard = 0
    while (cur.cause != null && cur.cause !== cur && guard++ < 16) {
        cur = cur.cause!!
    }
    val name = cur.javaClass.simpleName.ifEmpty { cur.javaClass.name }
    val text = cur.message?.trim()?.takeIf { it.isNotEmpty() }
        ?: t.message?.trim()?.takeIf { it.isNotEmpty() }
    return if (text == null) name else "$name: $text"
}

/**
 * 500 响应体的统一形状：`error` 给人看（一行），`stack` 给日志看。
 *
 * 两件事必须分开：`error` 会被服务端原样送到界面上，塞进完整 Java 栈就是几十行
 * `at …`；而排查问题又需要那份栈，所以留在 `stack` 里由服务端记日志。
 */
fun errorJson(t: Throwable): String =
    """{"error":${jsonStr(readableError(t))},"stack":${jsonStr(t.stackTraceToString())}}"""
