//! `FromRow` — maps a [`Row`] onto a struct or tuple.
//!
//! Structs get the impl from `#[derive(FromRow)]` (re-exported from
//! `suwayomi-db-macros`); tuples decode positionally, which is what the many
//! `query_as::<(i32,)>` call sites rely on.

use crate::error::Result;
use crate::row::Row;
use crate::value::Decode;

/// A type that can be built from a result row.
pub trait FromRow: Sized {
    /// Reads the row's columns into `Self`.
    fn from_row(row: &Row) -> Result<Self>;
}

/// Re-exported derive macro (`#[derive(FromRow)]`).
pub use suwayomi_db_macros::FromRow;

/// A row is its own projection (handy for ad-hoc `query_as::<Row>`).
impl FromRow for Row {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(row.clone())
    }
}

macro_rules! impl_from_row_tuple {
    ($($idx:tt => $ty:ident),+) => {
        impl<$($ty: Decode),+> FromRow for ($($ty,)+) {
            fn from_row(row: &Row) -> Result<Self> {
                Ok(($(row.try_get::<$ty, _>($idx)?,)+))
            }
        }
    };
}

impl_from_row_tuple!(0 => A);
impl_from_row_tuple!(0 => A, 1 => B);
impl_from_row_tuple!(0 => A, 1 => B, 2 => C);
impl_from_row_tuple!(0 => A, 1 => B, 2 => C, 3 => D);
impl_from_row_tuple!(0 => A, 1 => B, 2 => C, 3 => D, 4 => E);
impl_from_row_tuple!(0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F);
impl_from_row_tuple!(0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => G);
impl_from_row_tuple!(0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => G, 7 => H);
impl_from_row_tuple!(0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => G, 7 => H, 8 => I);
impl_from_row_tuple!(0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => G, 7 => H, 8 => I, 9 => J);
impl_from_row_tuple!(
    0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => G, 7 => H, 8 => I, 9 => J, 10 => K
);
impl_from_row_tuple!(
    0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => G, 7 => H, 8 => I, 9 => J, 10 => K, 11 => L
);
impl_from_row_tuple!(
    0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => G, 7 => H, 8 => I, 9 => J, 10 => K, 11 => L, 12 => M
);
impl_from_row_tuple!(
    0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => G, 7 => H, 8 => I, 9 => J, 10 => K, 11 => L, 12 => M, 13 => N
);
impl_from_row_tuple!(
    0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => G, 7 => H, 8 => I, 9 => J, 10 => K, 11 => L, 12 => M, 13 => N,
    14 => O
);
impl_from_row_tuple!(
    0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => G, 7 => H, 8 => I, 9 => J, 10 => K, 11 => L, 12 => M, 13 => N,
    14 => O, 15 => P
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    fn row() -> Row {
        Row::new(vec!["a".into(), "b".into()], vec![Value::Int(1), Value::Text("x".into())])
    }

    #[test]
    fn tuple_decodes_positionally() {
        let (a, b): (i32, String) = FromRow::from_row(&row()).unwrap();
        assert_eq!((a, b), (1, "x".to_owned()));
    }

    #[test]
    fn single_element_tuple() {
        let (a,): (i32,) = FromRow::from_row(&row()).unwrap();
        assert_eq!(a, 1);
    }

    #[test]
    fn option_survives_missing_column() {
        // NULL, not absent: tuples read by position so every slot must exist.
        let row = Row::new(vec!["a".into(), "b".into()], vec![Value::Int(1), Value::Null]);
        let (a, b): (i32, Option<String>) = FromRow::from_row(&row).unwrap();
        assert_eq!(a, 1);
        assert_eq!(b, None);
    }
}
