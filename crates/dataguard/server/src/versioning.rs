//! What the legacy chain keeps of each applied write, and the rows it reads.
//!
//! [`Versioning`] keeps the terms of every element the engine applied —
//! a write the tables refused is never applied — in [`Aliases`], which an
//! analysis of notes recognises the campaign's elements by
//! ([`Rows::known`]), and records each applied proposal among its note's.
//! Every other method is the inner store's.

use std::collections::BTreeMap;
use std::sync::Arc;

use aggregate_entity::Aliases;
use async_trait::async_trait;
use extract_from_notes::Known;
use harness_dataguard::Result;
use harness_dataguard::command::{CommandResult, Entry};
use harness_dataguard::queue::Hold;
use harness_dataguard::store::{Collision, Store, Write};
use serde_json::Value;

use crate::notes::Notes;
use crate::projection::ProjectingStore;

/// The partitions of the Entity aggregate: `Entity/<element id>`.
pub const ENTITY: &str = "Entity/";

/// The partitions of the NoteImport aggregate: `NoteImport/<note or
/// proposal id>`.
pub const NOTE_IMPORT: &str = "NoteImport/";

/// The element a write is of, when it is a write of the Entity aggregate.
pub fn element_of(write: &Write) -> Option<&str> {
    write.partition.strip_prefix(ENTITY)
}

/// The proposal a write is of, and its note, when it is a live write of a
/// proposal of the NoteImport aggregate.
fn proposal_of(write: &Write) -> Option<(&str, &str)> {
    let proposal = write.partition.strip_prefix(NOTE_IMPORT)?;
    let note = write.values.get("noteId").and_then(Value::as_str)?;
    Some((proposal, note))
}

/// A store whose applied elements' terms are kept, and whose applied
/// proposals are recorded among their note's.
pub struct Versioning {
    inner: Arc<dyn Store>,
    aliases: Arc<Aliases>,
    notes: Arc<Notes>,
}

impl Versioning {
    /// The store over `inner`, keeping the applied elements' terms in
    /// `aliases`.
    pub fn new(inner: Arc<dyn Store>, aliases: Arc<Aliases>) -> Self {
        Self {
            inner,
            aliases,
            notes: Arc::default(),
        }
    }

    /// The same, recording the proposals of each note in `notes`.
    #[must_use]
    pub fn noting(self, notes: Arc<Notes>) -> Self {
        Self { notes, ..self }
    }
}

#[async_trait]
impl Store for Versioning {
    async fn pending(&self) -> Result<Vec<Entry>> {
        self.inner.pending().await
    }

    async fn holds(&self) -> Result<Vec<Hold>> {
        self.inner.holds().await
    }

    async fn save(&self, entry: &Entry) -> Result<()> {
        self.inner.save(entry).await
    }

    async fn save_hold(&self, hold: &Hold, alive: bool) -> Result<()> {
        self.inner.save_hold(hold, alive).await
    }

    async fn data_version(&self) -> Result<u64> {
        self.inner.data_version().await
    }

    async fn row(&self, partition: &str) -> Result<BTreeMap<String, Value>> {
        self.inner.row(partition).await
    }

    async fn collision(&self, entry: &Entry) -> Result<Collision> {
        self.inner.collision(entry).await
    }

    async fn incoming(&self, partition: &str) -> Result<Vec<(String, u64)>> {
        self.inner.incoming(partition).await
    }

    async fn apply(&self, write: &Write, result: &CommandResult) -> Result<u64> {
        let Some(element) = element_of(write) else {
            let version = self.inner.apply(write, result).await?;
            if write.tombstone {
                if let Some(proposal) = write.partition.strip_prefix(NOTE_IMPORT) {
                    self.notes.forget(proposal);
                }
            } else if let Some((proposal, note)) = proposal_of(write) {
                self.notes.record(note, proposal);
            }
            return Ok(version);
        };
        let version = self.inner.apply(write, result).await?;
        if write.tombstone {
            self.aliases.forget(element);
        } else {
            self.aliases.record(element, &write.values);
        }
        Ok(version)
    }

    async fn journal(&self, result: &CommandResult) -> Result<()> {
        self.inner.journal(result).await
    }

    async fn result(&self, command: &str) -> Result<Option<CommandResult>> {
        self.inner.result(command).await
    }

    async fn by_idempotency_key(&self, data_capability: &str, key: &str) -> Result<Option<String>> {
        self.inner.by_idempotency_key(data_capability, key).await
    }
}

/// The rows the engine holds, as a command about to be submitted sees them,
/// and — over a database — which rows the engine deleted.
#[derive(Clone)]
pub struct Rows {
    store: Arc<dyn Store>,
    tables: Option<Arc<ProjectingStore>>,
    aliases: Arc<Aliases>,
    notes: Arc<Notes>,
}

impl Rows {
    /// The rows of `store`.
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self {
            store,
            tables: None,
            aliases: Arc::default(),
            notes: Arc::default(),
        }
    }

    /// The same, the proposals of each note read from `notes`.
    pub(crate) fn noting(self, notes: Arc<Notes>) -> Self {
        Self { notes, ..self }
    }

    /// The same, what the tables hold read from `tables`.
    pub(crate) fn reading(self, tables: Option<Arc<ProjectingStore>>) -> Self {
        Self { tables, ..self }
    }

    /// Whether the engine deleted `partition`'s row — its tombstone — which
    /// its rows read as empty, as one never written. Over the engine's memory
    /// none is: its rows keep a deleted row's values.
    ///
    /// # Errors
    ///
    /// [`harness_dataguard::Error::Store`] when the tables cannot be read.
    pub async fn deleted(&self, partition: &str) -> Result<bool> {
        let Some(tables) = &self.tables else {
            return Ok(false);
        };
        tables.tombstoned(partition).await.map_err(|e| {
            harness_dataguard::Error::Store(format!(
                "tombstone not read: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            ))
        })
    }

    /// The same, the elements' terms read from `aliases`.
    pub(crate) fn knowing(self, aliases: Arc<Aliases>) -> Self {
        Self { aliases, ..self }
    }

    /// The elements `campaign` holds, with their names and aliases: what
    /// an analysis of its notes recognises.
    pub fn known(&self, campaign: &str) -> Vec<Known> {
        self.aliases
            .known(campaign)
            .into_iter()
            .map(|element| Known {
                id: element.id,
                name: element.name,
                aliases: element.aliases,
            })
            .collect()
    }

    /// Whether a command of `data_capability` was submitted under `key`
    /// already: its replay is the engine's to answer, as it was queued.
    ///
    /// # Errors
    ///
    /// The store's.
    pub async fn replayed(&self, data_capability: &str, key: &str) -> Result<bool> {
        // The engine keeps a key under the id alone, its `@version` dropped.
        let id = data_capability.split('@').next().unwrap_or_default();
        self.store
            .by_idempotency_key(id, key)
            .await
            .map(|command| command.is_some())
    }

    /// The proposals of the imported note `note` names that the engine
    /// applied, by id: a proposal still queued may never be.
    pub fn proposals(&self, note: &str) -> Vec<String> {
        self.notes.of(note).into_iter().collect()
    }

    /// `partition`'s row as the commands queued on it will leave it: the row
    /// applied, then each pending command's payload, in queue order.
    ///
    /// # Errors
    ///
    /// The store's.
    pub async fn projected(&self, partition: &str) -> Result<BTreeMap<String, Value>> {
        let mut row = self.store.row(partition).await?;
        let mut ahead: Vec<Entry> = self
            .store
            .pending()
            .await?
            .into_iter()
            .filter(|entry| entry.partition == partition)
            .collect();
        ahead.sort_by_key(|entry| entry.position);
        for entry in ahead {
            row.extend(entry.payload);
        }
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use harness_dataguard::command::State;
    use harness_dataguard::memory::MemoryStore;
    use harness_dataguard::store::Collision;
    use serde_json::json;

    use super::*;

    const MORLOX: &str = "Entity/e1000000-0000-4000-8000-000000000003";

    fn write(values: Value, tombstone: bool) -> Write {
        let Value::Object(values) = values else {
            panic!("a row is an object");
        };
        Write {
            partition: MORLOX.to_string(),
            values: values.into_iter().collect(),
            tombstone,
        }
    }

    fn result() -> CommandResult {
        CommandResult {
            command_id: "c_1".to_string(),
            status: State::Applied,
            data_version: None,
            violations: Vec::new(),
            review_id: None,
            attachment: None,
        }
    }

    /// Morlox's row as a save leaves it.
    fn saved(name: &str) -> Value {
        json!({
            "entityId": "e1000000-0000-4000-8000-000000000003",
            "entityCampaignId": "c1000000-0000-4000-8000-000000000001",
            "type": "pnj",
            "name": name,
            "alias": ["le gobelin"],
        })
    }

    #[tokio::test]
    async fn an_applied_element_s_terms_are_known_until_it_is_removed() {
        let inner = Arc::new(MemoryStore::default());
        let aliases = Arc::new(Aliases::default());
        let store = Versioning::new(inner.clone(), aliases.clone());
        store
            .apply(&write(saved("Morlox"), false), &result())
            .await
            .expect("applied");
        assert_eq!(inner.row_now(MORLOX)["name"], "Morlox");
        let known = aliases.known("c1000000-0000-4000-8000-000000000001");
        assert_eq!(known.len(), 1);
        assert_eq!(known[0].aliases, ["le gobelin"]);

        store
            .apply(&write(json!({}), true), &result())
            .await
            .expect("removed");
        assert!(
            aliases
                .known("c1000000-0000-4000-8000-000000000001")
                .is_empty()
        );
    }

    /// A command pending on `partition`, at `position`, writing `payload`.
    fn pending(command: &str, partition: &str, position: u64, payload: Value) -> Entry {
        serde_json::from_value(json!({
            "command": command,
            "data_capability": "campaign.saveEntity@1",
            "by": "dm",
            "partition": partition,
            "position": position,
            "based_on": { "version": 0 },
            "payload": payload,
            "effect": "upsert",
            "touches": ["Entity.name"],
            "mode": "confirm_on_stale",
            "projection": { "confirmed": {}, "pending_ahead": [] },
            "state": "queued",
        }))
        .expect("an entry")
    }

    #[tokio::test]
    async fn every_call_but_an_applied_write_is_the_inner_store_s() {
        let collision = Collision {
            unique_key: Some("entity_alias_unique".to_owned()),
            trigger: None,
            dangling: Vec::new(),
        };
        let incoming = vec![("Campaign/c1".to_owned(), 4)];
        let inner = Arc::new(
            MemoryStore::default()
                .colliding("campaign.saveEntity", collision.clone())
                .pointed_at(MORLOX, incoming.clone()),
        );
        let store = Versioning::new(inner.clone(), Arc::default());
        let entry = pending("c_1", MORLOX, 1, json!({ "name": "Morlox" }));
        store.save(&entry).await.expect("saved");
        assert_eq!(store.pending().await.expect("pending"), vec![entry.clone()]);
        let hold: Hold = serde_json::from_value(json!({
            "id": "h_1",
            "partition": MORLOX,
            "by": "dm",
            "position": 1,
            "expires": 60,
            "ttl_seconds": 60,
        }))
        .expect("a hold");
        store.save_hold(&hold, true).await.expect("held");
        assert_eq!(store.holds().await.expect("holds"), vec![hold]);
        assert_eq!(store.collision(&entry).await.expect("collision"), collision);
        assert_eq!(store.incoming(MORLOX).await.expect("incoming"), incoming);
        store
            .apply(&write(json!({ "name": "Morlox" }), false), &result())
            .await
            .expect("applied");
        assert_eq!(store.data_version().await.expect("version"), 1);
        let journalled = CommandResult {
            command_id: "c_2".to_owned(),
            status: State::Rejected,
            ..result()
        };
        store.journal(&journalled).await.expect("journalled");
        assert_eq!(store.result("c_2").await.expect("result"), Some(journalled));
    }

    #[tokio::test]
    async fn a_row_projected_is_the_applied_one_and_its_own_partition_s_commands_in_order() {
        let inner = Arc::new(MemoryStore::default());
        let store: Arc<dyn Store> = inner.clone();
        store
            .apply(
                &write(json!({ "name": "Morlox", "notes": "applied" }), false),
                &result(),
            )
            .await
            .expect("applied");
        for entry in [
            pending("c_3", MORLOX, 3, json!({ "notes": "last" })),
            pending("c_2", MORLOX, 2, json!({ "notes": "first", "hpMax": 21 })),
            pending("c_9", "Entity/another", 1, json!({ "name": "Bandit" })),
        ] {
            store.save(&entry).await.expect("saved");
        }
        let row = Rows::new(store).projected(MORLOX).await.expect("projected");
        assert_eq!(row["name"], "Morlox");
        assert_eq!(row["notes"], "last");
        assert_eq!(row["hpMax"], 21);
    }
}
