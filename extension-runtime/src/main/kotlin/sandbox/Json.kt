//! 手拼 JSON 的转义工具（两端共用契约里的所有字符串都经它输出）。
//!
//! 之所以手拼而不引序列化库：这些 payload 是给 Rust 侧 `HttpSandboxFetcher`
//! 的固定契约，字段名必须逐个可控，不能随某个库的版本改变大小写策略。

package sandbox

fun jsonStr(s: String): String = "\"" + s
    .replace("\\", "\\\\")
    .replace("\"", "\\\"")
    .replace("\n", "\\n")
    .replace("\r", "\\r")
    .replace("\t", "\\t") + "\""

fun jsonOpt(s: String?): String = if (s == null) "null" else jsonStr(s)
