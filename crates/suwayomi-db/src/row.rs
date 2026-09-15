//! Result rows — a column-name index plus owned [`Value`]s.
//!
//! Kept deliberately simple (no borrowed buffers) because both backends
//! materialise their rows differently anyway: rusqlite hands out `ValueRef`
//! tied to the statement, tokio-postgres hands out `Row` tied to the client.

use crate::error::{Error, Result};
use crate::value::{Decode, Value};

/// One result row.
#[derive(Debug, Clone, Default)]
pub struct Row {
    columns: Vec<String>,
    values: Vec<Value>,
}

impl Row {
    /// Builds a row from column names and values (same length).
    pub fn new(columns: Vec<String>, values: Vec<Value>) -> Self {
        Self { columns, values }
    }

    /// Column names, in SELECT order.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// Number of columns.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// `true` when the row has no columns at all.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Column position for a name or index.
    fn position(&self, index: &str) -> Result<usize> {
        if let Ok(pos) = index.parse::<usize>() {
            if pos < self.values.len() {
                return Ok(pos);
            }
            return Err(Error::ColumnNotFound(index.to_owned()));
        }
        self.columns
            .iter()
            .position(|c| c.eq_ignore_ascii_case(index))
            .ok_or_else(|| Error::ColumnNotFound(index.to_owned()))
    }

    /// Reads a column, returning a decode error instead of panicking.
    ///
    /// Mirrors `sqlx::Row::try_get`, which is what the callers already use.
    pub fn try_get<T, I>(&self, index: I) -> Result<T>
    where
        T: Decode,
        I: ColumnIndex,
    {
        let name = index.name();
        let pos = self.position(&name)?;
        let value = &self.values[pos];
        // `Option<T>` handles NULL itself; everything else must report it.
        T::decode(value, &name)
    }

    /// Reads a column, panicking when it is missing or undecodable.
    ///
    /// Mirrors `sqlx::Row::get`.
    pub fn get<T, I>(&self, index: I) -> T
    where
        T: Decode,
        I: ColumnIndex,
    {
        match self.try_get(index) {
            Ok(v) => v,
            Err(e) => panic!("suwayomi-db: {e}"),
        }
    }
}

/// Anything usable as a column selector.
pub trait ColumnIndex {
    /// Borrowed name form (a numeric index is stringified by the caller).
    fn name(&self) -> std::borrow::Cow<'_, str>;
}

impl ColumnIndex for &str {
    fn name(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(self)
    }
}

impl ColumnIndex for String {
    fn name(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(self.as_str())
    }
}

impl ColumnIndex for &String {
    fn name(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(self.as_str())
    }
}

impl ColumnIndex for usize {
    fn name(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Owned(self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> Row {
        Row::new(
            vec!["id".into(), "title".into(), "read".into()],
            vec![Value::Int(7), Value::Text("x".into()), Value::Null],
        )
    }

    #[test]
    fn reads_by_name_case_insensitively() {
        assert_eq!(row().try_get::<i32, _>("ID").unwrap(), 7);
        assert_eq!(row().try_get::<String, _>("title").unwrap(), "x");
    }

    #[test]
    fn reads_by_position() {
        assert_eq!(row().try_get::<i32, _>(0usize).unwrap(), 7);
    }

    #[test]
    fn null_needs_an_option() {
        assert_eq!(row().try_get::<Option<bool>, _>("read").unwrap(), None);
        assert!(row().try_get::<bool, _>("read").is_err());
    }

    #[test]
    fn missing_column_is_an_error() {
        assert!(row().try_get::<i32, _>("nope").is_err());
    }
}
