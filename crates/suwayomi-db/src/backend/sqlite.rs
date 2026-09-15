//! SQLite backend — `rheos-tokio-rusqlite` (a `rusqlite` connection pinned to
//! one dedicated OS thread, addressed over a channel).
//!
//! One connection for the whole server: SQLite serialises writers anyway, WAL
//! gives concurrent readers, and this crate's `call` closure is not re-entrant,
//! so a second handle would only add lock contention.
//!
//! Note the contract of `call`: the closure must be `Send + 'static`, so the
//! SQL text and the parameters are moved in on every invocation. That is why
//! [`crate::dialect::plan`] runs *before* the closure rather than inside it.

use std::path::Path;

use rheos_tokio_rusqlite::Connection;
use rusqlite::types::{ToSqlOutput, ValueRef};

use crate::error::{Error, Result};
use crate::query::QueryResult;
use crate::row::Row;
use crate::value::Value;

/// The SQLite handle.
#[derive(Clone)]
pub struct SqliteBackend {
    conn: Connection,
}

impl SqliteBackend {
    /// Opens (creating parent directories as needed) the database at `path`.
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::Sqlite(format!("cannot create {}: {e}", parent.display())))?;
        }
        let conn = Connection::open(path).await.map_err(Error::from)?;
        let backend = Self { conn };
        backend.apply_pragmas().await?;
        Ok(backend)
    }

    /// Opens a throw-away in-memory database (tests).
    pub async fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().await.map_err(Error::from)?;
        let backend = Self { conn };
        backend.apply_pragmas().await?;
        Ok(backend)
    }

    /// WAL + a generous busy timeout: the GraphQL layer fans out concurrent
    /// readers while background jobs write.
    async fn apply_pragmas(&self) -> Result<()> {
        self.conn
            .call(|c| {
                // `call` hands back `rheos_tokio_rusqlite::Result`, which the
                // plain `?` on a `rusqlite::Error` converts into.
                Ok(c.execute_batch(
                    "PRAGMA journal_mode = WAL;\n\
                     PRAGMA synchronous = NORMAL;\n\
                     PRAGMA foreign_keys = ON;\n\
                     PRAGMA busy_timeout = 30000;",
                )?)
            })
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    /// Runs a statement.
    pub async fn execute(&self, sql: String, params: Vec<Value>) -> Result<QueryResult> {
        let conn = self.conn.clone();
        let affected = conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(&sql)?;
                Ok(stmt.execute(rusqlite::params_from_iter(params.iter().map(SqlParam)))?)
            })
            .await
            .map_err(Error::from)?;
        Ok(QueryResult::new(affected as u64))
    }

    /// Runs a query and materialises the rows.
    pub async fn fetch(&self, sql: String, params: Vec<Value>) -> Result<Vec<Row>> {
        let conn = self.conn.clone();
        let rows = conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(&sql)?;
                let columns: Vec<String> = stmt.column_names().iter().map(|c| (*c).to_owned()).collect();
                let mut cursor = stmt.query(rusqlite::params_from_iter(params.iter().map(SqlParam)))?;
                let mut out = Vec::new();
                while let Some(row) = cursor.next()? {
                    let mut values = Vec::with_capacity(columns.len());
                    for i in 0..columns.len() {
                        values.push(cell(row.get_ref(i)?));
                    }
                    out.push(Row::new(columns.clone(), values));
                }
                Ok(out)
            })
            .await
            .map_err(Error::from)?;
        Ok(rows)
    }

    /// Runs a multi-statement script.
    pub async fn batch(&self, sql: &str) -> Result<()> {
        let conn = self.conn.clone();
        let script = sql.to_owned();
        conn.call(move |c| Ok(c.execute_batch(&script)?)).await.map_err(Error::from)?;
        Ok(())
    }

    /// Closes the connection thread.
    pub async fn close(self) -> Result<()> {
        self.conn.close().await.map_err(Error::from)?;
        Ok(())
    }
}

/// Binds a [`Value`] into rusqlite.
struct SqlParam<'a>(&'a Value);

impl rusqlite::ToSql for SqlParam<'_> {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(match self.0 {
            Value::Null => ToSqlOutput::Borrowed(ValueRef::Null),
            Value::Int(v) => ToSqlOutput::Borrowed(ValueRef::Integer(*v)),
            Value::Real(v) => ToSqlOutput::Borrowed(ValueRef::Real(*v)),
            Value::Bool(v) => ToSqlOutput::Borrowed(ValueRef::Integer(i64::from(*v))),
            Value::Text(v) => ToSqlOutput::Borrowed(ValueRef::Text(v.as_bytes())),
            Value::Blob(v) => ToSqlOutput::Borrowed(ValueRef::Blob(v)),
            // The planner expands arrays into an IN list before we get here.
            Value::Array(_) => {
                return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(Error::Other(
                    "array parameters must be expanded by the dialect planner".to_owned(),
                ))));
            }
        })
    }
}

/// Converts one SQLite cell.
fn cell(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(v) => Value::Int(v),
        ValueRef::Real(v) => Value::Real(v),
        ValueRef::Text(v) => Value::Text(String::from_utf8_lossy(v).into_owned()),
        ValueRef::Blob(v) => Value::Blob(v.to_vec()),
    }
}
