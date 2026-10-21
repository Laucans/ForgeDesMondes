//! The read side of the wire protocol: what `POST /capabilities/<id>`
//! answers, from the Capabilities of the product.
//!
//! A Capability is a pure function from the rows of its persisted query to
//! its answer. Every read is the registry's: every Capability crate's answer
//! (`dataguard_registry`), run by the engine's reads over a connection of
//! their own that reads and does not write — `DATABASE_URL_READONLY` when it
//! names one, else the read-write URL, every read then in a `READ ONLY`
//! transaction, which the database itself holds to. The query binds the
//! request's variables in the order of its `$1`, `$2`…; a variable the
//! caller did not give is `null`, which no row is, and the Capability
//! answers its "nothing" — null, or an empty list. Over the in-memory store
//! there is no table to read: a Capability answers from no row.
//!
//! The names ids go by on the wire, where they are not the ones the store
//! holds (the mock-up's fixture slugs), are the product server's own
//! (`crates/server`), around this module.

use std::collections::BTreeMap;
use std::sync::Arc;

use harness_dataguard::postgres::ReadOnly;
use harness_dataguard::{Error, Result};
use serde_json::Value;
use tokio_postgres::Client;

use crate::MEMORY;

/// A client on `url`, its connection driven in the background.
async fn connect(url: &str) -> std::result::Result<Client, tokio_postgres::Error> {
    let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls).await?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("postgres (reads): {e}");
        }
    });
    Ok(client)
}

/// The registry's Capabilities, over the connection reads go through, or
/// none.
pub struct Reads {
    registry: harness_dataguard::reads::Reads,
}

/// Every Capability crate's answer, as the registry gathers them, over no
/// database yet.
fn registered() -> harness_dataguard::reads::Reads {
    harness_dataguard::reads::Reads::from(&dataguard_registry::plugins())
}

impl Reads {
    /// No database: a Capability answers from no row.
    #[must_use]
    pub fn none() -> Self {
        Self {
            registry: registered(),
        }
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
        let registry = registered().with_connection(Arc::new(ReadOnly::new(connect(url).await?)));
        Ok(Self { registry })
    }

    /// What the Capability `id` answers to `variables`; `null` for one this
    /// product does not serve yet.
    ///
    /// # Errors
    ///
    /// [`Error::Store`] when the database refuses the query.
    pub async fn run(&self, id: &str, variables: &Value) -> Result<Value> {
        let variables: BTreeMap<String, Value> = variables
            .as_object()
            .map(|given| given.clone().into_iter().collect())
            .unwrap_or_default();
        match self.registry.run(id, &variables).await {
            Err(Error::UnregisteredQuery(_)) => Ok(Value::Null),
            answered => answered,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn over_the_in_memory_store_a_capability_answers_from_no_row() {
        let reads = Reads::open(MEMORY).await.expect("no database is reached");
        let settings = reads
            .run("ai.getSettings", &Value::Null)
            .await
            .expect("an answer");
        assert_eq!(settings["providers"], serde_json::json!({}));
        assert_eq!(settings["lastTest"], Value::Null);
    }

    #[tokio::test]
    async fn a_capability_the_product_does_not_serve_yet_answers_null() {
        let reads = Reads::none();
        for id in ["session.listSessions", "nothing", "campaign.createCampaign"] {
            assert_eq!(
                reads.run(id, &Value::Null).await.expect("an answer"),
                Value::Null,
                "{id}"
            );
        }
    }

    #[tokio::test]
    async fn over_the_in_memory_store_a_campaign_capability_answers_from_no_row() {
        let reads = Reads::none();
        let variables = serde_json::json!({ "campaign": "cendres-valombre", "id": "elowen" });
        let listed = reads
            .run("campaign.listPlayers", &variables)
            .await
            .expect("an answer");
        assert_eq!(listed, serde_json::json!([]));
        let notes = reads
            .run("campaign.listImportedNotes", &variables)
            .await
            .expect("an answer");
        assert_eq!(notes, serde_json::json!({ "imports": [], "proposals": [] }));
        for unknown in ["campaign.getCampaign", "campaign.getEntity"] {
            assert_eq!(
                reads.run(unknown, &variables).await.expect("an answer"),
                Value::Null,
                "{unknown}: an id no row holds has no answer"
            );
        }
    }

    #[tokio::test]
    async fn the_campaign_reads_are_the_registry_s() {
        // No Capability in the registry: nothing here answers a campaign read.
        let unregistered = Reads {
            registry: harness_dataguard::reads::Reads::default(),
        };
        let variables = serde_json::json!({ "campaign": "cendres-valombre" });
        for id in ["campaign.listPlayers", "campaign.listImportedNotes"] {
            assert_eq!(
                unregistered.run(id, &variables).await.expect("an answer"),
                Value::Null,
                "{id}"
            );
        }
        let listed = Reads::none()
            .run("campaign.listPlayers", &variables)
            .await
            .expect("an answer");
        assert_eq!(listed, serde_json::json!([]));
    }

    #[tokio::test]
    async fn the_ai_settings_are_the_registry_s_read() {
        // No Capability in the registry: nothing here answers ai.getSettings.
        let unregistered = Reads {
            registry: harness_dataguard::reads::Reads::default(),
        };
        assert_eq!(
            unregistered
                .run("ai.getSettings", &Value::Null)
                .await
                .expect("an answer"),
            Value::Null
        );
        let settings = Reads::none()
            .run("ai.getSettings", &Value::Null)
            .await
            .expect("an answer");
        assert!(settings["providers"].is_object(), "{settings}");
    }

    #[tokio::test]
    async fn a_read_of_another_system_is_the_registry_s_capability() {
        let reads = Reads::none();
        let variables = serde_json::json!({ "campaignId": "cendres-valombre" });
        let listed = reads
            .run("media.listMedia", &variables)
            .await
            .expect("an answer");
        assert_eq!(
            listed,
            serde_json::json!({ "cover": null, "battlemaps": [], "illustrations": [] }),
            "media.listMedia over no row, from its own crate"
        );
    }
}
