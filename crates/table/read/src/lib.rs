//! The table system's read crate: the one connection its Capabilities read
//! through, `DATABASE_URL_READONLY`, and no way to write.
//!
//! The connection is the Data layer's (`crates/data`, the adapter of the
//! `database` port — the one crate of the read side that opens PostgreSQL),
//! narrowed to what a Capability may do: [`connect`], then
//! [`ReadOnly::query_json`]. Nothing else is re-exported — no read-write
//! connection, no migration. A read is read-only twice over: by its role, and
//! by the `READ ONLY` transaction every query runs in, so PostgreSQL refuses a
//! write even where the URL names a role that could write.

pub use data::{Error, ReadOnly, ToSql};

/// The read-only connection `DATABASE_URL_READONLY` names.
///
/// # Errors
///
/// [`Error::Unset`] without the variable, [`Error::Database`] when the
/// database cannot be reached.
pub async fn connect() -> Result<ReadOnly, Error> {
    data::read_only().await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Whether the harness provides the read-only database, as CI does.
    fn provided() -> bool {
        std::env::var_os(data::READ_ONLY_URL).is_some()
    }

    #[test]
    fn the_api_offers_a_connection_and_a_read_and_no_write() {
        let source = include_str!("lib.rs");
        let outside_tests = source.split("#[cfg(test)]").next().unwrap_or_default();
        let functions: Vec<&str> = outside_tests
            .lines()
            .filter(|line| line.starts_with("pub ") && line.contains("fn "))
            .collect();
        assert_eq!(functions.len(), 1, "{functions:?}");
        assert!(functions[0].contains("fn connect("), "{functions:?}");
        let exports: Vec<&str> = outside_tests
            .lines()
            .filter(|line| line.starts_with("pub use "))
            .collect();
        assert_eq!(exports, ["pub use data::{Error, ReadOnly, ToSql};"]);
    }

    #[tokio::test]
    async fn without_its_variable_the_connection_is_not_opened() {
        if provided() {
            return;
        }
        let refused = connect().await.err().expect("no variable, no connection");
        assert!(
            matches!(refused, Error::Unset(data::READ_ONLY_URL)),
            "{refused}"
        );
    }

    #[tokio::test]
    async fn the_connection_reads_rows_as_json_and_refuses_a_write() {
        if !provided() {
            return;
        }
        let mut reader = connect()
            .await
            .expect("DATABASE_URL_READONLY names the database");
        let rows = reader
            .query_json("SELECT 1::int4 AS one, 'a'::text AS letter", &[])
            .await
            .expect("a read");
        assert_eq!(rows, json!([{ "one": 1, "letter": "a" }]));
        let refused = reader
            .query_json("CREATE TEMP TABLE table_read_probe (a int)", &[])
            .await
            .expect_err("a write is refused");
        assert!(matches!(refused, Error::Refused(_)), "{refused}");
    }
}
