//! Dynamic SQL values and the encode/decode traits that bridge them to Rust
//! types.
//!
//! The old code leaned on sqlx's compile-time `Type<DB>` plumbing to pick a
//! wire type per bound value. A two-backend layer cannot do that (SQLite is
//! dynamically typed, PostgreSQL is not), so the currency is a single owned
//! enum: callers bind Rust values, we materialise a [`Value`], and each
//! backend converts it to its own representation.

use crate::error::{Error, Result};

/// A dynamically typed SQL value.
///
/// `Array` only shows up for PostgreSQL `= ANY($n)`; the SQLite planner expands
/// it into an `IN (…)` list instead (see [`crate::dialect`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Bool(bool),
    Text(String),
    Blob(Vec<u8>),
    Array(Vec<Value>),
}

impl Value {
    /// `true` for [`Value::Null`].
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Human-readable type name, used in decode errors.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Int(_) => "integer",
            Self::Real(_) => "real",
            Self::Bool(_) => "boolean",
            Self::Text(_) => "text",
            Self::Blob(_) => "blob",
            Self::Array(_) => "array",
        }
    }

    /// Numeric view, coercing integer/real/boolean as needed.
    fn as_i64(&self, column: &str) -> Result<i64> {
        match self {
            Self::Int(v) => Ok(*v),
            Self::Bool(v) => Ok(i64::from(*v)),
            Self::Real(v) => Ok(*v as i64),
            other => Err(Error::Decode { column: column.to_owned(), expected: other.type_name() }),
        }
    }

    /// Floating-point view.
    fn as_f64(&self, column: &str) -> Result<f64> {
        match self {
            Self::Real(v) => Ok(*v),
            Self::Int(v) => Ok(*v as f64),
            other => Err(Error::Decode { column: column.to_owned(), expected: other.type_name() }),
        }
    }
}

/// A Rust value that can be bound as a query parameter.
pub trait Encode {
    /// Materialises the wire value.
    fn encode(self) -> Value;
}

/// A Rust value that can be read out of a result row.
pub trait Decode: Sized {
    /// Reads the value, or fails with a decode error naming `column`.
    fn decode(value: &Value, column: &str) -> Result<Self>;
}

impl Encode for Value {
    fn encode(self) -> Value {
        self
    }
}

impl Decode for Value {
    fn decode(value: &Value, _column: &str) -> Result<Self> {
        Ok(value.clone())
    }
}

/// `()` binds as SQL `NULL` (useful for tests and optional writes).
impl Encode for () {
    fn encode(self) -> Value {
        Value::Null
    }
}

impl Encode for bool {
    fn encode(self) -> Value {
        Value::Bool(self)
    }
}

/// Integer widths all normalise to 64-bit; SQLite has a single integer type and
/// PostgreSQL coerces the literal down to the column type.
macro_rules! encode_int {
    ($($ty:ty),*) => {$(
        impl Encode for $ty {
            fn encode(self) -> Value {
                Value::Int(self as i64)
            }
        }
    )*};
}

encode_int!(i8, i16, i32, i64, u8, u16, u32, isize, usize);

impl Encode for f32 {
    fn encode(self) -> Value {
        Value::Real(f64::from(self))
    }
}

impl Encode for f64 {
    fn encode(self) -> Value {
        Value::Real(self)
    }
}

/// Borrowed scalars bind exactly like their owned form.
///
/// Call sites frequently hold an id by reference (`for id in &ids { … bind(id) }`),
/// and a blanket `impl<T: Encode> Encode for &T` would collide with the concrete
/// `&str` / `&String` / `&[T]` / `&Vec<T>` / `&Option<T>` impls below, so the
/// scalar references are spelled out one type at a time.
macro_rules! encode_ref_int {
    ($($ty:ty),*) => {$(
        impl Encode for &$ty {
            fn encode(self) -> Value {
                Value::Int(*self as i64)
            }
        }
    )*};
}

encode_ref_int!(i8, i16, i32, i64, u8, u16, u32, isize, usize);

impl Encode for &bool {
    fn encode(self) -> Value {
        Value::Bool(*self)
    }
}

impl Encode for &f32 {
    fn encode(self) -> Value {
        Value::Real(f64::from(*self))
    }
}

impl Encode for &f64 {
    fn encode(self) -> Value {
        Value::Real(*self)
    }
}

impl Encode for String {
    fn encode(self) -> Value {
        Value::Text(self)
    }
}

impl Encode for &str {
    fn encode(self) -> Value {
        Value::Text(self.to_owned())
    }
}

impl Encode for &String {
    fn encode(self) -> Value {
        Value::Text(self.clone())
    }
}

impl Encode for Vec<u8> {
    fn encode(self) -> Value {
        Value::Blob(self)
    }
}

impl Encode for &[u8] {
    fn encode(self) -> Value {
        Value::Blob(self.to_vec())
    }
}

/// Slice/vec bindings are PostgreSQL arrays — see [`Value::Array`].
macro_rules! encode_array {
    ($($ty:ty => $inner:ty),*) => {$(
        impl Encode for Vec<$inner> {
            fn encode(self) -> Value {
                Value::Array(self.into_iter().map(|v| Value::Int(v as i64)).collect())
            }
        }
        impl Encode for &[$inner] {
            fn encode(self) -> Value {
                Value::Array(self.iter().map(|v| Value::Int(*v as i64)).collect())
            }
        }
        impl Encode for &Vec<$inner> {
            fn encode(self) -> Value {
                Value::Array(self.iter().map(|v| Value::Int(*v as i64)).collect())
            }
        }
    )*};
}

encode_array!(i32 => i32, i64 => i64);

impl<T: Encode> Encode for Option<T> {
    fn encode(self) -> Value {
        match self {
            Some(v) => v.encode(),
            None => Value::Null,
        }
    }
}

impl<T: Encode> Encode for &Option<T>
where
    T: Clone,
{
    fn encode(self) -> Value {
        match self {
            Some(v) => v.clone().encode(),
            None => Value::Null,
        }
    }
}

/// Integers decode from both integer and real columns (SQLite may hand back a
/// real for a NUMERIC-ish expression) with a range check.
macro_rules! decode_int {
    ($($ty:ty),*) => {$(
        impl Decode for $ty {
            fn decode(value: &Value, column: &str) -> Result<Self> {
                let raw = value.as_i64(column)?;
                <$ty>::try_from(raw).map_err(|_| Error::Decode {
                    column: column.to_owned(),
                    expected: stringify!($ty),
                })
            }
        }
    )*};
}

decode_int!(i8, i16, i32, i64, u8, u16, u32, isize, usize);

impl Decode for f32 {
    fn decode(value: &Value, column: &str) -> Result<Self> {
        Ok(value.as_f64(column)? as f32)
    }
}

impl Decode for f64 {
    fn decode(value: &Value, column: &str) -> Result<Self> {
        value.as_f64(column)
    }
}

impl Decode for bool {
    fn decode(value: &Value, column: &str) -> Result<Self> {
        match value {
            Value::Bool(v) => Ok(*v),
            // SQLite stores booleans as 0/1.
            Value::Int(v) => Ok(*v != 0),
            other => Err(Error::Decode { column: column.to_owned(), expected: other.type_name() }),
        }
    }
}

impl Decode for String {
    fn decode(value: &Value, column: &str) -> Result<Self> {
        match value {
            Value::Text(v) => Ok(v.clone()),
            // PostgreSQL hands back `char`/`name`/enum-ish types as text, but a
            // stray numeric comparison result should still be readable.
            Value::Int(v) => Ok(v.to_string()),
            Value::Real(v) => Ok(v.to_string()),
            Value::Bool(v) => Ok(v.to_string()),
            other => Err(Error::Decode { column: column.to_owned(), expected: other.type_name() }),
        }
    }
}

impl Decode for Vec<u8> {
    fn decode(value: &Value, column: &str) -> Result<Self> {
        match value {
            Value::Blob(v) => Ok(v.clone()),
            Value::Text(v) => Ok(v.clone().into_bytes()),
            other => Err(Error::Decode { column: column.to_owned(), expected: other.type_name() }),
        }
    }
}

impl<T: Decode> Decode for Option<T> {
    fn decode(value: &Value, column: &str) -> Result<Self> {
        match value {
            Value::Null => Ok(None),
            other => T::decode(other, column).map(Some),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_decodes_from_sqlite_int() {
        assert!(!bool::decode(&Value::Int(0), "c").unwrap());
        assert!(bool::decode(&Value::Int(1), "c").unwrap());
        assert!(bool::decode(&Value::Bool(true), "c").unwrap());
    }

    #[test]
    fn option_swallows_null() {
        assert_eq!(Option::<i32>::decode(&Value::Null, "c").unwrap(), None);
        assert_eq!(Option::<i32>::decode(&Value::Int(7), "c").unwrap(), Some(7));
    }

    #[test]
    fn int_range_is_checked() {
        assert!(i32::decode(&Value::Int(i64::MAX), "c").is_err());
        assert!(i32::decode(&Value::Int(42), "c").is_ok());
    }

    #[test]
    fn f32_reads_real_and_int() {
        assert_eq!(f32::decode(&Value::Real(1.5), "c").unwrap(), 1.5);
        assert_eq!(f32::decode(&Value::Int(2), "c").unwrap(), 2.0);
    }
}
