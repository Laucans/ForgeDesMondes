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
//! projection keeps them, once the tables hold the write. Over either, it
//! keeps [`Scenes`] — each session's campaign, its scenes and their ranks,
//! as every write applied leaves them — and settles a new scene's rank
//! there: the one after its session's last, whatever was queued beside it
//! (`scene-order-contiguous`), which a reorder of the session's scenes is
//! judged against too. Every other method is the inner store's.
//! [`Rows`] reads what a link of a place to a scene is judged against
//! (`session.addPlaceToSession`), and the places of a new scene alike, and
//! keeps their verdict in [`Links`].

use std::collections::BTreeMap;
use std::sync::Arc;

use add_place_to_session::{Asked, PLACE, POSITION, SCENE, Scene, link_id, link_of};
use add_scene::{LINKED, POSITION as SCENE_POSITION, SESSION as SCENE_SESSION};
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
use crate::scene_places::{Facts, Links};
use crate::scenes::Scenes;

/// The partitions of the Entity aggregate: `Entity/<element id>`.
pub const ENTITY: &str = "Entity/";

/// The partitions of the Session aggregate: `Session/<session id>`, and a
/// scene's, `Session/<scene id>`.
pub const SESSION: &str = "Session/";

/// The partitions of the Campaign aggregate: `Campaign/<campaign id>`.
const CAMPAIGN: &str = "Campaign/";

/// A oneshot's creation: the id of the single session it writes on its
/// campaign's partition.
const ONESHOT_SESSION: &str = "sessionId";

/// The element a write is of, when it is a write of the Entity aggregate.
pub fn element_of(write: &Write) -> Option<&str> {
    write.partition.strip_prefix(ENTITY)
}

/// A store whose applied Entity writes carry their settled version, and a
/// new scene its settled rank.
pub struct Versioning {
    inner: Arc<dyn Store>,
    aliases: Option<Arc<Aliases>>,
    history: Option<Arc<History>>,
    scenes: Arc<Scenes>,
}

impl Versioning {
    /// The store over `inner`, keeping `aliases` — `None` when `inner` keeps
    /// them itself, after its tables.
    pub fn new(inner: Arc<dyn Store>, aliases: Option<Arc<Aliases>>) -> Self {
        Self {
            inner,
            aliases,
            history: None,
            scenes: Arc::default(),
        }
    }

    /// The same, keeping `history` too — as `aliases`, when `inner` does not.
    #[must_use]
    pub fn keeping(self, history: Option<Arc<History>>) -> Self {
        Self { history, ..self }
    }

    /// The same, keeping the sessions and scenes in `scenes`.
    #[must_use]
    pub fn following(self, scenes: Arc<Scenes>) -> Self {
        Self { scenes, ..self }
    }

    /// Applies a write of any aggregate but Entity: a new scene ranked after
    /// its session's last, and what a session's partition holds — the whole
    /// row a write carries — recorded in [`Scenes`] once applied, as is the
    /// session a oneshot's creation writes on its campaign's partition.
    async fn apply_other(&self, write: &Write, result: &CommandResult) -> Result<u64> {
        let Some(id) = write.partition.strip_prefix(SESSION) else {
            let version = self.inner.apply(write, result).await?;
            // A oneshot's session is written on its campaign's partition.
            if write.partition.starts_with(CAMPAIGN)
                && !write.tombstone
                && let Some(session) = write.values.get(ONESHOT_SESSION).and_then(Value::as_str)
            {
                self.scenes.record("Session", session, &write.values);
            }
            return Ok(version);
        };
        if write.tombstone {
            let version = self.inner.apply(write, result).await?;
            self.scenes.forget(id);
            return Ok(version);
        }
        let mut settled = None;
        if let Some(session) = write.values.get(SCENE_SESSION).and_then(Value::as_str)
            && !self
                .inner
                .row(&write.partition)
                .await?
                .contains_key(SCENE_SESSION)
        {
            let mut values = write.values.clone();
            let rank = self.scenes.next_position(session, id);
            values.insert(SCENE_POSITION.into(), rank.into());
            settled = Some(Write {
                partition: write.partition.clone(),
                values,
                tombstone: false,
            });
        }
        let settled = settled.as_ref().unwrap_or(write);
        let version = self.inner.apply(settled, result).await?;
        self.scenes.record("Session", id, &settled.values);
        Ok(version)
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
            return self.apply_other(write, result).await;
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
    scenes: Arc<Scenes>,
    links: Arc<Links>,
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
            scenes: Arc::default(),
            links: Arc::default(),
        }
    }

    /// The same, a campaign's archive read from `tables`.
    pub(crate) fn reading(self, tables: Option<Arc<ProjectingStore>>) -> Self {
        Self { tables, ..self }
    }

    /// The same, the sessions and scenes kept in `scenes`.
    pub(crate) fn following(self, scenes: Arc<Scenes>) -> Self {
        Self { scenes, ..self }
    }

    /// Takes what the tables hold of `session` — its campaign, its scenes —
    /// into where a new scene of it goes: the fixtures' rows, which no
    /// command wrote. Over the engine's memory, which has no table, nothing.
    ///
    /// # Errors
    ///
    /// [`harness_dataguard::Error::Store`] when the tables cannot be read.
    pub async fn refresh_session(&self, session: &str) -> Result<()> {
        let Some(tables) = &self.tables else {
            return Ok(());
        };
        let facts = tables.scene_facts(Some(session)).await.map_err(|e| {
            harness_dataguard::Error::Store(format!(
                "session not read: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            ))
        })?;
        for (aggregate, id, row) in facts {
            self.scenes.record(&aggregate, &id, &row);
        }
        Ok(())
    }

    /// Whether a command of `data_capability` under `key` was submitted
    /// already: one the engine replays. No key, none.
    ///
    /// # Errors
    ///
    /// The store's.
    pub async fn replayed(&self, data_capability: &str, key: Option<&str>) -> Result<bool> {
        let Some(key) = key else {
            return Ok(false);
        };
        Ok(self
            .store
            .by_idempotency_key(data_capability, key)
            .await?
            .is_some())
    }

    /// Whether `session` is one the engine or the tables hold.
    pub fn knows_session(&self, session: &str) -> bool {
        self.scenes.campaign_of(session).is_some()
    }

    /// The rank after the last of `session`'s scenes besides `scene`.
    pub fn next_scene_position(&self, session: &str, scene: &str) -> u64 {
        self.scenes.next_position(session, scene)
    }

    /// Takes what the tables and the engine's rows hold of `session`'s
    /// scenes into what a reorder of them is judged against: the fixtures'
    /// scenes, which no command wrote. Over the engine's memory, which has
    /// no table, nothing: its scenes are the ones it applied.
    ///
    /// # Errors
    ///
    /// [`harness_dataguard::Error::Store`] when the tables cannot be read.
    pub async fn refresh_scenes(&self, session: &str) -> Result<()> {
        let Some(tables) = &self.tables else {
            return Ok(());
        };
        let held = tables.scenes_of(Some(session)).await.map_err(|e| {
            harness_dataguard::Error::Store(format!(
                "scenes not read: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            ))
        })?;
        self.scenes.take(&held);
        Ok(())
    }

    /// The same, the verdicts of the links of places to scenes kept in
    /// `links`, which the engine's judge reads.
    pub(crate) fn linking(self, links: Arc<Links>) -> Self {
        Self { links, ..self }
    }

    /// What the link `asked` asks for is judged against: what the tables hold
    /// of its session, its scene — the one named, else the session's first —
    /// and its place, and where they hold nothing (over the engine's memory,
    /// always) what the engine's rows say, the commands queued on them
    /// included. The scene links the place already when this very link is
    /// applied or queued, and the places queued for it besides.
    ///
    /// # Errors
    ///
    /// [`harness_dataguard::Error::Store`] when the tables cannot be read; the
    /// store's.
    pub(crate) async fn place_facts(&self, asked: &Asked) -> Result<Facts> {
        let mut facts = match &self.tables {
            Some(tables) => tables.place_facts(asked).await.map_err(|e| {
                harness_dataguard::Error::Store(format!(
                    "session not read: {}",
                    e.as_db_error().map_or("-", |db| db.code().code())
                ))
            })?,
            None => Facts::default(),
        };
        let pending = self.store.pending().await?;
        let text = |row: &BTreeMap<String, Value>, field: &str| {
            row.get(field).and_then(Value::as_str).map(str::to_owned)
        };
        if facts.session_campaign.is_none() {
            let session = self
                .folded(&format!("{SESSION}{}", asked.session), &pending)
                .await?;
            facts.session_campaign = text(&session, "sessionCampaignId");
        }
        if facts.scene.is_none() {
            facts.scene = asked.scene.clone().map(|id| Scene {
                id,
                links: BTreeMap::new(),
            });
        }
        if let Some(scene) = &mut facts.scene {
            if facts.scene_session.is_none() {
                let row = self
                    .folded(&format!("{SESSION}{}", scene.id), &pending)
                    .await?;
                facts.scene_session = text(&row, "sceneSessionId");
            }
            let link = link_id(&asked.session, &scene.id, &asked.place);
            let row = self.folded(&format!("{SESSION}{link}"), &pending).await?;
            if let Some(position) = row.get(POSITION).and_then(Value::as_u64) {
                scene.links.entry(asked.place.clone()).or_insert(position);
            }
            // The places queued for the same scene, not applied yet.
            for entry in pending
                .iter()
                .filter(|entry| add_place_to_session::names(&entry.data_capability))
            {
                let payload = &entry.payload;
                let (Some(of), Some(place), Some(position)) = (
                    payload.get(SCENE).and_then(Value::as_str),
                    payload.get(PLACE).and_then(Value::as_str),
                    payload.get(POSITION).and_then(Value::as_u64),
                ) else {
                    continue;
                };
                if of.eq_ignore_ascii_case(&scene.id) {
                    scene
                        .links
                        .entry(place.to_ascii_lowercase())
                        .or_insert(position);
                }
            }
        }
        if facts.place_campaign.is_none() {
            let place = self
                .folded(&format!("{ENTITY}{}", asked.place), &pending)
                .await?;
            facts.place_campaign = text(&place, "entityCampaignId");
            facts.place_type = facts.place_type.or_else(|| text(&place, "type"));
        }
        Ok(facts)
    }

    /// Keeps the verdict of the link `id` names: whether its scene is its
    /// session's and its place of its campaign.
    pub(crate) fn judged(&self, id: &str, same_campaign: bool) {
        self.links.judged(id, same_campaign);
    }

    /// Keeps the verdict on the places the new scene `scene` of `session`
    /// links: whether each is an element of the session's campaign, as a
    /// place put in a scene is judged ([`Self::place_facts`]) — the session's
    /// campaign the one [`Scenes`] knows when the session's own row says none
    /// (a oneshot's, written on its campaign's partition).
    ///
    /// # Errors
    ///
    /// As [`Self::place_facts`].
    pub(crate) async fn judge_scene(
        &self,
        scene: &str,
        session: &str,
        places: &[String],
    ) -> Result<()> {
        let mut same = true;
        for place in places {
            let asked = Asked {
                session: session.to_owned(),
                scene: None,
                place: place.clone(),
            };
            let mut facts = self.place_facts(&asked).await?;
            if facts.session_campaign.is_none() {
                facts.session_campaign = self.scenes.campaign_of(session);
            }
            same &= facts.place_of_campaign();
        }
        self.judged(scene, same);
        Ok(())
    }

    /// Judges again every link queued, and every new scene's places: at a
    /// start, before the applier runs, so one queued before is applied as it
    /// was judged when submitted.
    ///
    /// # Errors
    ///
    /// As [`Self::place_facts`].
    pub(crate) async fn rejudge_links(&self) -> Result<()> {
        for entry in self.store.pending().await? {
            if add_scene::names(&entry.data_capability) {
                let scene = entry.partition.strip_prefix(SESSION).unwrap_or_default();
                let session = entry.payload.get(SCENE_SESSION).and_then(Value::as_str);
                let places: Vec<String> = match entry.payload.get(LINKED) {
                    Some(Value::Array(ids)) => ids
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect(),
                    _ => Vec::new(),
                };
                self.judge_scene(scene, session.unwrap_or_default(), &places)
                    .await?;
                continue;
            }
            if !add_place_to_session::names(&entry.data_capability) {
                continue;
            }
            let Some(id) = entry.partition.strip_prefix(SESSION) else {
                continue;
            };
            let Some((session, scene, place)) = link_of(id) else {
                continue;
            };
            let asked = Asked {
                session: session.to_owned(),
                scene: Some(scene.to_owned()),
                place: place.to_owned(),
            };
            let facts = self.place_facts(&asked).await?;
            self.judged(id, facts.same_campaign(session));
        }
        Ok(())
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
        let pending = self.store.pending().await?;
        self.folded(partition, &pending).await
    }

    /// `partition`'s row as the commands of `pending` queued on it will leave
    /// it: the row applied, then each one's payload, in queue order.
    async fn folded(&self, partition: &str, pending: &[Entry]) -> Result<BTreeMap<String, Value>> {
        let mut row = self.store.row(partition).await?;
        let mut ahead: Vec<&Entry> = pending
            .iter()
            .filter(|entry| entry.partition == partition)
            .collect();
        ahead.sort_by_key(|entry| entry.position);
        for entry in ahead {
            row.extend(entry.payload.clone());
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
