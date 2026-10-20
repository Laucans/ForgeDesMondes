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

use async_trait::async_trait;
use harness_dataguard::Result;
use harness_dataguard::command::{CommandResult, Entry};
use harness_dataguard::queue::Hold;
use harness_dataguard::store::{Collision, Store, Write};
use restore_entity_version::Kept;
use save_entity::{stamp, versioned};
use serde_json::Value;

use crate::aliases::Aliases;
use crate::history::History;
use crate::projection::ProjectingStore;

/// The partitions of the Entity aggregate: `Entity/<element id>`.
pub const ENTITY: &str = "Entity/";

/// The element a write is of, when it is a write of the Entity aggregate.
pub fn element_of(write: &Write) -> Option<&str> {
    write.partition.strip_prefix(ENTITY)
}

/// A store whose applied Entity writes carry their settled version.
pub struct Versioning {
    inner: Arc<dyn Store>,
    aliases: Option<Arc<Aliases>>,
    history: Option<Arc<History>>,
}

impl Versioning {
    /// The store over `inner`, keeping `aliases` — `None` when `inner` keeps
    /// them itself, after its tables.
    pub fn new(inner: Arc<dyn Store>, aliases: Option<Arc<Aliases>>) -> Self {
        Self {
            inner,
            aliases,
            history: None,
        }
    }

    /// The same, keeping `history` too — as `aliases`, when `inner` does not.
    #[must_use]
    pub fn keeping(self, history: Option<Arc<History>>) -> Self {
        Self { history, ..self }
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
            return self.inner.apply(write, result).await;
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
/// and — over a database — what its tables hold of a campaign.
#[derive(Clone)]
pub struct Rows {
    store: Arc<dyn Store>,
    history: Arc<History>,
    tables: Option<Arc<ProjectingStore>>,
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
        }
    }

    /// The same, a campaign's archive read from `tables`.
    pub(crate) fn reading(self, tables: Option<Arc<ProjectingStore>>) -> Self {
        Self { tables, ..self }
    }

    /// What the tables hold of the campaign `id` names, as an archive holds
    /// it (`campaign.exportCampaign`): `None` when they hold no such
    /// campaign — and over the engine's memory, which has no table.
    ///
    /// # Errors
    ///
    /// [`harness_dataguard::Error::Store`] when the tables cannot be read.
    pub async fn archived(&self, id: &str) -> Result<Option<Value>> {
        let Some(tables) = &self.tables else {
            return Ok(None);
        };
        tables.archived(id).await.map_err(|e| {
            harness_dataguard::Error::Store(format!(
                "campaign not read: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            ))
        })
    }

    /// The live session of the campaign the uuid `campaign` names, a return to its
    /// cover writes the diffusion row of (`live.showCover`): `None` when it
    /// has none — and over the engine's memory, which has no table.
    ///
    /// # Errors
    ///
    /// [`harness_dataguard::Error::Store`] when the tables cannot be read.
    pub async fn live_session(&self, campaign: &str) -> Result<Option<String>> {
        let Some(tables) = &self.tables else {
            return Ok(None);
        };
        tables.live_session(campaign).await.map_err(|e| {
            harness_dataguard::Error::Store(format!(
                "session not read: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            ))
        })
    }

    /// The versions of `element` a restoration can name, oldest first.
    pub fn versions(&self, element: &str) -> Vec<Kept> {
        self.history.of(element)
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
