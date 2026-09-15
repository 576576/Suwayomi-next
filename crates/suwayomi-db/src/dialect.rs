//! SQL dialect planning: one canonical query text, two concrete backends.
//!
//! Queries in this tree are written with SQLite-style `?` placeholders (some
//! sites run them through `suwayomi_domain::sql::bind_placeholders` first, which
//! yields `$1..$n`). Both spellings are accepted here and renumbered in order of
//! appearance, so the caller never has to care which backend is live.
//!
//! What each dialect needs on top of the placeholder rewrite:
//!
//! | construct | PostgreSQL | SQLite |
//! |---|---|---|
//! | `?` / `$1` | `$n` | `?n` |
//! | `= ANY(?)` bound to a list | `= ANY($n)` + array parameter | `IN (?a, ?b, …)` |
//! | `NULL` parameter | inlined as the `NULL` literal (a dynamic bind has no type to infer, unlike sqlx) | bound normally |
//! | `ILIKE` | native | rewritten to `LIKE` (ASCII case-insensitive) |
//! | `suwayomi.` schema prefix | native | stripped (SQLite has no schemas) |
//! | `x::type` casts | native | stripped |

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::value::Value;

/// Backend SQL dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Sqlite,
    Postgres,
}

/// A query text plus the parameters it actually binds.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    /// Backend-specific SQL.
    pub sql: String,
    /// Parameters, positionally matching the placeholders in `sql`.
    pub params: Vec<Value>,
}

/// Rewrites `sql` for `dialect` and selects the parameters it consumes.
///
/// Placeholders are consumed from `params` in appearance order; an explicit
/// `$n`/`?n` that appears twice reuses its first binding, matching PostgreSQL
/// semantics.
pub fn plan(dialect: Dialect, sql: &str, params: &[Value]) -> Result<Planned> {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len() + 16);
    let mut bound: Vec<Value> = Vec::new();
    // Explicit `$n` / `?n` token -> slot in `bound`.
    let mut explicit: HashMap<String, usize> = HashMap::new();
    let mut next = 0usize;
    let mut i = 0usize;

    while i < chars.len() {
        match chars[i] {
            '\'' => copy_quoted(&chars, &mut i, &mut out, '\''),
            '"' => copy_quoted(&chars, &mut i, &mut out, '"'),
            '-' if chars.get(i + 1) == Some(&'-') => {
                while i < chars.len() && chars[i] != '\n' {
                    out.push(chars[i]);
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                out.push('/');
                out.push('*');
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    out.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() {
                    out.push('*');
                    out.push('/');
                    i += 2;
                }
            }
            c @ ('?' | '$') => {
                // A dollar-quoted body (`$$…$$` / `$tag$…$tag$`) is opaque: the
                // `?` inside a PL/pgSQL function body is not a placeholder.
                if c == '$'
                    && let Some(tag_len) = dollar_quote_tag(&chars, i)
                {
                    let tag: Vec<char> = chars[i..i + tag_len].to_vec();
                    let mut j = i + tag_len;
                    let mut stop = chars.len();
                    while j + tag_len <= chars.len() {
                        if chars[j..j + tag_len] == tag[..] {
                            stop = j + tag_len;
                            break;
                        }
                        j += 1;
                    }
                    for ch in &chars[i..stop] {
                        out.push(*ch);
                    }
                    i = stop;
                    continue;
                }

                let mut j = i + 1;
                let mut digits = String::new();
                while j < chars.len() && chars[j].is_ascii_digit() {
                    digits.push(chars[j]);
                    j += 1;
                }
                // A bare `$` is not a placeholder.
                if c == '$' && digits.is_empty() {
                    out.push(c);
                    i += 1;
                    continue;
                }
                i = j;
                let key = if digits.is_empty() { None } else { Some(format!("{c}{digits}")) };
                let reuse = key.as_ref().and_then(|k| explicit.get(k).copied());
                let value = match reuse {
                    Some(slot) => bound[slot].clone(),
                    None => take(params, &mut next)?.clone(),
                };

                // `= ANY(?)` with a list parameter: SQLite has no array type, so
                // the whole call (operator included) becomes an IN list.
                if dialect == Dialect::Sqlite
                    && let Value::Array(items) = &value
                    && let Some(any_start) = trailing_any_start(&out)
                    && let Some(close) = closing_paren(&chars, i)
                {
                    out.truncate(any_start);
                    out.push_str("IN (");
                    // `IN ()` is a syntax error; `IN (NULL)` matches nothing,
                    // which is what `= ANY('{}')` evaluates to as well.
                    if items.is_empty() {
                        out.push_str("NULL");
                    }
                    for (n, item) in items.iter().enumerate() {
                        if n > 0 {
                            out.push_str(", ");
                        }
                        let slot = bound.len();
                        bound.push(item.clone());
                        emit(dialect, &mut out, slot);
                    }
                    out.push(')');
                    i = close + 1;
                    continue;
                }

                match reuse {
                    Some(slot) => emit(dialect, &mut out, slot),
                    None => {
                        // A dynamic NULL has no type for PostgreSQL to infer, so
                        // inline the literal instead of sending a parameter.
                        if dialect == Dialect::Postgres && value.is_null() {
                            out.push_str("NULL");
                            continue;
                        }
                        let slot = bound.len();
                        bound.push(value);
                        if let Some(key) = key {
                            explicit.insert(key, slot);
                        }
                        emit(dialect, &mut out, slot);
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }

    if dialect == Dialect::Sqlite {
        out = rewrite_sqlite(&out);
    }
    Ok(Planned { sql: out, params: bound })
}

/// Copies a `'…'` / `"…"` run verbatim (handles the doubled-delimiter escape).
fn copy_quoted(chars: &[char], i: &mut usize, out: &mut String, quote: char) {
    out.push(quote);
    *i += 1;
    while *i < chars.len() {
        let c = chars[*i];
        out.push(c);
        *i += 1;
        if c == quote {
            if chars.get(*i) == Some(&quote) {
                out.push(quote);
                *i += 1;
            } else {
                break;
            }
        }
    }
}

/// Pulls the next parameter, reporting a binding mismatch rather than panicking.
fn take<'a>(params: &'a [Value], next: &mut usize) -> Result<&'a Value> {
    let value = params.get(*next).ok_or_else(|| {
        Error::Other("query uses more placeholders than values were bound".to_owned())
    })?;
    *next += 1;
    Ok(value)
}

/// Appends the dialect's placeholder for slot `index` (0-based).
fn emit(dialect: Dialect, out: &mut String, index: usize) {
    match dialect {
        Dialect::Postgres => {
            out.push('$');
        }
        Dialect::Sqlite => {
            out.push('?');
        }
    }
    out.push_str(&(index + 1).to_string());
}

/// Length of a dollar-quote opener (`$$` or `$tag$`) at `i`, if any.
fn dollar_quote_tag(chars: &[char], i: usize) -> Option<usize> {
    if chars.get(i) != Some(&'$') {
        return None;
    }
    let mut j = i + 1;
    while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
        j += 1;
    }
    (chars.get(j) == Some(&'$')).then_some(j - i + 1)
}

/// Byte offset where a trailing `= ANY(` (or bare `ANY(`) call starts.
///
/// `x = ANY(list)` is exactly `x IN (list)`, so the equality operator has to go
/// with the call — truncating at the `ANY` would leave a dangling `=`.
fn trailing_any_start(out: &str) -> Option<usize> {
    let trimmed = out.trim_end();
    let open = trimmed.len().checked_sub(1)?;
    if !trimmed.is_char_boundary(open) || trimmed.as_bytes()[open] != b'(' {
        return None;
    }
    let head = trimmed[..open].trim_end();
    let any_at = head.len().checked_sub(3)?;
    if !head.is_char_boundary(any_at) || !head[any_at..].eq_ignore_ascii_case("ANY") {
        return None;
    }
    // `someANY(...)` is a different function — require a word boundary.
    let before = head[..any_at].trim_end();
    if before.chars().last().is_some_and(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    // Drop the `=` of `= ANY(`, but not the one in `>=` / `<=` / `!=` / `<>`.
    // Without a plain equality operator there is no `IN (…)` equivalent, so
    // leave the SQL alone and let the backend report the syntax error rather
    // than emit something that means something else.
    let eq = before.len().checked_sub(1)?;
    if !before.is_char_boundary(eq) || before.as_bytes()[eq] != b'=' {
        return None;
    }
    let prev = before[..eq].trim_end().as_bytes().last().copied();
    if matches!(prev, Some(b'>') | Some(b'<') | Some(b'!')) {
        return None;
    }
    // `eq` keeps the whitespace that separated the column from `=`.
    Some(eq)
}

/// Index of the `)` that closes the call, skipping whitespace.
fn closing_paren(chars: &[char], mut i: usize) -> Option<usize> {
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    (i < chars.len() && chars[i] == ')').then_some(i)
}

/// SQLite has no schemas, no `ILIKE` and no PostgreSQL cast syntax; strip them
/// outside string literals.
fn rewrite_sqlite(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0usize;
    while i < chars.len() {
        match chars[i] {
            '\'' => copy_quoted(&chars, &mut i, &mut out, '\''),
            '"' => copy_quoted(&chars, &mut i, &mut out, '"'),
            // `suwayomi.manga` -> `manga`
            _ if starts_with_ci(&chars, i, "suwayomi.") => i += 9,
            // `chapter_number::float4` -> `chapter_number`
            ':' if chars.get(i + 1) == Some(&':') => {
                let mut j = i + 2;
                while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                // `::float4[]`, `::numeric(10, 2)`
                if chars.get(j) == Some(&'[') && chars.get(j + 1) == Some(&']') {
                    j += 2;
                } else if chars.get(j) == Some(&'(') {
                    while j < chars.len() && chars[j] != ')' {
                        j += 1;
                    }
                    j += 1;
                }
                i = j;
            }
            'I' | 'i' if starts_with_ci(&chars, i, "ILIKE") && !is_word_char(chars.get(i + 5)) => {
                out.push_str("LIKE");
                i += 5;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Case-insensitive `needle` match at `i`.
fn starts_with_ci(chars: &[char], i: usize, needle: &str) -> bool {
    let n: Vec<char> = needle.chars().collect();
    if i + n.len() > chars.len() {
        return false;
    }
    chars[i..i + n.len()]
        .iter()
        .zip(n.iter())
        .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// `true` when the option is an identifier character (so `ILIKE` in `ILIKES`
/// is not rewritten).
fn is_word_char(c: Option<&char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || *c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ints(v: &[i32]) -> Value {
        Value::Array(v.iter().map(|x| Value::Int(i64::from(*x))).collect())
    }

    #[test]
    fn renumbers_bare_placeholders_per_dialect() {
        let p = plan(Dialect::Postgres, "SELECT * FROM t WHERE a = ? AND b = ?", &[Value::Int(1), Value::Int(2)])
            .unwrap();
        assert_eq!(p.sql, "SELECT * FROM t WHERE a = $1 AND b = $2");

        let p = plan(Dialect::Sqlite, "SELECT * FROM t WHERE a = ? AND b = ?", &[Value::Int(1), Value::Int(2)])
            .unwrap();
        assert_eq!(p.sql, "SELECT * FROM t WHERE a = ?1 AND b = ?2");
    }

    #[test]
    fn accepts_preconverted_dollar_placeholders() {
        let p = plan(Dialect::Sqlite, "SELECT * FROM t WHERE a = $1", &[Value::Int(1)]).unwrap();
        assert_eq!(p.sql, "SELECT * FROM t WHERE a = ?1");
        assert_eq!(p.params, vec![Value::Int(1)]);
    }

    #[test]
    fn placeholders_inside_literals_are_untouched() {
        let p = plan(Dialect::Postgres, "SELECT '?' AS x, 'a''b' WHERE a = ?", &[Value::Int(1)]).unwrap();
        assert_eq!(p.sql, "SELECT '?' AS x, 'a''b' WHERE a = $1");
    }

    #[test]
    fn any_list_expands_on_sqlite_and_stays_an_array_on_postgres() {
        let sql = "DELETE FROM page WHERE chapter = ANY($1)";
        let p = plan(Dialect::Sqlite, sql, &[ints(&[3, 4, 5])]).unwrap();
        assert_eq!(p.sql, "DELETE FROM page WHERE chapter IN (?1, ?2, ?3)");
        assert_eq!(p.params, vec![Value::Int(3), Value::Int(4), Value::Int(5)]);

        let p = plan(Dialect::Postgres, sql, &[ints(&[3, 4, 5])]).unwrap();
        assert_eq!(p.sql, "DELETE FROM page WHERE chapter = ANY($1)");
        assert_eq!(p.params.len(), 1);
    }

    #[test]
    fn null_parameters_are_inlined_for_postgres() {
        let p = plan(Dialect::Postgres, "UPDATE t SET a = ? WHERE id = ?", &[Value::Null, Value::Int(4)]).unwrap();
        assert_eq!(p.sql, "UPDATE t SET a = NULL WHERE id = $1");
        assert_eq!(p.params, vec![Value::Int(4)]);

        // SQLite types values dynamically, so NULL stays a parameter there.
        let p = plan(Dialect::Sqlite, "UPDATE t SET a = ? WHERE id = ?", &[Value::Null, Value::Int(4)]).unwrap();
        assert_eq!(p.sql, "UPDATE t SET a = ?1 WHERE id = ?2");
        assert_eq!(p.params, vec![Value::Null, Value::Int(4)]);
    }

    #[test]
    fn sqlite_rewrites_schema_prefix_ilike_and_casts() {
        let p = plan(
            Dialect::Sqlite,
            "SELECT chapter_number::float4 FROM suwayomi.chapter WHERE name ILIKE ? OR name = 'ILIKE suwayomi.x'",
            &[Value::Text("%a%".into())],
        )
        .unwrap();
        assert_eq!(p.sql, "SELECT chapter_number FROM chapter WHERE name LIKE ?1 OR name = 'ILIKE suwayomi.x'");
    }

    #[test]
    fn postgres_keeps_ilike_and_the_schema_prefix() {
        let p =
            plan(Dialect::Postgres, "SELECT id FROM suwayomi.manga WHERE title ILIKE ?", &[Value::Text("%a%".into())])
                .unwrap();
        assert_eq!(p.sql, "SELECT id FROM suwayomi.manga WHERE title ILIKE $1");
    }

    #[test]
    fn empty_any_list_becomes_in_null() {
        let p = plan(Dialect::Sqlite, "SELECT * FROM t WHERE id = ANY(?)", &[Value::Array(Vec::new())]).unwrap();
        // `IN ()` would be a syntax error; `IN (NULL)` matches no row.
        assert_eq!(p.sql, "SELECT * FROM t WHERE id IN (NULL)");
        assert!(p.params.is_empty());
    }

    #[test]
    fn array_parameter_outside_an_any_call_is_left_for_the_backend() {
        let p = plan(Dialect::Sqlite, "SELECT * FROM t WHERE id = ?", &[ints(&[1, 2])]).unwrap();
        assert_eq!(p.sql, "SELECT * FROM t WHERE id = ?1");
        assert_eq!(p.params.len(), 1);

        // `<> ANY(…)` has no `IN` equivalent — do not invent one.
        let p = plan(Dialect::Sqlite, "SELECT * FROM t WHERE id <> ANY(?)", &[ints(&[1, 2])]).unwrap();
        assert_eq!(p.sql, "SELECT * FROM t WHERE id <> ANY(?1)");
    }

    #[test]
    fn binding_more_placeholders_than_values_is_an_error() {
        assert!(plan(Dialect::Sqlite, "SELECT ?", &[]).is_err());
    }

    #[test]
    fn dollar_quoted_bodies_are_not_treated_as_placeholders() {
        let p = plan(Dialect::Postgres, "SELECT $$a ? b$$", &[]).unwrap();
        assert_eq!(p.sql, "SELECT $$a ? b$$");
    }
}
