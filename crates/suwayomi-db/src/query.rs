//! Query builders — the sqlx surface the tree already speaks.
//!
//! `suwayomi_db::query(sql).bind(v).fetch_all(&db)` replaces
//! `sqlx::query(sql).bind(v).fetch_all(db.pool())` one-for-one; the only
//! differences are that the backend is chosen at runtime and that the
//! placeholders are rewritten per dialect on the way out.

use std::marker::PhantomData;

use crate::backend::Db;
use crate::error::{Error, Result};
use crate::from_row::FromRow;
use crate::row::Row;
use crate::value::{Decode, Encode, Value};

/// Rows touched by a non-query statement (`sqlx::postgres::PgQueryResult`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueryResult {
    rows_affected: u64,
}

impl QueryResult {
    /// Number of rows the statement affected.
    pub fn rows_affected(&self) -> u64 {
        self.rows_affected
    }

    /// Builds a result (used by the backends).
    pub fn new(rows_affected: u64) -> Self {
        Self { rows_affected }
    }
}

/// Anything a query can be run against — every handle in the tree is a [`Db`]
/// (or a reference to one), so `db.pool()` and `&db` are interchangeable.
pub trait Executor {
    /// The underlying handle.
    fn db(&self) -> &Db;
}

impl Executor for Db {
    fn db(&self) -> &Db {
        self
    }
}

impl<T: Executor + ?Sized> Executor for &T {
    fn db(&self) -> &Db {
        (**self).db()
    }
}

/// `query(sql)` — statements and untyped row reads.
pub struct Query<'q> {
    sql: &'q str,
    params: Vec<Value>,
}

/// `query_as::<T>(sql)` — maps each row onto `T`.
pub struct QueryAs<'q, O> {
    sql: &'q str,
    params: Vec<Value>,
    marker: PhantomData<fn() -> O>,
}

/// `query_scalar::<T>(sql)` — reads the first column.
pub struct QueryScalar<'q, O> {
    sql: &'q str,
    params: Vec<Value>,
    marker: PhantomData<fn() -> O>,
}

/// Starts a statement query.
pub fn query(sql: &str) -> Query<'_> {
    Query { sql, params: Vec::new() }
}

/// Starts a row-mapping query with the target type inferred from context or
/// given explicitly (`query_as::<CategoryRow>(sql)`).
pub fn query_as<O>(sql: &str) -> QueryAs<'_, O> {
    QueryAs { sql, params: Vec::new(), marker: PhantomData }
}

/// Starts a scalar query.
pub fn query_scalar<O>(sql: &str) -> QueryScalar<'_, O> {
    QueryScalar { sql, params: Vec::new(), marker: PhantomData }
}

impl Query<'_> {
    /// Binds one parameter, in placeholder order.
    pub fn bind<T: Encode>(mut self, value: T) -> Self {
        self.params.push(value.encode());
        self
    }

    /// Runs the statement and reports the affected-row count.
    pub async fn execute<E: Executor>(self, executor: E) -> Result<QueryResult> {
        executor.db().execute_sql(self.sql, &self.params).await
    }

    /// Fetches every row.
    pub async fn fetch_all<E: Executor>(self, executor: E) -> Result<Vec<Row>> {
        executor.db().fetch_sql(self.sql, &self.params).await
    }

    /// Fetches exactly one row, erroring with [`Error::RowNotFound`].
    pub async fn fetch_one<E: Executor>(self, executor: E) -> Result<Row> {
        let rows = executor.db().fetch_sql(self.sql, &self.params).await?;
        rows.into_iter().next().ok_or(Error::RowNotFound)
    }

    /// Fetches zero or one row.
    pub async fn fetch_optional<E: Executor>(self, executor: E) -> Result<Option<Row>> {
        let rows = executor.db().fetch_sql(self.sql, &self.params).await?;
        Ok(rows.into_iter().next())
    }
}

impl<O: FromRow> QueryAs<'_, O> {
    /// Binds one parameter, in placeholder order.
    pub fn bind<T: Encode>(mut self, value: T) -> Self {
        self.params.push(value.encode());
        self
    }

    /// Runs the statement and reports the affected-row count.
    pub async fn execute<E: Executor>(self, executor: E) -> Result<QueryResult> {
        executor.db().execute_sql(self.sql, &self.params).await
    }

    /// Fetches every row, mapped onto `O`.
    pub async fn fetch_all<E: Executor>(self, executor: E) -> Result<Vec<O>> {
        let rows = executor.db().fetch_sql(self.sql, &self.params).await?;
        rows.iter().map(O::from_row).collect()
    }

    /// Fetches exactly one mapped row.
    pub async fn fetch_one<E: Executor>(self, executor: E) -> Result<O> {
        let rows = executor.db().fetch_sql(self.sql, &self.params).await?;
        let row = rows.into_iter().next().ok_or(Error::RowNotFound)?;
        O::from_row(&row)
    }

    /// Fetches zero or one mapped row.
    pub async fn fetch_optional<E: Executor>(self, executor: E) -> Result<Option<O>> {
        let rows = executor.db().fetch_sql(self.sql, &self.params).await?;
        match rows.into_iter().next() {
            Some(row) => O::from_row(&row).map(Some),
            None => Ok(None),
        }
    }
}

impl<O: Decode> QueryScalar<'_, O> {
    /// Binds one parameter, in placeholder order.
    pub fn bind<T: Encode>(mut self, value: T) -> Self {
        self.params.push(value.encode());
        self
    }

    /// Runs the statement and reports the affected-row count.
    pub async fn execute<E: Executor>(self, executor: E) -> Result<QueryResult> {
        executor.db().execute_sql(self.sql, &self.params).await
    }

    /// Fetches the first column of every row.
    pub async fn fetch_all<E: Executor>(self, executor: E) -> Result<Vec<O>> {
        let rows = executor.db().fetch_sql(self.sql, &self.params).await?;
        rows.iter().map(|row| row.try_get::<O, _>(0usize)).collect()
    }

    /// Fetches the first column of the first row.
    pub async fn fetch_one<E: Executor>(self, executor: E) -> Result<O> {
        let rows = executor.db().fetch_sql(self.sql, &self.params).await?;
        let row = rows.into_iter().next().ok_or(Error::RowNotFound)?;
        row.try_get::<O, _>(0usize)
    }

    /// Fetches the first column of the first row, if any.
    pub async fn fetch_optional<E: Executor>(self, executor: E) -> Result<Option<O>> {
        let rows = executor.db().fetch_sql(self.sql, &self.params).await?;
        match rows.into_iter().next() {
            Some(row) => row.try_get::<O, _>(0usize).map(Some),
            None => Ok(None),
        }
    }
}
