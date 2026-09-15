//! PostgreSQL backend — `tokio-postgres` clients behind a `deadpool` pool.
//!
//! Mirrors the previous sqlx pool settings (≤24 connections, well under the 32
//! session cap a local embedded server had) and sets `search_path=suwayomi` as
//! a startup parameter so unqualified table names resolve exactly as before.

use deadpool_postgres::{Manager, Pool};
use tokio_postgres::types::{IsNull, ToSql, Type, to_sql_checked};

use crate::error::{Error, Result};
use crate::query::QueryResult;
use crate::row::Row;
use crate::value::Value;

/// The PostgreSQL handle.
#[derive(Clone)]
pub struct PostgresBackend {
    pool: Pool,
}

impl PostgresBackend {
    /// Connects to the server at `url` (a `postgres://` or key/value DSN).
    pub async fn connect(url: &str) -> Result<Self> {
        let mut config: tokio_postgres::Config =
            url.parse().map_err(|e| Error::Postgres(format!("invalid database URL: {e}")))?;
        // Every table lives in the `suwayomi` schema; the old pool did this in
        // an `after_connect` hook, a startup parameter is the cheaper equivalent.
        config.options("-c search_path=suwayomi");
        let manager = Manager::new(config, tokio_postgres::NoTls);
        let pool = Pool::builder(manager).max_size(24).build().map_err(|e| Error::Pool(e.to_string()))?;
        // Fail fast on an unreachable server instead of at the first query.
        let client = pool.get().await?;
        drop(client);
        Ok(Self { pool })
    }

    /// Runs a statement.
    pub async fn execute(&self, sql: String, params: Vec<Value>) -> Result<QueryResult> {
        let client = self.pool.get().await?;
        let owned = pg_params(&params);
        let refs: Vec<&(dyn ToSql + Sync)> = owned.iter().map(|v| v as &(dyn ToSql + Sync)).collect();
        let affected = client.execute(sql.as_str(), &refs).await?;
        Ok(QueryResult::new(affected))
    }

    /// Runs a query and materialises the rows.
    pub async fn fetch(&self, sql: String, params: Vec<Value>) -> Result<Vec<Row>> {
        let client = self.pool.get().await?;
        let owned = pg_params(&params);
        let refs: Vec<&(dyn ToSql + Sync)> = owned.iter().map(|v| v as &(dyn ToSql + Sync)).collect();
        let rows = client.query(sql.as_str(), &refs).await?;
        Ok(rows.iter().map(pg_row).collect())
    }

    /// Runs a multi-statement script.
    pub async fn batch(&self, sql: &str) -> Result<()> {
        let client = self.pool.get().await?;
        client.batch_execute(sql).await?;
        Ok(())
    }
}

/// One bindable parameter.
///
/// `tokio-postgres` takes `&[&(dyn ToSql + Sync)]`. A `Box<dyn ToSql + Send + Sync>`
/// cannot be coerced into that (trait-object upcasting is still unstable) *and*
/// would not be `Send` to hold across an await, so the parameters stay a concrete
/// enum instead — `Send + Sync + ToSql` for free.
#[derive(Debug, Clone)]
enum PgParam {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
    IntArray(Vec<i64>),
}

impl ToSql for PgParam {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut bytes::BytesMut,
    ) -> std::result::Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        // PostgreSQL infers each `$n` from its context (the target column, an
        // operator's operand, `LIMIT`, …) and hands that exact type to us. The
        // wire width must therefore come from `ty`, never from how the value
        // happens to be stored: `i32::to_sql` always writes four bytes, so
        // pairing it with an inferred `int8` sends a short parameter and the
        // server rejects the Bind message with
        // `insufficient data left in message`.
        // `Type::BOOL` and friends are associated constants, so the patterns
        // need an owned scrutinee; cloning a `Type` is a cheap enum copy.
        let inferred = ty.clone();
        match (self, inferred) {
            // The planner inlines NULLs for PostgreSQL, so this is a fallback.
            (Self::Null, _) => Option::<String>::None.to_sql(ty, out),
            (Self::Bool(v), _) => v.to_sql(ty, out),
            (Self::Int(v), Type::BOOL) => (*v != 0).to_sql(ty, out),
            (Self::Int(v), Type::INT2) => i16::try_from(*v).map_err(narrow_error)?.to_sql(ty, out),
            (Self::Int(v), Type::INT4) => i32::try_from(*v).map_err(narrow_error)?.to_sql(ty, out),
            (Self::Int(v), Type::OID) => u32::try_from(*v).map_err(narrow_error)?.to_sql(ty, out),
            (Self::Int(v), Type::FLOAT4) => (*v as f32).to_sql(ty, out),
            (Self::Int(v), Type::FLOAT8) => (*v as f64).to_sql(ty, out),
            (Self::Int(v), _) => v.to_sql(ty, out),
            (Self::Real(v), Type::FLOAT4) => (*v as f32).to_sql(ty, out),
            (Self::Real(v), Type::INT2) => i16::try_from(*v as i64).map_err(narrow_error)?.to_sql(ty, out),
            (Self::Real(v), Type::INT4) => i32::try_from(*v as i64).map_err(narrow_error)?.to_sql(ty, out),
            (Self::Real(v), Type::INT8) => (*v as i64).to_sql(ty, out),
            (Self::Real(v), _) => v.to_sql(ty, out),
            (Self::Text(v), _) => v.to_sql(ty, out),
            (Self::Blob(v), _) => v.to_sql(ty, out),
            // Arrays: match the inferred element width, `_int4` being the common
            // case (`= ANY($n)` against an `INTEGER` column).
            (Self::IntArray(v), _) if ty.name() == "_int4" => {
                let narrow: Vec<i32> = v.iter().map(|n| *n as i32).collect();
                narrow.to_sql(ty, out)
            }
            (Self::IntArray(v), _) if ty.name() == "_float8" => {
                let floats: Vec<f64> = v.iter().map(|n| *n as f64).collect();
                floats.to_sql(ty, out)
            }
            (Self::IntArray(v), _) => v.to_sql(ty, out),
        }
    }

    fn accepts(_ty: &Type) -> bool {
        // `to_sql` above already dispatches on the type; a mismatch surfaces as
        // an error from the delegate rather than a rejection here.
        true
    }

    to_sql_checked!();
}

/// Error for a value that does not fit the type PostgreSQL inferred.
fn narrow_error(value: impl std::fmt::Display) -> Box<dyn std::error::Error + Sync + Send> {
    format!("value {value} does not fit the parameter type PostgreSQL inferred").into()
}

/// Converts the dynamic parameter list into `tokio-postgres` bindables.
fn pg_params(values: &[Value]) -> Vec<PgParam> {
    values
        .iter()
        .map(|value| match value {
            Value::Null => PgParam::Null,
            Value::Bool(v) => PgParam::Bool(*v),
            Value::Int(v) => PgParam::Int(*v),
            Value::Real(v) => PgParam::Real(*v),
            Value::Text(v) => PgParam::Text(v.clone()),
            Value::Blob(v) => PgParam::Blob(v.clone()),
            Value::Array(items) => PgParam::IntArray(
                items
                    .iter()
                    .filter_map(|item| match item {
                        Value::Int(v) => Some(*v),
                        _ => None,
                    })
                    .collect(),
            ),
        })
        .collect()
}

/// Converts a PostgreSQL row.
fn pg_row(row: &tokio_postgres::Row) -> Row {
    let columns: Vec<String> = row.columns().iter().map(|c| c.name().to_owned()).collect();
    let values = row.columns().iter().enumerate().map(|(i, col)| pg_cell(row, i, col.type_())).collect();
    Row::new(columns, values)
}

/// Converts one cell, keyed off the column's wire type.
fn pg_cell(row: &tokio_postgres::Row, index: usize, ty: &Type) -> Value {
    macro_rules! read {
        ($rust:ty, $map:expr) => {
            match row.try_get::<_, Option<$rust>>(index) {
                Ok(Some(v)) => $map(v),
                Ok(None) => Value::Null,
                Err(e) => {
                    tracing::warn!(column = %row.columns()[index].name(), error = %e, "unreadable postgres column");
                    Value::Null
                }
            }
        };
    }

    match *ty {
        Type::BOOL => read!(bool, Value::Bool),
        Type::INT2 => read!(i16, |v| Value::Int(i64::from(v))),
        Type::INT4 => read!(i32, |v| Value::Int(i64::from(v))),
        Type::INT8 => read!(i64, Value::Int),
        Type::FLOAT4 => read!(f32, |v| Value::Real(f64::from(v))),
        Type::FLOAT8 => read!(f64, Value::Real),
        Type::BYTEA => read!(Vec<u8>, Value::Blob),
        Type::JSON | Type::JSONB => read!(serde_json::Value, |v: serde_json::Value| Value::Text(v.to_string())),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN | Type::CHAR => {
            read!(String, Value::Text)
        }
        _ => read!(String, Value::Text),
    }
}
