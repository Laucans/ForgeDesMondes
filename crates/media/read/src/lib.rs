//! The media system's read crate: what a media Capability reads through
//! (rule 2). It opens one connection, the read-only one — the database
//! `DATABASE_URL_READONLY` names, whose role holds no write privilege — and
//! runs a persisted query's SQL in a `READ ONLY` transaction. It has no
//! insert, no update, no delete, and no other URL to open: writing media is a
//! DataCapability's, through the DataGuard.

use serde_json::Value;

/// The variable naming the read-only database.
pub const READ_ONLY_URL: &str = "DATABASE_URL_READONLY";

/// Why a read does not happen.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `DATABASE_URL_READONLY` is not set: there is no fallback on the
    /// read-write URL.
    #[error("{READ_ONLY_URL} is not set: the media system reads through the read-only role only")]
    Unset,
    /// The database could not be reached.
    #[error("cannot reach the read-only database: {0}")]
    Connect(#[source] tokio_postgres::Error),
    /// The database refused the query, or its answer is not JSON.
    #[error("read: {0}")]
    Query(String),
}

/// The read-only connection of the media system.
pub struct MediaRead {
    client: tokio_postgres::Client,
}

impl MediaRead {
    /// Opens the connection `DATABASE_URL_READONLY` names, as `var` gives it
    /// — `std::env::var` in a binary.
    ///
    /// # Errors
    ///
    /// [`Error::Unset`] when the variable is absent or empty,
    /// [`Error::Connect`] when the database cannot be reached.
    pub async fn connect(var: impl Fn(&str) -> Option<String>) -> Result<Self, Error> {
        let url = var(READ_ONLY_URL)
            .filter(|url| !url.is_empty())
            .ok_or(Error::Unset)?;
        let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
            .await
            .map_err(Error::Connect)?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("postgres (media reads): {e}");
            }
        });
        Ok(Self { client })
    }

    /// The rows `sql` selects — the text of a persisted query, never a value
    /// a request carries — as a JSON array of objects keyed by column,
    /// read in a `READ ONLY` transaction that is rolled back after.
    ///
    /// # Errors
    ///
    /// [`Error::Query`] when the database refuses the query — a write
    /// included.
    pub async fn rows(&mut self, sql: &str) -> Result<Value, Error> {
        let wrapped = format!(
            "SELECT coalesce(jsonb_agg(to_jsonb(q)), '[]'::jsonb)::text FROM (\n{}\n) q",
            sql.trim().trim_end_matches(';')
        );
        let failed = |e: tokio_postgres::Error| Error::Query(e.to_string());
        let tx = self
            .client
            .build_transaction()
            .read_only(true)
            .start()
            .await
            .map_err(failed)?;
        let row = tx.query_one(&wrapped, &[]).await.map_err(failed)?;
        tx.rollback().await.map_err(failed)?;
        serde_json::from_str(&row.get::<_, String>(0)).map_err(|e| Error::Query(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A connection to the harness's read-only database, when the environment
    /// names one (CI does); none otherwise.
    async fn database() -> Option<MediaRead> {
        let url = std::env::var(READ_ONLY_URL).ok()?;
        Some(
            MediaRead::connect(|_| Some(url.clone()))
                .await
                .expect("the read-only database is reached"),
        )
    }

    #[tokio::test]
    async fn rows_come_back_as_json_objects_keyed_by_column() {
        let Some(mut media) = database().await else {
            eprintln!("{READ_ONLY_URL} is not set: no database to read");
            return;
        };
        let rows = media
            .rows("SELECT 1 AS n, 'a'::text AS s -- a trailing comment\n;")
            .await
            .expect("a read");
        assert_eq!(rows, serde_json::json!([{ "n": 1, "s": "a" }]));
        let none = media
            .rows("SELECT 1 AS n WHERE false")
            .await
            .expect("a read");
        assert_eq!(none, serde_json::json!([]));
    }

    #[tokio::test]
    async fn a_broken_query_is_a_query_error() {
        let Some(mut media) = database().await else {
            return;
        };
        let refused = media.rows("SELECT FROM nowhere_at_all").await.err();
        assert!(matches!(refused, Some(Error::Query(_))), "{refused:?}");
    }

    #[tokio::test]
    async fn without_the_read_only_url_nothing_is_opened() {
        for url in [None, Some(String::new())] {
            let refused = MediaRead::connect(|_| url.clone()).await.err();
            assert!(matches!(refused, Some(Error::Unset)), "{refused:?}");
        }
    }

    #[tokio::test]
    async fn it_reads_the_read_only_url_alone() {
        let asked = std::sync::Mutex::new(Vec::new());
        let refused = MediaRead::connect(|name| {
            asked.lock().unwrap().push(name.to_string());
            None
        })
        .await
        .err();
        assert!(refused.is_some());
        assert_eq!(*asked.lock().unwrap(), [READ_ONLY_URL]);
    }

    #[tokio::test]
    async fn an_unreachable_database_is_a_connect_error() {
        let refused = MediaRead::connect(|_| Some("postgres://reader@127.0.0.1:1/app".to_string()))
            .await
            .err();
        assert!(matches!(refused, Some(Error::Connect(_))), "{refused:?}");
    }
}
