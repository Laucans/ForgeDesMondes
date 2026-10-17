//! The read side of the wire protocol: what `POST /capabilities/<id>`
//! answers, from the Capabilities of the product.
//!
//! A Capability is a pure function from the rows of its persisted query to
//! its answer. The server runs the query here, over a connection of its own
//! that reads and does not write: `DATABASE_URL_READONLY` when it names one,
//! else the read-write URL — and every read then runs in a `READ ONLY`
//! transaction, which the database itself holds to. Over the in-memory store
//! there is no table to read: a Capability answers from no row.

use harness_dataguard::{Error, Result};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_postgres::Client;

use crate::MEMORY;

/// The connection reads go through, or none.
pub struct Reads {
    client: Option<Mutex<Client>>,
}

impl Reads {
    /// No database: a Capability answers from no row.
    #[must_use]
    pub const fn none() -> Self {
        Self { client: None }
    }

    /// The reads over the database `url` names — or [`none`](Self::none) for
    /// [`MEMORY`].
    ///
    /// # Errors
    ///
    /// [`crate::PlantError::Database`] when the database cannot be reached.
    pub async fn open(url: &str) -> std::result::Result<Self, crate::PlantError> {
        if url == MEMORY {
            return Ok(Self::none());
        }
        let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("postgres (reads): {e}");
            }
        });
        Ok(Self {
            client: Some(Mutex::new(client)),
        })
    }

    /// What the Capability `id` answers; `null` for one this product does not
    /// serve yet.
    ///
    /// # Errors
    ///
    /// [`Error::Store`] when the database refuses the query.
    pub async fn run(&self, id: &str) -> Result<Value> {
        match id {
            get_settings::CAPABILITY => {
                Ok(get_settings::answer(&self.rows(get_settings::SQL).await?))
            }
            _ => Ok(Value::Null),
        }
    }

    /// The rows `sql` selects, as a JSON array of objects keyed by column.
    async fn rows(&self, sql: &str) -> Result<Value> {
        let Some(client) = &self.client else {
            return Ok(Value::Array(Vec::new()));
        };
        let wrapped =
            format!("SELECT coalesce(jsonb_agg(to_jsonb(q)), '[]'::jsonb)::text FROM ({sql}) q");
        let mut client = client.lock().await;
        let store = |e: tokio_postgres::Error| Error::Store(format!("read: {e}"));
        let tx = client
            .build_transaction()
            .read_only(true)
            .start()
            .await
            .map_err(store)?;
        let row = tx.query_one(&wrapped, &[]).await.map_err(store)?;
        tx.rollback().await.map_err(store)?;
        serde_json::from_str(&row.get::<_, String>(0))
            .map_err(|e| Error::Store(format!("read: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn over_the_in_memory_store_a_capability_answers_from_no_row() {
        let reads = Reads::open(MEMORY).await.expect("no database is reached");
        let settings = reads.run("ai.getSettings").await.expect("an answer");
        assert_eq!(settings["providers"], serde_json::json!({}));
        assert_eq!(settings["lastTest"], Value::Null);
    }

    #[tokio::test]
    async fn a_capability_the_product_does_not_serve_yet_answers_null() {
        let reads = Reads::none();
        assert_eq!(
            reads
                .run("campaign.listCampaigns")
                .await
                .expect("an answer"),
            Value::Null
        );
    }
}
