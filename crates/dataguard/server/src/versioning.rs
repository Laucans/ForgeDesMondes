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
use harness_dataguard::command::{CommandResult, Effect, Entry};
use harness_dataguard::guard;
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
/// and — over a database — what its tables hold of a campaign, and of what a
/// creation of an instance names.
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

    /// What the tables hold of the session, the scene and the battlemap a
    /// creation of an instance names (`table.createBattlemapInstance`):
    /// nothing over the engine's memory, which has no table.
    ///
    /// # Errors
    ///
    /// [`harness_dataguard::Error::Store`] when the tables cannot be read.
    pub async fn placement(
        &self,
        session: &str,
        scene: &str,
        battlemap: &str,
    ) -> Result<create_battlemap_instance::Placement> {
        let Some(tables) = &self.tables else {
            return Ok(create_battlemap_instance::Placement::default());
        };
        tables
            .placement(session, scene, battlemap)
            .await
            .map_err(|e| {
                harness_dataguard::Error::Store(format!(
                    "placement not read: {}",
                    e.as_db_error().map_or("-", |db| db.code().code())
                ))
            })
    }

    /// What the tables hold of the instance and the character a drop of
    /// tokens names (`table.placeTokens`), with the tokens of that character
    /// queued on the instance and not applied yet: theirs are taken too.
    /// Over the engine's memory, which has no table, no instance.
    ///
    /// # Errors
    ///
    /// [`harness_dataguard::Error::Store`] when the tables cannot be read;
    /// the store's.
    pub async fn roster(&self, instance: &str, character: &str) -> Result<place_tokens::Roster> {
        let Some(tables) = &self.tables else {
            return Ok(place_tokens::Roster::default());
        };
        let named = |entry: &Entry, field: &str, id: &str| {
            entry
                .payload
                .get(field)
                .and_then(Value::as_str)
                .is_some_and(|value| value.eq_ignore_ascii_case(id))
        };
        // The queue before the tables: a token applied between the two reads
        // is then counted twice, which takes no number nor PC more, rather
        // than missed by both.
        let queued: Vec<Entry> = self
            .store
            .pending()
            .await?
            .into_iter()
            .filter(|entry| {
                named(entry, "tokenInstanceId", instance) && named(entry, "characterId", character)
            })
            .collect();
        let mut roster = tables.roster(instance, character).await.map_err(|e| {
            harness_dataguard::Error::Store(format!(
                "roster not read: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            ))
        })?;
        roster.placed += queued.len();
        roster.taken.extend(
            queued
                .iter()
                .filter_map(|entry| entry.payload.get("number").and_then(Value::as_i64)),
        );
        Ok(roster)
    }

    /// What the tables hold of the instance a start of its combat names
    /// (`table.startCombat`), with the tokens queued on it and not applied
    /// yet: they join too, and a token whose removal is queued does not. A
    /// combat of the instance — in the tables, in the
    /// engine's rows, or queued — is open while its row, as the commands
    /// queued on it will leave it, has no end. Over the engine's memory,
    /// which has no table, no instance.
    ///
    /// # Errors
    ///
    /// [`harness_dataguard::Error::Store`] when the tables cannot be read;
    /// the store's.
    pub async fn lineup(&self, instance: &str) -> Result<start_combat::Lineup> {
        let Some(tables) = &self.tables else {
            return Ok(start_combat::Lineup::default());
        };
        let text = |entry: &Entry, field: &str| {
            entry
                .payload
                .get(field)
                .and_then(Value::as_str)
                .map(str::to_ascii_lowercase)
        };
        let of_instance =
            |entry: &Entry, field: &str| text(entry, field).is_some_and(|id| id == instance);
        // The queue before the tables, as a drop's roster reads them: a
        // token applied between the two reads is then read twice, which the
        // tables' read makes one, rather than missed by both.
        let pending = self.store.pending().await?;
        let queued: Vec<(String, String)> = pending
            .iter()
            .filter(|entry| of_instance(entry, "tokenInstanceId"))
            .filter_map(|entry| Some((text(entry, "tokenId")?, text(entry, "characterId")?)))
            .collect();
        let mut combats: Vec<String> = pending
            .iter()
            .filter(|entry| of_instance(entry, "combatInstanceId"))
            .filter_map(|entry| text(entry, "combatId"))
            .collect();
        let (mut lineup, held) = tables.lineup(instance, &queued).await.map_err(|e| {
            harness_dataguard::Error::Store(format!(
                "lineup not read: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            ))
        })?;
        // A removal is the tombstone of the token's partition.
        let removed: Vec<&str> = pending
            .iter()
            .filter(|entry| entry.effect == Effect::Delete)
            .filter_map(|entry| entry.partition.strip_prefix("BattlemapInstance/"))
            .collect();
        lineup.tokens.retain(|p| {
            !removed
                .iter()
                .any(|gone| gone.eq_ignore_ascii_case(&p.token))
        });
        combats.extend(held);
        combats.sort_unstable();
        combats.dedup();
        for combat in combats {
            let row = self.projected(&start_combat::partition(&combat)).await?;
            if row.get("endedAt").is_none_or(Value::is_null) {
                lineup.open = true;
                break;
            }
        }
        Ok(lineup)
    }

    /// Whether the engine holds a command of `data_capability` under the
    /// idempotency key `key`: one it replays.
    ///
    /// # Errors
    ///
    /// The store's.
    pub async fn replays(&self, data_capability: &str, key: &str) -> Result<bool> {
        Ok(self
            .store
            .by_idempotency_key(data_capability, key)
            .await?
            .is_some())
    }

    /// The versions of `element` a restoration can name, oldest first.
    pub fn versions(&self, element: &str) -> Vec<Kept> {
        self.history.of(element)
    }

    /// `partition`'s row as the commands queued on it will leave it: the row
    /// applied, then each pending command's payload, in queue order — a
    /// relative command's numbers added to the row's, as the engine composes
    /// them (`guard::projected`, `table.changeHitPoints`). The row and the
    /// queue are read while the applier is still ([`APPLYING`]): it writes a
    /// row before it marks its command applied, and a command read in both
    /// would be counted twice.
    ///
    /// # Errors
    ///
    /// The store's.
    pub async fn projected(&self, partition: &str) -> Result<BTreeMap<String, Value>> {
        let (row, ahead) = self.ahead(partition).await?;
        Ok(ahead
            .iter()
            .fold(row, |row, entry| guard::projected(&row, entry)))
    }

    /// A token's row as [`Rows::projected`] reads it, and none at all once
    /// its removal is queued (`table.removeToken`): no command is judged
    /// against it to land after its tombstone, which a write lifts. The
    /// token's commands alone read it so — and a participant's removal its
    /// entry's (`table.removeParticipant`); every other aggregate's row reads
    /// through a deletion as ever.
    ///
    /// # Errors
    ///
    /// The store's.
    pub async fn standing(&self, partition: &str) -> Result<BTreeMap<String, Value>> {
        let (row, ahead) = self.ahead(partition).await?;
        if ahead.iter().any(|entry| entry.effect == Effect::Delete) {
            return Ok(BTreeMap::new());
        }
        Ok(ahead
            .iter()
            .fold(row, |row, entry| guard::projected(&row, entry)))
    }

    /// `partition`'s row applied and its pending commands in queue order,
    /// read while the applier is still ([`APPLYING`]).
    async fn ahead(&self, partition: &str) -> Result<(BTreeMap<String, Value>, Vec<Entry>)> {
        let applying = APPLYING.lock().await;
        let row = self.store.row(partition).await?;
        let mut ahead: Vec<Entry> = self
            .store
            .pending()
            .await?
            .into_iter()
            .filter(|entry| entry.partition == partition)
            .collect();
        drop(applying);
        ahead.sort_by_key(|entry| entry.position);
        Ok((row, ahead))
    }
}

/// Held by the applier for each of its runs ([`crate::applier`]), and by
/// [`Rows::projected`] while it reads a row and the queue: a run writes a
/// command's row, then marks it applied, and the two reads never fall
/// between.
pub(crate) static APPLYING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

    #[tokio::test]
    async fn a_row_whose_deletion_is_queued_still_stands_projected_and_none_for_a_token() {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
        store
            .apply(&write(json!({ "name": "Morlox" }), false), &result())
            .await
            .expect("applied");
        let mut removal = pending("c_2", MORLOX, 2, json!({}));
        removal.effect = Effect::Delete;
        for entry in [
            removal,
            pending("c_3", MORLOX, 3, json!({ "notes": "late" })),
        ] {
            store.save(&entry).await.expect("saved");
        }
        let rows = Rows::new(store);
        let row = rows.projected(MORLOX).await.expect("projected");
        assert_eq!(row["name"], "Morlox");
        assert_eq!(row["notes"], "late");
        assert_eq!(
            rows.standing(MORLOX).await.expect("standing"),
            BTreeMap::new()
        );
    }

    #[tokio::test]
    async fn a_token_standing_with_no_deletion_queued_is_its_row_projected() {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
        store
            .apply(&write(json!({ "name": "Morlox" }), false), &result())
            .await
            .expect("applied");
        store
            .save(&pending("c_2", MORLOX, 2, json!({ "notes": "queued" })))
            .await
            .expect("saved");
        let row = Rows::new(store).standing(MORLOX).await.expect("standing");
        assert_eq!(row["name"], "Morlox");
        assert_eq!(row["notes"], "queued");
    }

    #[tokio::test]
    async fn a_row_projected_mid_run_of_the_applier_waits_for_it_and_counts_its_blow_once() {
        const TOKEN: &str = "BattlemapInstance/70000000-0000-4000-8000-000000000002";
        let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
        let hp = |current: i64| Write {
            partition: TOKEN.to_owned(),
            values: BTreeMap::from([("hpCurrent".to_owned(), json!(current))]),
            tombstone: false,
        };
        store.apply(&hp(10), &result()).await.expect("dropped");
        let mut blow = pending("c_2", TOKEN, 2, json!({ "hpCurrent": -6 }));
        blow.mode = harness_dataguard::command::Mode::Relative;
        store.save(&blow).await.expect("queued");
        // The applier, between the write of the blow's row and its mark.
        let applying = APPLYING.lock().await;
        store.apply(&hp(4), &result()).await.expect("written");
        let rows = Rows::new(store.clone());
        let read = tokio::spawn(async move { rows.projected(TOKEN).await });
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(!read.is_finished(), "read while the applier ran");
        blow.state = State::Applied;
        store.save(&blow).await.expect("marked");
        drop(applying);
        let row = read.await.expect("joined").expect("projected");
        // 4, not 4 - 6: a second blow cut against -2 would heal.
        assert_eq!(row["hpCurrent"], 4);
    }
}
