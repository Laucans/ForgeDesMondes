//! The engine's writes, carried into the product's tables.
//!
//! The engine keeps a row per partition in its own table; the product reads
//! its tables (`crates/data/migrations/`). The AiSettings aggregate projects
//! its own writes, in the apply transaction (`aggregate_ai_settings`). A save
//! of an element (`campaign.saveEntity`) writes its partition, `Entity/<its
//! id>`, whole: [`ProjectingStore`] upserts its `entity` row and its sheet —
//! or drops the sheet of an element made a place or an event — makes its
//! `entity_alias` rows its aliases, and adds the version it carries
//! to `entity_version`, in one transaction — whose triggers judge the
//! element's campaign again. An analysis of notes (`ai.extractFromNotes`) writes a partition
//! per imported note, `NoteImport/<its id>`, and one per proposal: once the
//! note is applied, [`ProjectingStore`] adds its `imported_note` row and the
//! `note_proposal` row of every proposal of it applied, in one transaction —
//! whichever the engine applied first. A validation of a note's proposals
//! (`campaign.createEntitiesFromNote`) writes each proposal's partition
//! again, its check and the element it created: [`ProjectingStore`] writes
//! `note_proposal.checked` and `created_entity_id` — the element's once the
//! tables hold it, whichever the engine applied first. Every other method is
//! the inner store's.
//!
//! The projection runs after the engine's own transaction: when it fails,
//! the engine's row stays applied and the error is logged by its SQLSTATE
//! alone — never a value. [`ProjectingStore::repair`]
//! projects the row again at the next start: every
//! element whose version the tables do not hold yet — those again after each
//! apply of an element, so one the tables refused (a player they do not hold
//! yet) reaches them once they can. The tables' CHECKs, their foreign keys
//! and their triggers judge it a second time. An element's terms reach
//! [`Aliases`] once its projection is committed, and only then: the
//! tables' aliases are the ones `alias-unique-in-campaign` judges against.
//! So do its versions reach [`History`], as `entity_version` numbers them:
//! the ones a restoration names.

use std::collections::BTreeMap;
use std::sync::Arc;

use aggregate_entity::Aliases;
use async_trait::async_trait;
use harness_dataguard::Result;
use harness_dataguard::command::{CommandResult, Entry};
use harness_dataguard::postgres::PgStore;
use harness_dataguard::queue::Hold;
use harness_dataguard::store::{Collision, Store, Write};
use restore_entity_version::Kept;
use serde_json::Value;

use crate::history::History;
use crate::notes::Notes;
use crate::versioning::{ENTITY, NOTE_IMPORT, element_of};

/// An element's row, `$1` the engine's row as JSON text: its `entity` row
/// upserted by its id.
const UPSERT_ENTITY: &str = "INSERT INTO entity
        (id, campaign_id, type, name, description, notes, created_at, updated_at)
    SELECT (v ->> 'entityId')::uuid, (v ->> 'entityCampaignId')::uuid, v ->> 'type',
        v ->> 'name', v ->> 'description', v ->> 'notes',
        coalesce((v ->> 'entityCreatedAt')::timestamptz, now()),
        coalesce((v ->> 'updatedAt')::timestamptz, now())
    FROM (SELECT $1::text::jsonb AS v) saved
    ON CONFLICT (id) DO UPDATE SET
        campaign_id = EXCLUDED.campaign_id,
        type = EXCLUDED.type,
        name = EXCLUDED.name,
        description = EXCLUDED.description,
        notes = EXCLUDED.notes,
        updated_at = EXCLUDED.updated_at";

/// Its sheet, when the row holds one, upserted by the element's id.
const UPSERT_CHARACTER: &str = "INSERT INTO character
        (entity_id, player_id, initial_prompt, background, behavior,
         strength, dexterity, constitution, intelligence, wisdom, charisma,
         armor_class, hp_max, spells, actions, portrait_media_id, token_media_id,
         token_crop, token_color, dndbeyond_id, dndbeyond_synced_at)
    SELECT (v ->> 'characterEntityId')::uuid, (v ->> 'playerId')::uuid,
        v ->> 'initialPrompt', v ->> 'background', v ->> 'behavior',
        (v ->> 'strength')::numeric::smallint, (v ->> 'dexterity')::numeric::smallint,
        (v ->> 'constitution')::numeric::smallint, (v ->> 'intelligence')::numeric::smallint,
        (v ->> 'wisdom')::numeric::smallint, (v ->> 'charisma')::numeric::smallint,
        (v ->> 'armorClass')::numeric::smallint, (v ->> 'hpMax')::numeric::smallint,
        coalesce(nullif(v -> 'spells', 'null'), '[]'), coalesce(nullif(v -> 'actions', 'null'), '[]'),
        (v ->> 'portraitMediaId')::uuid, (v ->> 'tokenMediaId')::uuid,
        nullif(v -> 'tokenCrop', 'null'), v ->> 'tokenColor',
        v ->> 'dndbeyondId', (v ->> 'dndbeyondSyncedAt')::timestamptz
    FROM (SELECT $1::text::jsonb AS v) saved
    WHERE v ->> 'characterEntityId' IS NOT NULL
    ON CONFLICT (entity_id) DO UPDATE SET
        player_id = EXCLUDED.player_id,
        initial_prompt = EXCLUDED.initial_prompt,
        background = EXCLUDED.background,
        behavior = EXCLUDED.behavior,
        strength = EXCLUDED.strength,
        dexterity = EXCLUDED.dexterity,
        constitution = EXCLUDED.constitution,
        intelligence = EXCLUDED.intelligence,
        wisdom = EXCLUDED.wisdom,
        charisma = EXCLUDED.charisma,
        armor_class = EXCLUDED.armor_class,
        hp_max = EXCLUDED.hp_max,
        spells = EXCLUDED.spells,
        actions = EXCLUDED.actions,
        portrait_media_id = EXCLUDED.portrait_media_id,
        token_media_id = EXCLUDED.token_media_id,
        token_crop = EXCLUDED.token_crop,
        token_color = EXCLUDED.token_color,
        dndbeyond_id = EXCLUDED.dndbeyond_id,
        dndbeyond_synced_at = EXCLUDED.dndbeyond_synced_at";

/// Its sheet, when it holds none: a place or an event has no sheet.
const DROP_CHARACTER: &str = "DELETE FROM character
    WHERE entity_id = ($1::text::jsonb ->> 'entityId')::uuid
        AND $1::text::jsonb ->> 'characterEntityId' IS NULL";

/// The aliases it no longer has, gone.
const DROP_ALIASES: &str = "DELETE FROM entity_alias
    WHERE entity_id = ($1::text::jsonb ->> 'entityId')::uuid
        AND alias <> ALL (ARRAY(SELECT jsonb_array_elements_text(
            coalesce(nullif($1::text::jsonb -> 'alias', 'null'), '[]'))))";

/// The aliases it did not have yet, added.
const ADD_ALIASES: &str = "INSERT INTO entity_alias (entity_id, alias)
    SELECT DISTINCT (v ->> 'entityId')::uuid, given.term
    FROM (SELECT $1::text::jsonb AS v) saved,
        jsonb_array_elements_text(coalesce(nullif(v -> 'alias', 'null'), '[]')) AS given (term)
    WHERE NOT EXISTS (SELECT 1 FROM entity_alias a
        WHERE a.entity_id = (v ->> 'entityId')::uuid AND a.alias = given.term)";

/// The version the save carries, numbered after the element's last one in
/// the table — which a projection lost earlier may have left behind the
/// engine's.
const ADD_VERSION: &str = "INSERT INTO entity_version
        (id, entity_id, version_number, source, snapshot, diff, restored_from_version_id, created_at)
    SELECT (v ->> 'entityVersionId')::uuid, (v ->> 'entityId')::uuid,
        (SELECT coalesce(max(version_number), 0) + 1 FROM entity_version
            WHERE entity_id = (v ->> 'entityId')::uuid),
        v ->> 'source', v -> 'snapshot', v -> 'diff', (v ->> 'restoredFromVersionId')::uuid,
        coalesce((v ->> 'entityVersionCreatedAt')::timestamptz, now())
    FROM (SELECT $1::text::jsonb AS v) saved";

/// Every live element the engine holds whose version the tables do not: a
/// projection that failed after its apply, or was lost since. A row with no
/// version is none the tables could hold.
const UNPROJECTED: &str = "SELECT r.partition FROM dataguard_rows r
    WHERE r.partition LIKE 'Entity/%' AND NOT r.tombstoned
        AND r.fields ->> 'entityVersionId' IS NOT NULL
        AND NOT EXISTS (SELECT 1 FROM entity_version v
            WHERE v.id::text = lower(r.fields ->> 'entityVersionId'))
    ORDER BY r.partition";

/// Every element of the tables with its campaign, name and aliases, as the
/// engine's row holds them: what [`Aliases`] starts from.
const ELEMENTS: &str = "SELECT jsonb_build_object(
        'entityId', e.id, 'entityCampaignId', e.campaign_id, 'name', e.name,
        'alias', coalesce((SELECT jsonb_agg(a.alias) FROM entity_alias a WHERE a.entity_id = e.id), '[]')
    )::text
    FROM entity e";

/// The imported notes the engine holds and the tables do not — of the note
/// `$1` names, or of every one when it is null: an analysis whose note was
/// just applied, or whose projection was lost. A note of a campaign the
/// tables do not hold, or whose ids are no uuids, is none they could hold.
const ADD_NOTES: &str =
    "INSERT INTO imported_note (id, campaign_id, raw_text, created_at, analyzed_at)
    SELECT (r.fields ->> 'importedNoteId')::uuid, c.id, r.fields ->> 'rawText',
        coalesce((r.fields ->> 'createdAt')::timestamptz, now()),
        (r.fields ->> 'analyzedAt')::timestamptz
    FROM dataguard_rows r
    JOIN campaign c ON c.id::text = lower(r.fields ->> 'campaignId')
    WHERE r.partition LIKE 'NoteImport/%' AND NOT r.tombstoned
        AND ($1::text IS NULL OR lower(r.fields ->> 'importedNoteId') = lower($1::text))
        AND pg_input_is_valid(r.fields ->> 'importedNoteId', 'uuid')
        AND r.fields ->> 'rawText' IS NOT NULL
        AND pg_input_is_valid(coalesce(r.fields ->> 'createdAt', 'now'), 'timestamptz')
        AND pg_input_is_valid(coalesce(r.fields ->> 'analyzedAt', 'now'), 'timestamptz')
        AND NOT EXISTS (SELECT 1 FROM imported_note n
            WHERE n.id::text = lower(r.fields ->> 'importedNoteId'))";

/// The proposals the engine holds and the tables do not, of a note they hold
/// — the one `$1` names, or every one when it is null. The element a
/// proposal recognised and the tables no longer hold is forgotten, as its
/// relation's `onDelete nullify` would have.
const ADD_PROPOSALS: &str = "INSERT INTO note_proposal
        (id, note_id, type, name, excerpt, existing_entity_id, checked)
    SELECT (r.fields ->> 'noteProposalId')::uuid, n.id, r.fields ->> 'type',
        r.fields ->> 'name', coalesce(r.fields ->> 'excerpt', ''), e.id,
        coalesce((r.fields ->> 'checked')::boolean, e.id IS NULL)
    FROM dataguard_rows r
    JOIN imported_note n ON n.id::text = lower(r.fields ->> 'noteId')
    LEFT JOIN entity e ON e.id::text = lower(r.fields ->> 'existingEntityId')
    WHERE r.partition LIKE 'NoteImport/%' AND NOT r.tombstoned
        AND ($1::text IS NULL OR n.id::text = lower($1::text))
        AND pg_input_is_valid(r.fields ->> 'noteProposalId', 'uuid')
        AND r.fields ->> 'type' IN ('pj', 'pnj', 'lieu', 'evenement')
        AND r.fields ->> 'name' IS NOT NULL
        AND pg_input_is_valid(coalesce(r.fields ->> 'checked', 'true'), 'boolean')
        AND NOT EXISTS (SELECT 1 FROM note_proposal p
            WHERE p.id::text = lower(r.fields ->> 'noteProposalId'))
    ORDER BY r.partition";

/// The checked state and the created element the engine holds of each
/// proposal the tables hold — of the note `$1` names, or of every one when
/// it is null; of the proposals that created the element `$2` names, or of
/// every one when it is null — where they differ: a validation applied
/// (`campaign.createEntitiesFromNote`). An element the tables do not hold
/// yet is recorded once they do; one recorded is never replaced, nor is a
/// proposal that created one unchecked (`no-creation-before-validation`).
const STAMP_PROPOSALS: &str = "UPDATE note_proposal p
    SET checked = s.checked,
        created_entity_id = coalesce(e.id, p.created_entity_id)
    FROM (SELECT r.partition, lower(r.fields ->> 'createdEntityId') AS created,
            CASE r.fields ->> 'checked' WHEN 'true' THEN true WHEN 'false' THEN false END AS checked
        FROM dataguard_rows r
        WHERE r.partition LIKE 'NoteImport/%' AND NOT r.tombstoned) s
    LEFT JOIN entity e ON e.id::text = s.created
    WHERE s.partition = 'NoteImport/' || p.id::text AND s.checked IS NOT NULL
        AND ($1::text IS NULL OR p.note_id::text = lower($1::text))
        AND ($2::text IS NULL OR s.created = lower($2::text))
        AND (p.created_entity_id IS NULL OR e.id IS NULL OR e.id = p.created_entity_id)
        AND (s.checked OR coalesce(e.id, p.created_entity_id) IS NULL)
        AND (p.checked <> s.checked
            OR p.created_entity_id IS DISTINCT FROM coalesce(e.id, p.created_entity_id))";

/// Every proposal the engine holds, with its note: what [`Notes`] starts
/// from.
const PROPOSALS: &str =
    "SELECT r.fields ->> 'noteId', substr(r.partition, length('NoteImport/') + 1)
    FROM dataguard_rows r
    WHERE r.partition LIKE 'NoteImport/%' AND NOT r.tombstoned
        AND jsonb_typeof(r.fields -> 'noteId') = 'string'";

/// Whether the row of the partition `$1` names is the engine's tombstone.
const TOMBSTONED: &str =
    "SELECT EXISTS (SELECT 1 FROM dataguard_rows WHERE partition = $1 AND tombstoned)";

/// The versions the tables hold — the one `$1` names, or every one when it
/// is null — oldest first.
const VERSIONS: &str =
    "SELECT entity_id::text, id::text, version_number, snapshot::text, diff::text
    FROM entity_version
    WHERE $1::text IS NULL OR id = ($1::text)::uuid
    ORDER BY entity_id, version_number";

/// A [`PgStore`] whose applied `AiSettings/1` and `Entity/<id>` writes reach
/// the tables, keeping the elements' terms and versions as the tables hold
/// them.
pub struct ProjectingStore {
    inner: PgStore,
    aliases: Arc<Aliases>,
    history: Arc<History>,
}

impl ProjectingStore {
    /// The store over `inner`, keeping `aliases`.
    pub fn new(inner: PgStore, aliases: Arc<Aliases>) -> Self {
        Self {
            inner,
            aliases,
            history: Arc::default(),
        }
    }

    /// The same, keeping the elements' versions in `history`.
    #[must_use]
    pub fn keeping(self, history: Arc<History>) -> Self {
        Self { history, ..self }
    }

    /// Projects what the engine holds and the tables do not, logging a
    /// failure: at a start, so a projection that failed after its apply is
    /// made again.
    pub async fn repair(&self) {
        // --- Entity
        self.repair_elements().await;
        // --- NoteImport
        self.repair_notes(None).await;
        // ---
    }

    /// Adds the imported note `note` names — every one, `None` — and the
    /// proposals of it the engine holds, when the tables do not hold them
    /// yet, in one transaction, logging a failure: after each apply of a
    /// note or a proposal, and at a start. Tables not migrated yet hold none.
    async fn repair_notes(&self, note: Option<&str>) {
        let written = async {
            let mut client = self.inner.client().lock().await;
            let tx = client.transaction().await?;
            tx.execute(ADD_NOTES, &[&note]).await?;
            tx.execute(ADD_PROPOSALS, &[&note]).await?;
            tx.execute(STAMP_PROPOSALS, &[&note, &None::<&str>]).await?;
            tx.commit().await
        };
        if let Err(e) = written.await {
            let db = e.as_db_error();
            eprintln!(
                "projection: imported notes not projected: {} {}",
                db.map_or("-", |db| db.code().code()),
                db.and_then(|db| db.constraint()).unwrap_or_default()
            );
        }
    }

    /// Records `element` on the proposals that created it, once the tables
    /// hold it, logging a failure: after each apply of an element.
    async fn stamp_created(&self, element: &str) {
        let stamped = {
            let client = self.inner.client().lock().await;
            client
                .execute(STAMP_PROPOSALS, &[&None::<&str>, &Some(element)])
                .await
        };
        if let Err(e) = stamped {
            eprintln!(
                "projection: proposals not stamped: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            );
        }
    }

    /// Records every proposal the engine holds among its note's: at a start,
    /// so a validation unchecks the ones applied before.
    pub async fn notes(&self, notes: &Notes) {
        let rows = {
            let client = self.inner.client().lock().await;
            client.query(PROPOSALS, &[]).await
        };
        match rows {
            Ok(rows) => {
                for row in rows {
                    notes.record(row.get(0), row.get(1));
                }
            }
            Err(e) => eprintln!(
                "projection: proposals not read: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            ),
        }
    }

    /// Whether the engine deleted `partition`'s row: its tombstone.
    pub async fn tombstoned(
        &self,
        partition: &str,
    ) -> std::result::Result<bool, tokio_postgres::Error> {
        let client = self.inner.client().lock().await;
        let row = client.query_one(TOMBSTONED, &[&partition]).await?;
        Ok(row.get(0))
    }

    /// Projects every element the engine holds whose version the tables do
    /// not hold, recording its terms once they do, logging a failure: after
    /// each apply of an element, and at a start. Tables not migrated yet
    /// hold none.
    async fn repair_elements(&self) {
        let partitions = {
            let client = self.inner.client().lock().await;
            client.query(UNPROJECTED, &[]).await
        };
        let partitions = match partitions {
            Ok(rows) => rows,
            Err(e) => {
                eprintln!(
                    "projection: entity_version not read: {}",
                    e.as_db_error().map_or("-", |db| db.code().code())
                );
                return;
            }
        };
        for partition in partitions {
            let partition: String = partition.get(0);
            let row = match self.inner.row(&partition).await {
                Ok(row) => row,
                Err(e) => {
                    eprintln!("projection: {partition} not read: {e}");
                    continue;
                }
            };
            match self.project_element(&row).await {
                Ok(()) => {
                    if let Some(element) = partition.strip_prefix(ENTITY) {
                        self.aliases.record(element, &row);
                        let version = text(&row, "entityVersionId");
                        self.versions(version.map(|version| (element, version)))
                            .await;
                    }
                }
                Err(e) => {
                    let db = e.as_db_error();
                    eprintln!(
                        "projection: {partition} not projected: {} {}",
                        db.map_or("-", |db| db.code().code()),
                        db.and_then(|db| db.constraint()).unwrap_or_default()
                    );
                }
            }
        }
    }

    /// Records every element the tables hold in the terms it keeps: at a
    /// start, so the elements saved before are judged against. Tables not
    /// migrated yet hold none.
    pub async fn elements(&self) {
        let rows = {
            let client = self.inner.client().lock().await;
            client.query(ELEMENTS, &[]).await
        };
        match rows {
            Ok(rows) => {
                for row in rows {
                    let json: String = row.get(0);
                    let Ok(Value::Object(element)) = serde_json::from_str(&json) else {
                        continue;
                    };
                    let element: BTreeMap<String, Value> = element.into_iter().collect();
                    if let Some(id) = text(&element, "entityId") {
                        self.aliases.record(id, &element);
                    }
                }
            }
            Err(e) => eprintln!(
                "projection: entity not read: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            ),
        }
    }

    /// Keeps the versions the tables hold as they number them: of an element,
    /// the one just projected — `Some((element, version))` — and of every
    /// element at a start, `None`. Tables not migrated yet hold none.
    pub async fn versions(&self, projected: Option<(&str, &str)>) {
        let version = projected.map(|(_, version)| version);
        let rows = {
            let client = self.inner.client().lock().await;
            client.query(VERSIONS, &[&version]).await
        };
        let rows = match rows {
            Ok(rows) => rows,
            Err(e) => {
                eprintln!(
                    "projection: entity_version not read: {}",
                    e.as_db_error().map_or("-", |db| db.code().code())
                );
                return;
            }
        };
        let mut elements: BTreeMap<String, Vec<Kept>> = BTreeMap::new();
        for row in rows {
            let json = |i: usize| serde_json::from_str::<Value>(row.get::<_, &str>(i)).ok();
            let (Some(Value::Object(snapshot)), Some(Value::Array(diff))) = (json(3), json(4))
            else {
                continue;
            };
            let Ok(number) = u64::try_from(row.get::<_, i32>(2)) else {
                continue;
            };
            let kept = Kept {
                id: row.get(1),
                number,
                snapshot,
                diff,
            };
            match projected {
                Some((element, _)) => self.history.record(element, kept),
                None => elements.entry(row.get(0)).or_default().push(kept),
            }
        }
        for (element, versions) in elements {
            self.history.replace(&element, versions);
        }
    }

    /// Writes `row`'s element, sheet, aliases and version, in one
    /// transaction.
    async fn project_element(
        &self,
        row: &BTreeMap<String, Value>,
    ) -> std::result::Result<(), tokio_postgres::Error> {
        let json = Value::Object(row.clone().into_iter().collect()).to_string();
        let mut client = self.inner.client().lock().await;
        let tx = client.transaction().await?;
        for sql in [
            UPSERT_ENTITY,
            UPSERT_CHARACTER,
            DROP_CHARACTER,
            DROP_ALIASES,
            ADD_ALIASES,
            ADD_VERSION,
        ] {
            tx.execute(sql, &[&json]).await?;
        }
        tx.commit().await
    }
}

/// Whether `write` reaches the element's tables: a live write of an
/// `Entity/<id>` partition.
fn element(write: &Write) -> bool {
    !write.tombstone && element_of(write).is_some()
}

/// The imported note a live write of the NoteImport aggregate is of: the
/// note's own id, or the note a proposal is of.
fn noted(write: &Write) -> Option<&str> {
    if write.tombstone || !write.partition.starts_with(NOTE_IMPORT) {
        return None;
    }
    ["importedNoteId", "noteId"]
        .iter()
        .find_map(|field| write.values.get(*field).and_then(Value::as_str))
}

/// A field as text; `None` when absent, `null` or not a string.
fn text<'a>(row: &'a BTreeMap<String, Value>, field: &str) -> Option<&'a str> {
    row.get(field).and_then(Value::as_str)
}

#[async_trait]
impl Store for ProjectingStore {
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

    /// The inner store's apply, then each aggregate's projection — one
    /// block each, on its own partitions, deleted when the aggregate
    /// projects in the apply transaction (its `ProjectPg`).
    async fn apply(&self, write: &Write, result: &CommandResult) -> Result<u64> {
        let version = self.inner.apply(write, result).await?;
        // --- Entity
        if element(write) {
            self.repair_elements().await;
            if let Some(element) = element_of(write) {
                self.stamp_created(element).await;
            }
        } else if let Some(element) = element_of(write) {
            // A removal: tombstones are not projected, its terms and its
            // versions go.
            self.aliases.forget(element);
            self.history.forget(element);
        }
        // --- NoteImport
        if let Some(note) = noted(write) {
            self.repair_notes(Some(note)).await;
        }
        // ---
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

#[cfg(test)]
mod tests {
    //! Against the harness's PostgreSQL (`DATABASE_URL`), in a scratch
    //! database of its own.

    use harness_dataguard::api::Submission;
    use harness_dataguard::command::State;
    use harness_dataguard::guard::Caller;
    use harness_dataguard::memory::MemoryNotifier;
    use harness_dataguard::postgres::SystemClock;
    use serde_json::json;
    use tokio_postgres::{Client, NoTls};

    use super::*;
    use crate::{Engine, bound, plugins};

    const MIGRATION: &str = include_str!("../../../data/migrations/0001_ai_settings.sql");
    /// The settings row's partition: a write the tables of no aggregate
    /// projected here reach.
    const AI_SETTINGS: &str = "AiSettings/1";
    const CLOUD: &str = "a1000000-0000-4000-8000-000000000001";
    const HOME: &str = "a1000000-0000-4000-8000-000000000002";

    #[test]
    fn only_a_live_write_of_an_element_s_partition_reaches_its_tables() {
        let write = |partition: &str, tombstone| Write {
            partition: partition.to_string(),
            values: BTreeMap::new(),
            tombstone,
        };
        let morlox = "Entity/e1000000-0000-4000-8000-000000000003";
        assert!(element(&write(morlox, false)));
        assert!(!element(&write(morlox, true)));
        assert!(!element(&write(AI_SETTINGS, false)));
        assert!(!element(&write(AI_SETTINGS, true)));
    }

    #[test]
    fn a_live_write_of_a_note_or_a_proposal_names_its_note() {
        let write = |partition: &str, values: serde_json::Value, tombstone| Write {
            partition: partition.to_string(),
            values: serde_json::from_value(values).expect("an object"),
            tombstone,
        };
        let note = "a1000000-0000-4000-8000-000000000001";
        let noted_by = |values, tombstone| {
            noted(&write(&format!("NoteImport/{note}"), values, tombstone)).map(str::to_owned)
        };
        assert_eq!(
            noted_by(json!({ "importedNoteId": note }), false).as_deref(),
            Some(note)
        );
        assert_eq!(
            noted_by(json!({ "noteProposalId": CLOUD, "noteId": note }), false).as_deref(),
            Some(note)
        );
        assert_eq!(noted_by(json!({ "importedNoteId": note }), true), None);
        assert_eq!(noted_by(json!({ "checked": true }), false), None);
        assert_eq!(
            noted(&write(AI_SETTINGS, json!({ "noteId": note }), false)),
            None
        );
    }

    async fn connect(url: &str) -> Client {
        let (client, connection) = tokio_postgres::connect(url, NoTls)
            .await
            .expect("PostgreSQL");
        tokio::spawn(connection);
        client
    }

    /// `DATABASE_URL` naming the database `name` instead.
    fn naming(url: &str, name: &str) -> String {
        let (address, query) = url.split_once('?').unwrap_or((url, ""));
        let (server, _) = address.rsplit_once('/').expect("a URL naming its database");
        let query = if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        format!("{server}/{name}{query}")
    }

    /// What the DM sends ai.saveSettings to make the provider at `url`
    /// active: its `Prepare` makes it the state it writes.
    fn wire(url: &str) -> BTreeMap<String, Value> {
        let wire = json!({
            "mode": "api",
            "url": url,
            "key": "",
            "generationModel": "g",
            "summaryModel": "s",
        });
        wire.as_object()
            .expect("an object")
            .clone()
            .into_iter()
            .collect()
    }

    /// The `llm` port's fake, its home server answering its model list: what
    /// ai.saveSettings detects the server's API type through.
    fn home_server() -> harness_dataguard::plugins::Shared {
        let llm = ports_llm::fake::FakeLlm::new().with_endpoint(
            "http://192.168.1.40:4096",
            json!({ "object": "list", "data": [] }),
        );
        let mut shared = harness_dataguard::plugins::Shared::default();
        shared.insert(Arc::new(ports_llm::SharedLlm::new(llm)));
        shared
    }

    async fn submitted(
        engine: &Engine,
        payload: BTreeMap<String, Value>,
        key: Option<&str>,
        roles: &[&str],
    ) -> String {
        engine
            .submit(Submission {
                data_capability: "ai.saveSettings@1".to_string(),
                payload,
                target_id: Some("1".to_string()),
                based_on: None,
                idempotency_key: key.map(str::to_string),
                caller: Caller {
                    by: "dm".to_string(),
                    from: "AiSettings".to_string(),
                    permissions: Vec::new(),
                    roles: roles.iter().map(|r| (*r).to_string()).collect(),
                },
            })
            .await
            .expect("queued")
            .command_id
    }

    #[tokio::test]
    async fn every_method_but_apply_answers_as_the_inner_store() {
        let url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL");
        let name = format!("projection_test_{}", std::process::id());
        let admin = connect(&url).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let scratch = naming(&url, &name);
        let tables = connect(&scratch).await;
        tables.batch_execute(MIGRATION).await.expect("migrated");

        let projecting = Arc::new(ProjectingStore::new(
            PgStore::open(connect(&scratch).await)
                .await
                .expect("the store"),
            Arc::default(),
        ));
        // The same tables, read without the wrapper.
        let plain = PgStore::open(connect(&scratch).await)
            .await
            .expect("the store");
        let engine = bound(
            Engine::new(
                projecting.clone(),
                Arc::new(MemoryNotifier::default()),
                Arc::new(SystemClock),
            ),
            plugins(&Arc::default()),
            home_server(),
        )
        .expect("the product's manifests parse");
        engine.load().await.expect("an empty queue");

        let approver = ["role:approver"];
        let keyed = submitted(
            &engine,
            wire("https://api.anthropic.com/v1"),
            Some("k-1"),
            &approver,
        )
        .await;
        engine.run_once().await.expect("applied");
        submitted(&engine, wire("http://192.168.1.40:4096"), None, &approver).await;
        engine.run_once().await.expect("applied");
        let refused = submitted(
            &engine,
            wire("https://inconnu.example.org"),
            None,
            &approver,
        )
        .await;
        let settled = engine.run_once().await.expect("rejected");
        assert_eq!(settled[0].status, State::Rejected, "{settled:?}");
        // Without the approver's role: it waits, pending, at the head.
        let waiting = submitted(&engine, wire("https://api.anthropic.com/v1"), None, &[]).await;
        engine
            .hold("AiSettings/ailleurs", "dm", None)
            .await
            .expect("a hold");
        tables
            .batch_execute(
                "INSERT INTO dataguard_links (relation, partition, rows) \
                 VALUES ('AiSettings.activeProviderId', 'AiSettings/lié', 2)",
            )
            .await
            .expect("a link");

        let pending = projecting.pending().await.expect("pending");
        let ids: Vec<&str> = pending.iter().map(|e| e.command.as_str()).collect();
        assert_eq!(ids, [waiting.as_str()]);
        let plain_ids: Vec<String> = plain
            .pending()
            .await
            .expect("pending")
            .into_iter()
            .map(|e| e.command)
            .collect();
        assert_eq!(plain_ids, [waiting]);

        let holds = projecting.holds().await.expect("holds");
        assert_eq!(holds.len(), 1, "{holds:?}");
        assert_eq!(holds, plain.holds().await.expect("holds"));

        assert_eq!(projecting.data_version().await.expect("a version"), 2);
        assert_eq!(plain.data_version().await.expect("a version"), 2);

        let row = projecting.row(AI_SETTINGS).await.expect("the row");
        assert_eq!(row["aiProviderId"], HOME);
        assert_eq!(row, plain.row(AI_SETTINGS).await.expect("the row"));

        let collision = projecting
            .collision(&pending[0])
            .await
            .expect("a collision");
        assert_eq!(collision.unique_key.as_deref(), Some(AI_SETTINGS));
        assert_eq!(
            collision,
            plain.collision(&pending[0]).await.expect("a collision")
        );

        let incoming = projecting
            .incoming("AiSettings/lié")
            .await
            .expect("incoming");
        assert_eq!(incoming, [("AiSettings.activeProviderId".to_string(), 2)]);
        assert_eq!(
            incoming,
            plain.incoming("AiSettings/lié").await.expect("incoming")
        );

        for command in [&keyed, &refused] {
            let result = projecting.result(command).await.expect("a result");
            assert!(result.is_some(), "{command} journalled");
            assert_eq!(result, plain.result(command).await.expect("a result"));
        }
        assert_eq!(
            projecting
                .result(&refused)
                .await
                .expect("a result")
                .map(|r| r.status),
            Some(State::Rejected)
        );

        let found = projecting
            .by_idempotency_key("ai.saveSettings", "k-1")
            .await
            .expect("a key");
        assert_eq!(found.as_deref(), Some(keyed.as_str()));
        assert_eq!(
            found,
            plain
                .by_idempotency_key("ai.saveSettings", "k-1")
                .await
                .expect("a key")
        );

        drop((engine, projecting, plain, tables));
        admin
            .batch_execute(&format!("DROP DATABASE {name} WITH (FORCE)"))
            .await
            .expect("the scratch database dropped");
    }
}
