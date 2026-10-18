//! The soundbox system's read crate: the one connection its Capabilities read
//! through, `DATABASE_URL_READONLY`, and no way to write.
//!
//! Its whole API is [`connect`] (or [`connect_to`], for a URL given) and
//! [`ReadOnly::query_json`]. A read is read-only twice over: by its role, and
//! by the `READ ONLY` transaction every query runs in, so PostgreSQL refuses
//! a write even where the URL names a role that could write.
//!
//! The crate stands alone inside the system: a Capability depends on it and
//! on no other system's crate, and holds no database driver of its own
//! (`scripts/isolation.sh`, rule 4).

use serde_json::Value;
use tokio_postgres::NoTls;

pub use tokio_postgres::types::ToSql;

/// The variable naming the read-only connection.
pub const READ_ONLY_URL: &str = "DATABASE_URL_READONLY";

/// Why a read did not happen.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `DATABASE_URL_READONLY` is not set.
    #[error("{0} is not set")]
    Unset(&'static str),
    /// The database could not be reached.
    #[error("database: {0}")]
    Database(#[source] tokio_postgres::Error),
    /// PostgreSQL refused the statement on the read-only connection: a write,
    /// or a privilege the read-only role has not.
    #[error("refused on the read-only connection: {0}")]
    Refused(#[source] tokio_postgres::Error),
    /// The query failed otherwise.
    #[error("query: {0}")]
    Query(#[source] tokio_postgres::Error),
    /// The rows came back as something other than JSON.
    #[error("rows are not JSON: {0}")]
    NotJson(#[source] serde_json::Error),
}

/// The read-only connection `DATABASE_URL_READONLY` names.
///
/// # Errors
///
/// [`Error::Unset`] without the variable, [`Error::Database`] when the
/// database cannot be reached.
pub async fn connect() -> Result<ReadOnly, Error> {
    connect_to(&url_of(|name| std::env::var(name).ok())?).await
}

/// The URL `var` gives for [`READ_ONLY_URL`].
fn url_of(var: impl Fn(&str) -> Option<String>) -> Result<String, Error> {
    var(READ_ONLY_URL).ok_or(Error::Unset(READ_ONLY_URL))
}

/// The read-only connection to the database `url` names, in the clear — as
/// the Data layer's connections are.
///
/// # Errors
///
/// [`Error::Database`] when the database cannot be reached.
pub async fn connect_to(url: &str) -> Result<ReadOnly, Error> {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .map_err(Error::Database)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(ReadOnly { client })
}

/// The read-only connection: it runs queries and nothing else.
pub struct ReadOnly {
    client: tokio_postgres::Client,
}

impl ReadOnly {
    /// Runs `sql` with `params` in a `READ ONLY` transaction, rolled back
    /// after, and answers its rows as a JSON array of objects keyed by column
    /// name, in the order the query gives them. The query is one bare
    /// `SELECT`, without a trailing `;`: it is wrapped in `to_jsonb`, so a
    /// column of any type reads as the JSON PostgreSQL gives it, and a SQL
    /// `NULL` is JSON `null`.
    ///
    /// # Errors
    ///
    /// [`Error::Refused`] when PostgreSQL refuses a write or a privilege,
    /// [`Error::Query`] when the query fails otherwise, [`Error::Database`]
    /// when the transaction cannot be opened or closed.
    pub async fn query_json(
        &mut self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Value, Error> {
        let wrapped = format!("SELECT to_jsonb(q)::text FROM ({sql}) q");
        let tx = self
            .client
            .build_transaction()
            .read_only(true)
            .start()
            .await
            .map_err(Error::Database)?;
        let rows = tx.query(&wrapped, params).await;
        let closed = tx.rollback().await;
        let rows = rows.map_err(refused_or_failed)?;
        closed.map_err(Error::Database)?;
        rows.iter()
            .map(|row| {
                let text: String = row.try_get(0).map_err(Error::Query)?;
                serde_json::from_str(&text).map_err(Error::NotJson)
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array)
    }
}

/// [`Error::Refused`] for a write or a privilege PostgreSQL refuses, else
/// [`Error::Query`].
fn refused_or_failed(e: tokio_postgres::Error) -> Error {
    use tokio_postgres::error::SqlState;
    match e.code() {
        Some(code)
            if *code == SqlState::READ_ONLY_SQL_TRANSACTION
                || *code == SqlState::INSUFFICIENT_PRIVILEGE =>
        {
            Error::Refused(e)
        }
        _ => Error::Query(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The public items of this file, by the line that declares them.
    fn public_items() -> Vec<&'static str> {
        include_str!("lib.rs")
            .lines()
            .take_while(|line| !line.starts_with("#[cfg(test)]"))
            .filter(|line| line.trim_start().starts_with("pub"))
            .collect()
    }

    #[test]
    fn the_read_only_variable_is_the_one_the_architecture_names() {
        assert_eq!(READ_ONLY_URL, "DATABASE_URL_READONLY");
    }

    #[test]
    fn the_api_offers_a_connection_and_a_read_and_no_write() {
        let items = public_items();
        let functions: Vec<&&str> = items.iter().filter(|l| l.contains("fn ")).collect();
        assert_eq!(functions.len(), 3, "{functions:?}");
        for line in &functions {
            let name = line.split("fn ").nth(1).unwrap_or_default();
            assert!(
                ["connect(", "connect_to(", "query_json("]
                    .iter()
                    .any(|allowed| name.starts_with(allowed)),
                "{line}"
            );
        }
    }

    #[test]
    fn without_its_variable_the_connection_is_not_opened() {
        let refused = url_of(|_| None).expect_err("no variable, no connection");
        assert!(matches!(refused, Error::Unset(READ_ONLY_URL)), "{refused}");
        assert_eq!(refused.to_string(), "DATABASE_URL_READONLY is not set");
        let url = url_of(|name| (name == READ_ONLY_URL).then(|| "postgres://r@h/db".to_string()));
        assert_eq!(url.expect("the variable is set"), "postgres://r@h/db");
    }

    #[tokio::test]
    async fn an_unreachable_database_is_an_error_not_a_panic() {
        let refused = connect_to("postgres://reader@127.0.0.1:1/none")
            .await
            .err()
            .expect("nothing listens on port 1");
        assert!(matches!(refused, Error::Database(_)), "{refused}");
    }
}
