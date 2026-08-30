//! SQL placeholder normalization (PostgreSQL only).
//!
//! Queries are written with SQLite-style `?` placeholders and re-bound for
//! PostgreSQL (`$1, $2, …`) at execution time.

/// Converts `?` placeholders to `$1..$n` (PostgreSQL).
pub fn bind_placeholders(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut n = 0usize;
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '?' => {
                n += 1;
                out.push('$');
                out.push_str(&n.to_string());
            }
            '\'' => {
                // copy string literal verbatim (placeholders inside are data, not binds)
                out.push(c);
                while let Some(&next) = chars.peek() {
                    out.push(next);
                    chars.next();
                    if next == '\'' {
                        if chars.peek() == Some(&'\'') {
                            out.push('\'');
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_rewrite() {
        assert_eq!(
            bind_placeholders("SELECT * FROM t WHERE a = ? AND b = ?"),
            "SELECT * FROM t WHERE a = $1 AND b = $2"
        );
    }

    #[test]
    fn postgres_ignores_placeholders_in_string_literals() {
        assert_eq!(bind_placeholders("SELECT '?' AS x WHERE a = ?"), "SELECT '?' AS x WHERE a = $1");
    }
}
