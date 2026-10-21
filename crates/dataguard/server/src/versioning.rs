//! The Entity aggregate's versions, settled when a write is applied.
//!
//! A save is numbered, snapshotted and diffed when it is submitted, against
//! the row the commands queued ahead of it will leave ([`Rows::projected`]):
//! what the DataGuard judges. [`Versioning`] settles it at application, on
//! the single applier, against the row as it really is — the same
//! computation (`save_entity::versioned`): a command ahead that was
//! cancelled or rejected leaves no gap in the numbers and no change in the
//! diff that never happened. Over the engine's memory it keeps [`Aliases`]
//! too, which `alias-unique-in-campaign` reads, and the [`History`] of the
//! versions a restoration is prepared against; over a database the
//! projection keeps them, once the tables hold the write. Every other method
//! is the inner store's.

use std::collections::BTreeMap;
use std::sync::Arc;

use aggregate_entity::Aliases;
use async_trait::async_trait;
use extract_from_notes::Known;
use harness_dataguard::Result;
use harness_dataguard::command::{CommandResult, Entry};
use harness_dataguard::queue::Hold;
use harness_dataguard::store::{Collision, Store, Write};
use restore_entity_version::Kept;
use save_entity::{stamp, versioned};
use serde_json::Value;

use crate::history::History;
use crate::notes::Notes;
use crate::projection::{Placing, ProjectingStore};

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

/// A store whose applied Entity writes carry their settled version, and
/// whose applied proposals are recorded among their note's.
pub struct Versioning {
    inner: Arc<dyn Store>,
    aliases: Option<Arc<Aliases>>,
    history: Option<Arc<History>>,
    notes: Arc<Notes>,
}

impl Versioning {
    /// The store over `inner`, keeping `aliases` — `None` when `inner` keeps
    /// them itself, after its tables.
    pub fn new(inner: Arc<dyn Store>, aliases: Option<Arc<Aliases>>) -> Self {
        Self {
            inner,
            aliases,
            history: None,
            notes: Arc::default(),
        }
    }

    /// The same, keeping `history` too — as `aliases`, when `inner` does not.
    #[must_use]
    pub fn keeping(self, history: Option<Arc<History>>) -> Self {
        Self { history, ..self }
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
        if write.tombstone {
            let version = self.inner.apply(write, result).await?;
            if let Some(aliases) = &self.aliases {
                aliases.forget(element);
            }
            if let Some(history) = &self.history {
                history.forget(element);
            }
            return Ok(version);
        }
        let before = self.inner.row(&write.partition).await?;
        let mut values = write.values.clone();
        // A save carries a version of its own (`every-change-versioned`
        // refuses one reusing the last id); a write carrying none keeps the
        // row's.
        let versioning = values.get("entityVersionId") != before.get("entityVersionId");
        if versioning {
            let version = versioned(&before, &values);
            stamp(&mut values, version);
        }
        let settled = Write {
            partition: write.partition.clone(),
            values,
            tombstone: false,
        };
        let version = self.inner.apply(&settled, result).await?;
        if let Some(aliases) = &self.aliases {
            aliases.record(element, &settled.values);
        }
        if let Some(history) = self.history.as_ref().filter(|_| versioning)
            && let Some(kept) = Kept::of(&settled.values)
        {
            history.record(element, kept);
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
/// and — over a database — what its tables hold of a campaign, a media and
/// its place.
#[derive(Clone)]
pub struct Rows {
    store: Arc<dyn Store>,
    history: Arc<History>,
    tables: Option<Arc<ProjectingStore>>,
    aliases: Arc<Aliases>,
    notes: Arc<Notes>,
}

impl Rows {
    /// The rows of `store`, whose elements have no version kept.
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self::keeping(store, Arc::default())
    }

    /// The rows of `store`, the versions of its elements kept in `history`.
    pub(crate) fn keeping(store: Arc<dyn Store>, history: Arc<History>) -> Self {
        Self {
            store,
            history,
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

    /// The element `id` names as a media's place reads it — its type and its
    /// campaign — from the tables over a database, which hold the fixtures'
    /// elements and every one applied; from the engine's row over its
    /// memory. `None` when there is no such live element.
    ///
    /// # Errors
    ///
    /// [`harness_dataguard::Error::Store`] when the tables cannot be read;
    /// the store's.
    pub async fn place(&self, id: &str) -> Result<Option<upload_media::Place>> {
        let Some(tables) = &self.tables else {
            // The ids the server makes are lower case.
            let row = self
                .store
                .row(&format!("{ENTITY}{}", id.to_ascii_lowercase()))
                .await?;
            let text = |field: &str| row.get(field).and_then(Value::as_str).map(str::to_owned);
            return Ok(text("type")
                .zip(text("entityCampaignId"))
                .map(|(kind, campaign)| upload_media::Place { kind, campaign }));
        };
        tables.place(id).await.map_err(|e| {
            harness_dataguard::Error::Store(format!(
                "entity not read: {}",
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

    /// Whether the store holds the campaign `id` names, live — the engine's
    /// row, or the one its table holds (the Campaign aggregate's
    /// `ProjectPg::hydrate`) — over a database; over the engine's memory,
    /// which has no table, every campaign is held.
    ///
    /// # Errors
    ///
    /// The store's.
    pub async fn holds_campaign(&self, id: &str) -> Result<bool> {
        let Some(tables) = &self.tables else {
            return Ok(true);
        };
        tables.holds_campaign(id).await
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

    /// The media `media` names and the element `place` names, as a change of
    /// place (`media.setMediaPlace`) is judged against them: over a database
    /// the tables' rows, which hold the fixtures' and judge the place again,
    /// under the engine's as the commands queued on them will leave them — a
    /// lieu or a media created a moment ago is there, and a field the engine
    /// holds wins. `None` when there is no such media, and the place `None`
    /// when there is no such element — or none is named.
    ///
    /// # Errors
    ///
    /// [`harness_dataguard::Error::Store`] when the tables cannot be read;
    /// the store's.
    pub async fn placed(&self, media: &str, place: Option<&str>) -> Result<Option<Placing>> {
        let held = |row: BTreeMap<String, Value>| Some(row).filter(|row| !row.is_empty());
        let engine_media = held(self.projected(&format!("Media/{media}")).await?);
        let engine_place = match place {
            Some(place) => held(self.projected(&format!("{ENTITY}{place}")).await?),
            None => None,
        };
        let Some(tables) = &self.tables else {
            return Ok(engine_media.map(|media| (media, engine_place)));
        };
        let found = tables.placing(media, place).await.map_err(|e| {
            harness_dataguard::Error::Store(format!(
                "media not read: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            ))
        })?;
        let (table_media, table_place) = found.unzip();
        let over = |under: Option<BTreeMap<String, Value>>,
                    row: Option<BTreeMap<String, Value>>| {
            match (under, row) {
                (Some(mut under), Some(row)) => {
                    under.extend(row);
                    Some(under)
                }
                (under, row) => under.or(row),
            }
        };
        Ok(over(table_media, engine_media)
            .map(|media| (media, over(table_place.flatten(), engine_place))))
    }

    /// The versions of `element` a restoration can name, oldest first.
    pub fn versions(&self, element: &str) -> Vec<Kept> {
        self.history.of(element)
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

    /// Morlox's row as a save leaves it, numbered `number` with version `id`.
    fn saved(name: &str, id: &str, number: u64) -> Value {
        json!({
            "entityId": "e1000000-0000-4000-8000-000000000003",
            "entityCampaignId": "c1000000-0000-4000-8000-000000000001",
            "type": "pnj",
            "name": name,
            "alias": ["le gobelin"],
            "entityVersionId": id,
            "versionNumber": number,
            "source": "dm",
            "snapshot": {},
            "diff": [],
        })
    }

    #[tokio::test]
    async fn an_applied_save_carries_the_version_after_the_real_row_s_and_records_its_terms() {
        let inner = Arc::new(MemoryStore::default());
        let aliases = Arc::new(Aliases::default());
        let store = Versioning::new(inner.clone(), Some(aliases.clone()));
        // Numbered 5 when submitted, behind saves that never landed.
        store
            .apply(&write(saved("Morlox", "v1", 5), false), &result())
            .await
            .expect("applied");
        let row = inner.row_now(MORLOX);
        assert_eq!(row["versionNumber"], 1);
        assert_eq!(
            row["snapshot"],
            json!({ "type": "pnj", "name": "Morlox", "aliases": ["le gobelin"] })
        );
        assert_eq!(row["diff"].as_array().map(Vec::len), Some(3));
        let other = write(saved("Bandit", "v9", 1), false).values;
        assert_eq!(
            aliases.taken("bandit", &other),
            Some("le gobelin".to_owned())
        );

        store
            .apply(&write(saved("Morlox le Vil", "v2", 7), false), &result())
            .await
            .expect("applied");
        let row = inner.row_now(MORLOX);
        assert_eq!(row["versionNumber"], 2);
        assert_eq!(
            row["diff"],
            json!([{ "field": "name", "old": "Morlox", "new": "Morlox le Vil" }])
        );
    }

    #[tokio::test]
    async fn a_write_carrying_no_version_of_its_own_keeps_the_row_s_and_a_removal_frees_its_terms()
    {
        let inner = Arc::new(MemoryStore::default());
        let aliases = Arc::new(Aliases::default());
        let store = Versioning::new(inner.clone(), Some(aliases.clone()));
        store
            .apply(&write(saved("Morlox", "v1", 1), false), &result())
            .await
            .expect("applied");
        let before = inner.row_now(MORLOX);
        // The same version id: no new version is made of it.
        let mut same = saved("Morlox le Vil", "v1", 1);
        same["snapshot"] = before["snapshot"].clone();
        same["diff"] = before["diff"].clone();
        store
            .apply(&write(same, false), &result())
            .await
            .expect("applied");
        let row = inner.row_now(MORLOX);
        assert_eq!(row["versionNumber"], before["versionNumber"]);
        assert_eq!(row["snapshot"], before["snapshot"]);
        assert_eq!(row["name"], "Morlox le Vil");

        store
            .apply(&write(json!({}), true), &result())
            .await
            .expect("removed");
        let other = write(saved("Bandit", "v9", 1), false).values;
        assert_eq!(aliases.taken("bandit", &other), None);
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
        let store = Versioning::new(inner.clone(), None);
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
