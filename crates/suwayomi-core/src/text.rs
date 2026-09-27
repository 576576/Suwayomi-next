//! 无状态的文本工具 —— 纯函数，无全局状态、无锁、无 I/O。
//!
//! 这里放"被多个 crate 反复重写过的"小算法。此前 `urlencode` 在
//! `domain`（sandbox / kitsu / shikimori / myanimelist）、`opds`、`rest`
//! 里各写了一份完全等价的命令式循环，共 6 份；统一到这里之后，
//! 修 bug 只需要改一个地方。

use std::fmt::Write as _;

/// 按 RFC 3986 的 `unreserved` 集合做百分号转义。
///
/// 只保留 `A-Z a-z 0-9 - _ . ~`，其余字节一律转成 `%XX`（大写十六进制）；
/// 非 ASCII 字符按 UTF-8 逐字节转义，正好符合 URI 规范。
///
/// 这是**表达式**而不是"建缓冲 + 循环 push"：`fold` 把每个字节映射成
/// 一段输出，累积器只在这个表达式内部存在，调用方看不到可变状态。
pub fn urlencode(s: &str) -> String {
    s.bytes().fold(String::with_capacity(s.len()), |mut out, b| {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(char::from(b)),
            _ => {
                // `write!` 直接追加到已有缓冲，避免 `%XX` 每次都分配一个临时 String。
                let _ = write!(out, "%{b:02X}");
            }
        }
        out
    })
}

#[cfg(test)]
mod tests {
    use super::urlencode;

    #[test]
    fn keeps_unreserved_bytes() {
        assert_eq!(urlencode("abcXYZ019-_.~"), "abcXYZ019-_.~");
    }

    #[test]
    fn escapes_reserved_and_space() {
        assert_eq!(urlencode("a b&c=d"), "a%20b%26c%3Dd");
    }

    #[test]
    fn escapes_utf8_bytewise() {
        // "中" 的 UTF-8 是 E4 B8 AD
        assert_eq!(urlencode("中"), "%E4%B8%AD");
    }

    #[test]
    fn empty_stays_empty() {
        assert_eq!(urlencode(""), "");
    }
}
