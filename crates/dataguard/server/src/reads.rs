//! The read side of the wire protocol: what `POST /capabilities/<id>`
//! answers, from the Capabilities of the product.
//!
//! A Capability is a pure function from the rows of its persisted query to
//! its answer. The server runs the query here, over a connection of its own
//! that reads and does not write: `DATABASE_URL_READONLY` when it names one,
//! else the read-write URL — and every read then runs in a `READ ONLY`
//! transaction, which the database itself holds to. Over the in-memory store
//! there is no table to read: a Capability answers from no row.
//!
//! A read is routed by the system its identifier names (`ai.…`,
//! `campaign.…`) and carries its variables, which the query binds in the
//! order of its `$1`, `$2`… A variable the caller did not give is the empty
//! text: no row is that, and the Capability answers its "nothing" — null, or
//! an empty list. A read of any other system is the registry's: every
//! Capability crate's answer (`dataguard_registry`), run by the engine's
//! reads over a read-only connection of their own — the strangler's root,
//! which takes each system's reads over as this module lets them go.

use std::collections::BTreeMap;
use std::sync::Arc;

use harness_dataguard::postgres::ReadOnly;
use harness_dataguard::{Error, Result};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_postgres::Client;
use tokio_postgres::types::ToSql;

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

/// How a Capability turns the rows of its query, and the variable it was
/// given, into its answer.
type Answer = fn(&Value, &str) -> Value;

/// The Capabilities of the campaign system that are served: identifier, the
/// SQL of its persisted query, the one variable it binds to `$1` (none when
/// the query takes no parameter), and its answer. A Capability that lands
/// is one more row here and one dependency line in `Cargo.toml`.
const CAMPAIGN: [(&str, &str, Option<&str>, Answer); 9] = [
    (
        list_campaigns::CAPABILITY,
        list_campaigns::SQL,
        None,
        |rows, _| list_campaigns::answer_rows(rows),
    ),
    (
        get_campaign::CAPABILITY,
        get_campaign::SQL,
        Some("id"),
        |rows, id| get_campaign::answer_rows(rows, id).unwrap_or(Value::Null),
    ),
    (
        list_players::CAPABILITY,
        list_players::SQL,
        Some("campaign"),
        list_players::answer_rows,
    ),
    (
        list_characters::CAPABILITY,
        list_characters::SQL,
        Some("campaign"),
        list_characters::answer_rows,
    ),
    (
        list_places::CAPABILITY,
        list_places::SQL,
        Some("campaign"),
        list_places::answer_rows,
    ),
    (
        list_events::CAPABILITY,
        list_events::SQL,
        Some("campaign"),
        list_events::answer_rows,
    ),
    (
        get_entity::CAPABILITY,
        get_entity::SQL,
        Some("id"),
        get_entity::answer_rows,
    ),
    (
        list_entity_versions::CAPABILITY,
        list_entity_versions::SQL,
        Some("entity"),
        |rows, entity| list_entity_versions::answer_rows(rows, entity).unwrap_or(Value::Null),
    ),
    (
        list_imported_notes::CAPABILITY,
        list_imported_notes::SQL,
        Some("campaign"),
        list_imported_notes::answer_rows,
    ),
];

/// The connection reads go through, or none.
pub struct Reads {
    client: Option<Mutex<Client>>,
    names: Names,
    /// The registry's Capabilities, for the reads of the systems this
    /// module does not route.
    registry: harness_dataguard::reads::Reads,
}

/// Every Capability crate's answer, as the registry gathers them, over no
/// database yet.
fn registered() -> harness_dataguard::reads::Reads {
    harness_dataguard::reads::Reads::from(&dataguard_registry::plugins())
}

/// The names ids go by on the wire, where they are not the ones the store
/// holds: what a caller sends is turned into the stored id before the query
/// runs, and the stored id in an answer into the name the caller knows.
#[derive(Default)]
struct Names {
    /// Wire name → stored id.
    stored: BTreeMap<String, String>,
    /// Stored id → wire name.
    wire: BTreeMap<String, String>,
}

impl Names {
    /// `value` with every text that is a wire name turned into its stored id.
    fn to_stored(&self, value: &Value) -> Value {
        Self::rename(&self.stored, value)
    }

    /// `value` with every text that is a stored id turned into its wire name;
    /// with no name to give, `value` itself.
    fn to_wire(&self, value: Value) -> Value {
        if self.wire.is_empty() {
            value
        } else {
            Self::rename(&self.wire, &value)
        }
    }

    fn rename(by: &BTreeMap<String, String>, value: &Value) -> Value {
        match value {
            Value::String(text) => by
                .get(text)
                .map_or_else(|| value.clone(), |to| to.clone().into()),
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| Self::rename(by, v)).collect())
            }
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), Self::rename(by, v)))
                    .collect(),
            ),
            _ => value.clone(),
        }
    }
}

impl Reads {
    /// No database: a Capability answers from no row.
    #[must_use]
    pub fn none() -> Self {
        Self {
            client: None,
            names: Names::default(),
            registry: registered(),
        }
    }

    /// These `(wire name, stored id)` pairs as the names ids go by on the
    /// wire: the mock-up's fixtures name their rows by slug
    /// (`cendres-valombre`) while the store holds a uuid for each.
    #[must_use]
    pub fn named(mut self, pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        for (wire, stored) in pairs {
            self.names.wire.insert(stored.clone(), wire.clone());
            self.names.stored.insert(wire, stored);
        }
        self
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
        let client = connect(url).await?;
        let registry = registered().with_connection(Arc::new(ReadOnly::new(connect(url).await?)));
        Ok(Self {
            client: Some(Mutex::new(client)),
            names: Names::default(),
            registry,
        })
    }

    /// What the Capability `id` answers to `variables`; `null` for one this
    /// product does not serve yet.
    ///
    /// # Errors
    ///
    /// [`Error::Store`] when the database refuses the query.
    pub async fn run(&self, id: &str, variables: &Value) -> Result<Value> {
        let variables = self.names.to_stored(variables);
        let answer = match id.split_once('.') {
            Some(("ai", _)) => self.ai(id).await?,
            Some(("campaign", _)) => self.campaign(id, &variables).await?,
            _ => self.registered(id, &variables).await?,
        };
        Ok(self.names.to_wire(answer))
    }

    /// What the registry's Capability `id` answers to `variables`; `null`
    /// for one no unit answers.
    async fn registered(&self, id: &str, variables: &Value) -> Result<Value> {
        let variables: BTreeMap<String, Value> = variables
            .as_object()
            .map(|given| given.clone().into_iter().collect())
            .unwrap_or_default();
        match self.registry.run(id, &variables).await {
            Err(Error::UnregisteredQuery(_)) => Ok(Value::Null),
            answered => answered,
        }
    }

    async fn ai(&self, id: &str) -> Result<Value> {
        match id {
            get_settings::CAPABILITY => Ok(get_settings::answer_rows(
                &self.rows(get_settings::SQL, &[]).await?,
            )),
            _ => Ok(Value::Null),
        }
    }

    async fn campaign(&self, id: &str, variables: &Value) -> Result<Value> {
        let Some((_, sql, variable, answer)) = CAMPAIGN.iter().find(|(c, ..)| *c == id) else {
            return Ok(Value::Null);
        };
        // The one variable a Capability reads, the empty text when not given.
        let given = variable.map_or("", |name| variables[name].as_str().unwrap_or_default());
        let params: &[&(dyn ToSql + Sync)] = if variable.is_some() { &[&given] } else { &[] };
        Ok(answer(&self.rows(sql, params).await?, given))
    }

    /// The rows `sql` selects with `params` bound to its `$1`, `$2`…, as a
    /// JSON array of objects keyed by column.
    async fn rows(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> Result<Value> {
        let Some(client) = &self.client else {
            return Ok(Value::Array(Vec::new()));
        };
        let wrapped =
            format!("SELECT coalesce(jsonb_agg(to_jsonb(q)), '[]'::jsonb)::text FROM ({sql}) q");
        let mut client = client.lock().await;
        // The database's own message says what a bare "db error" does not;
        // its detail, which may quote a value, is left out.
        let store = |e: tokio_postgres::Error| {
            let why = e
                .as_db_error()
                .map_or_else(|| e.to_string(), |db| db.message().to_string());
            Error::Store(format!("read: {why}"))
        };
        let tx = client
            .build_transaction()
            .read_only(true)
            .start()
            .await
            .map_err(store)?;
        let row = tx.query_one(&wrapped, params).await.map_err(store)?;
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
    async fn the_systems_this_module_routes_never_reach_the_registry() {
        // No Capability in the registry: what answers is this module's own.
        let reads = Reads {
            registry: harness_dataguard::reads::Reads::default(),
            ..Reads::none()
        };
        let variables = serde_json::json!({ "campaign": "cendres-valombre" });
        let listed = reads
            .run("campaign.listPlayers", &variables)
            .await
            .expect("an answer");
        assert_eq!(listed, serde_json::json!([]));
        let settings = reads
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

    #[test]
    fn names_turn_the_callers_names_into_stored_ids_and_back() {
        let reads = Reads::none().named([
            ("elowen".to_string(), "uuid-elowen".to_string()),
            ("julie".to_string(), "uuid-julie".to_string()),
        ]);
        let sent = serde_json::json!({ "id": "elowen", "other": "unnamed", "n": 3 });
        assert_eq!(
            reads.names.to_stored(&sent),
            serde_json::json!({ "id": "uuid-elowen", "other": "unnamed", "n": 3 })
        );
        let answer = serde_json::json!([{ "id": "uuid-elowen", "player": "uuid-julie",
            "aliases": ["elowen", "uuid-x"], "note": "uuid-elowen is a name inside a text" }]);
        assert_eq!(
            reads.names.to_wire(answer),
            serde_json::json!([{ "id": "elowen", "player": "julie",
                "aliases": ["elowen", "uuid-x"], "note": "uuid-elowen is a name inside a text" }]),
            "a whole text is renamed, nested or not; a part of one never is"
        );
    }
}
