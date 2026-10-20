//! The engine's writes, carried into the product's tables.
//!
//! The engine keeps a row per partition in its own table; the product reads
//! `ai_provider` and `ai_settings` (`crates/data/migrations/`). A save of the
//! AI settings is one command on `AiSettings/1` carrying the active provider
//! whole (`ai.saveSettings`): once the engine applied it, [`ProjectingStore`]
//! upserts that provider's row and points the settings row at it. A
//! connection test (`ai.testConnection`) writes a provider's partition,
//! `AiSettings/<its id>`, with its outcome alone: [`ProjectingStore`] writes
//! the four `last_test_*` columns of that row — of the active provider's when
//! the partition is the nil uuid, a test that reached no provider. A save of
//! an element (`campaign.saveEntity`) writes its partition, `Entity/<its
//! id>`, whole: [`ProjectingStore`] upserts its `entity` row and its sheet —
//! or drops the sheet of an element made a place or an event — makes its
//! `entity_alias` rows its aliases, and adds the version it carries
//! to `entity_version`, in one transaction — whose triggers judge the
//! element's campaign again. An export (`campaign.exportCampaign`) writes
//! its campaign's partition, `Campaign/<its id>`, with the stamp alone:
//! [`ProjectingStore`] writes `campaign.last_exported_at`, and no other
//! column. A scene's battlemap instance (`table.createBattlemapInstance`)
//! writes its partition, `BattlemapInstance/<its id>`: [`ProjectingStore`]
//! inserts its `battlemap_instance` row ([`instances`]). A token dropped on
//! a battlemap instance (`table.placeTokens`) writes its partition,
//! `BattlemapInstance/<its id>`: [`ProjectingStore`] inserts its `token` row
//! ([`tokens`]); a token hidden or revealed (`table.setTokenVisibility`)
//! writes it again with its visibility alone: [`ProjectingStore`] writes
//! `token.visible`, and no other column; a change of an NPC's HP
//! (`table.changeHitPoints`) writes it with its current HP alone:
//! [`ProjectingStore`] writes `token.hp_current`. A rename of a token
//! (`table.renameToken`) writes the same partition with its name alone:
//! [`ProjectingStore`] writes `token.name`, and no other column; a condition
//! badge put on or taken off (`table.setTokenCondition`), with its conditions
//! alone: [`ProjectingStore`] writes `token.conditions`, and no other column.
//! A « Suivant » (`table.nextTurn`) writes a combat's partition with its
//! current entry and its round alone: [`ProjectingStore`] writes
//! `combat.current_entry_id` and `combat.round` ([`turns`]).
//! A campaign's row is its table's over the engine's, and an archive is read
//! from the tables ([`ProjectingStore::archived`]). Every
//! other method is the inner store's.
//!
//! The projection runs after the engine's own transaction: when it fails,
//! the engine's row stays applied and the error is logged by its SQLSTATE
//! alone — never a value, which may hold the API key. [`ProjectingStore::repair`]
//! projects the row again at the next start: the settings row, and every
//! element whose version the tables do not hold yet — those again after each
//! apply of an element, so one the tables refused (a player they do not hold
//! yet) reaches them once they can. The tables' CHECKs, their foreign keys
//! and their triggers judge it a second time. An element's terms reach
//! [`Aliases`] once its projection is committed, and only then: the
//! tables' aliases are the ones `alias-unique-in-campaign` judges against.
//! So do its versions reach [`History`], as `entity_version` numbers them:
//! the ones a restoration names. A stamp the table lost is written again at
//! the next start, never over a later one.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use harness_dataguard::Result;
use harness_dataguard::command::{CommandResult, Entry};
use harness_dataguard::postgres::PgStore;
use harness_dataguard::queue::Hold;
use harness_dataguard::store::{Collision, Store, Write};
use restore_entity_version::Kept;
use serde_json::Value;
use test_connection::NO_PROVIDER;

use crate::ai_settings::uuid;
use crate::aliases::Aliases;
use crate::history::History;
use crate::instances;
use crate::tokens;
use crate::turns;
use crate::versioning::{ENTITY, element_of};

/// The partition whose writes reach the tables: the one settings row.
pub const AI_SETTINGS: &str = "AiSettings/1";

/// The active provider's row, upserted by its id; `last_test_*` untouched.
const UPSERT_PROVIDER: &str = r#"INSERT INTO ai_provider
        (id, mode, label, url, detected_api_type, api_key, "binary", generation_model, summary_model)
    VALUES (($1::text)::uuid, $2, $3, $4, $5, $6, $7, $8, $9)
    ON CONFLICT (id) DO UPDATE SET
        mode = EXCLUDED.mode,
        label = EXCLUDED.label,
        url = EXCLUDED.url,
        detected_api_type = EXCLUDED.detected_api_type,
        api_key = EXCLUDED.api_key,
        "binary" = EXCLUDED."binary",
        generation_model = EXCLUDED.generation_model,
        summary_model = EXCLUDED.summary_model"#;

/// The settings row pointed at it.
const ACTIVATE: &str = "UPDATE ai_settings
    SET active_provider_id = ($1::text)::uuid,
        updated_at = coalesce(($2::text)::timestamptz, now())
    WHERE id = 1";

/// A connection test's outcome on a provider's row, and nothing else of it;
/// on the active provider's when no provider is named (`$1` null). Never
/// over a later test: an apply wins a tie (the engine applies in order), a
/// replay (`$6`) does not, so a start never restores an older test.
const LAST_TEST: &str = "UPDATE ai_provider
    SET last_test_ok = $2,
        last_test_at = ($3::text)::timestamptz,
        last_test_message = $4,
        last_test_latency_ms = $5
    WHERE id = coalesce(($1::text)::uuid, (SELECT active_provider_id FROM ai_settings WHERE id = 1))
        AND (last_test_at IS NULL
            OR last_test_at < ($3::text)::timestamptz
            OR (NOT $6 AND last_test_at = ($3::text)::timestamptz))";

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

/// A campaign as its table holds it, under the Campaign aggregate's fields,
/// `$1` its id in whatever case: what the engine judges an export against —
/// the fixtures' campaigns, and every one no command wrote yet. A campaign
/// the engine deleted (its tombstone) is none.
const CAMPAIGN_ROW: &str = "SELECT jsonb_strip_nulls(jsonb_build_object(
        'id', id, 'name', name, 'kind', kind, 'pitch', pitch, 'art', art, 'glyph', glyph,
        'coverMediaId', cover_media_id, 'createdAt', created_at,
        'lastExportedAt', last_exported_at, 'importedAt', imported_at,
        'originCampaignId', origin_campaign_id, 'soundMasterVolume', sound_master_volume
    ))::text
    FROM campaign c WHERE c.id::text = lower($1)
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Campaign/' || c.id::text AND r.tombstoned)";

/// The last export the engine stamped on the campaign partition `$1` names —
/// on every one when it is null — written on its row: the one column an
/// export changes (`export-stamps-date`), never moved back. After each apply
/// of a campaign, and at a start, so a stamp whose projection failed after
/// its apply is written again. A stamp that is no time is left aside, so it
/// keeps no other campaign's from its row.
const STAMP_EXPORTS: &str = "UPDATE campaign c
    SET last_exported_at = (r.fields ->> 'lastExportedAt')::timestamptz
    FROM dataguard_rows r
    WHERE r.partition = 'Campaign/' || c.id::text AND NOT r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND pg_input_is_valid(r.fields ->> 'lastExportedAt', 'timestamptz')
        AND (c.last_exported_at IS NULL
            OR c.last_exported_at < (r.fields ->> 'lastExportedAt')::timestamptz)";

/// The name the engine holds of the token partition `$1` names — of every
/// token when it is null — written on its row: the one column a rename
/// changes, and the token's own alone (`multi-drop-numbering`). After each
/// apply of a token, and at a start, so a name whose projection failed after
/// its apply is written again. The token is found by its key, the id its
/// partition names. A row the table does not hold yet is the drop's to
/// insert; a name that is blanks alone is left aside, as the DataGuard
/// refuses it.
const NAME_TOKENS: &str = "UPDATE token t
    SET name = r.fields ->> 'name'
    FROM dataguard_rows r
    WHERE NOT r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'tokenId')
        AND t.id = CASE WHEN pg_input_is_valid(r.fields ->> 'tokenId', 'uuid')
            THEN (r.fields ->> 'tokenId')::uuid END
        AND btrim(r.fields ->> 'name') <> ''
        AND t.name IS DISTINCT FROM r.fields ->> 'name'";

/// The conditions the engine holds of the token partition `$1` names — of
/// every token when it is null — written on its row: the one column a badge
/// set changes, and the token's own alone (`instance-isolated`). After each
/// apply of a token, and at a start, so conditions whose projection failed
/// after their apply are written again (`state-persisted-reset`). The token
/// is found by its key, the id its partition names. A row the table does not
/// hold yet is the drop's to insert; conditions that are no list of names
/// are left aside, as the DataGuard refuses them.
const CONDITION_TOKENS: &str = "UPDATE token t
    SET conditions = ARRAY(SELECT c.name FROM jsonb_array_elements_text(r.fields -> 'conditions')
        WITH ORDINALITY AS c(name, position) ORDER BY c.position)
    FROM dataguard_rows r
    WHERE NOT r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'tokenId')
        AND t.id = CASE WHEN pg_input_is_valid(r.fields ->> 'tokenId', 'uuid')
            THEN (r.fields ->> 'tokenId')::uuid END
        AND jsonb_typeof(r.fields -> 'conditions') = 'array'
        AND NOT jsonb_path_exists(r.fields -> 'conditions', '$[*] ? (@.type() != \"string\")')
        AND t.conditions IS DISTINCT FROM
            ARRAY(SELECT c.name FROM jsonb_array_elements_text(r.fields -> 'conditions')
                WITH ORDINALITY AS c(name, position) ORDER BY c.position)";

/// What an archive holds of the campaign `$1` names, in whatever case, as
/// the tables hold it — the campaign, its players, its elements each with
/// its sheet, its aliases and its versions — in one statement, so one
/// snapshot: each row under its columns' names, in a stable order. A
/// campaign the engine deleted is none.
const ARCHIVE: &str = "SELECT jsonb_build_object(
        'campaign', to_jsonb(c),
        'players', coalesce((SELECT jsonb_agg(to_jsonb(p) ORDER BY p.created_at, p.id)
            FROM player p WHERE p.campaign_id = c.id), '[]'),
        'elements', coalesce((SELECT jsonb_agg(to_jsonb(e) || jsonb_build_object(
                'character', (SELECT to_jsonb(ch) FROM character ch WHERE ch.entity_id = e.id),
                'aliases', coalesce((SELECT jsonb_agg(a.alias ORDER BY a.alias, a.id)
                    FROM entity_alias a WHERE a.entity_id = e.id), '[]'),
                'versions', coalesce((SELECT jsonb_agg(to_jsonb(v) ORDER BY v.version_number)
                    FROM entity_version v WHERE v.entity_id = e.id), '[]'))
            ORDER BY e.created_at, e.id)
            FROM entity e WHERE e.campaign_id = c.id), '[]')
    )::text
    FROM campaign c WHERE c.id::text = lower($1)
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Campaign/' || c.id::text AND r.tombstoned)";

/// The partitions of the Campaign aggregate: `Campaign/<campaign id>`.
const CAMPAIGN: &str = "Campaign/";

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
        self.repair_settings().await;
        self.repair_elements().await;
        self.repair_exports(None).await;
        {
            let client = self.inner.client().lock().await;
            instances::repair(&client).await;
            tokens::repair(&client).await;
            tokens::show(&client, None).await;
            tokens::hit_points(&client, None).await;
            turns::turn(&client, None).await;
        }
        self.repair_names(None).await;
        self.repair_conditions(None).await;
    }

    /// Writes the name the engine holds of the token `partition` names — of
    /// every token, `None` — on its row, logging a failure: after each apply
    /// of a token, and at a start. Tables not migrated yet hold none.
    async fn repair_names(&self, partition: Option<&str>) {
        let written = {
            let client = self.inner.client().lock().await;
            client.execute(NAME_TOKENS, &[&partition]).await
        };
        if let Err(e) = written {
            eprintln!(
                "projection: token not renamed: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            );
        }
    }

    /// Writes the conditions the engine holds of the token `partition` names
    /// — of every token, `None` — on its row, logging a failure: after each
    /// apply of a token, and at a start. Tables not migrated yet hold none.
    async fn repair_conditions(&self, partition: Option<&str>) {
        let written = {
            let client = self.inner.client().lock().await;
            client.execute(CONDITION_TOKENS, &[&partition]).await
        };
        if let Err(e) = written {
            eprintln!(
                "projection: token conditions not written: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            );
        }
    }

    /// What the tables hold of the session, the scene and the battlemap a
    /// creation of an instance names (`table.createBattlemapInstance`).
    pub async fn placement(
        &self,
        session: &str,
        scene: &str,
        battlemap: &str,
    ) -> std::result::Result<create_battlemap_instance::Placement, tokio_postgres::Error> {
        let client = self.inner.client().lock().await;
        instances::placement(&client, session, scene, battlemap).await
    }

    /// What the tables hold of the instance and the character a drop of
    /// tokens names (`table.placeTokens`).
    pub async fn roster(
        &self,
        instance: &str,
        character: &str,
    ) -> std::result::Result<place_tokens::Roster, tokio_postgres::Error> {
        let client = self.inner.client().lock().await;
        tokens::roster(&client, instance, character).await
    }

    /// The combats the engine holds of the instance a « Suivant » names
    /// (`table.nextTurn`), open or ended.
    pub async fn combats(
        &self,
        instance: &str,
    ) -> std::result::Result<Vec<String>, tokio_postgres::Error> {
        let client = self.inner.client().lock().await;
        turns::combats(&client, instance).await
    }

    /// The entries the engine holds of the combat a « Suivant » turns
    /// (`table.nextTurn`).
    pub async fn entries(
        &self,
        combat: &str,
    ) -> std::result::Result<Vec<String>, tokio_postgres::Error> {
        let client = self.inner.client().lock().await;
        turns::entries(&client, combat).await
    }

    /// Writes the last export the engine stamped on the row of the campaign
    /// `partition` names — of every campaign, `None` — logging a failure:
    /// after each apply of a campaign, and at a start.
    async fn repair_exports(&self, partition: Option<&str>) {
        let written = {
            let client = self.inner.client().lock().await;
            client.execute(STAMP_EXPORTS, &[&partition]).await
        };
        if let Err(e) = written {
            eprintln!(
                "projection: campaign not stamped: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            );
        }
    }

    /// The campaign `id` names as an archive holds it — `{campaign, players,
    /// elements}` — or `None` when the tables hold no such campaign.
    pub async fn archived(
        &self,
        id: &str,
    ) -> std::result::Result<Option<Value>, tokio_postgres::Error> {
        let client = self.inner.client().lock().await;
        let row = client.query_opt(ARCHIVE, &[&id]).await?;
        Ok(row.and_then(|row| serde_json::from_str(row.get::<_, &str>(0)).ok()))
    }

    /// The campaign `partition` names as its table holds it, when it does.
    async fn campaign_row(
        &self,
        partition: &str,
    ) -> std::result::Result<BTreeMap<String, Value>, tokio_postgres::Error> {
        let Some(id) = partition.strip_prefix(CAMPAIGN) else {
            return Ok(BTreeMap::new());
        };
        let client = self.inner.client().lock().await;
        let row = client.query_opt(CAMPAIGN_ROW, &[&id]).await?;
        Ok(row
            .and_then(|row| serde_json::from_str::<Value>(row.get::<_, &str>(0)).ok())
            .and_then(|row| match row {
                Value::Object(row) => Some(row.into_iter().collect()),
                _ => None,
            })
            .unwrap_or_default())
    }

    /// Projects `AiSettings/1` as the engine holds it, when it carries a
    /// provider, logging a failure: after each apply of it, and at a start.
    async fn repair_settings(&self) {
        let row = match self.inner.row(AI_SETTINGS).await {
            Ok(row) => row,
            Err(e) => {
                eprintln!("projection: {AI_SETTINGS} not read: {e}");
                return;
            }
        };
        if let Err(e) = self.project(&row).await {
            // The SQLSTATE and the constraint only: a database error's
            // detail quotes the failing row, the API key with it.
            let db = e.as_db_error();
            eprintln!(
                "projection: {AI_SETTINGS} not projected: {} {}",
                db.map_or("-", |db| db.code().code()),
                db.and_then(|db| db.constraint()).unwrap_or_default()
            );
        }
    }

    /// Writes the last test of every provider the engine holds one for: at a
    /// start, so a test whose projection failed after its apply is written
    /// again. The test that reached no provider is not: the active provider
    /// may have changed since. A row holding a test as late or later keeps it.
    pub async fn repair_tests(&self) {
        let ids = {
            let client = self.inner.client().lock().await;
            client.query("SELECT id::text FROM ai_provider", &[]).await
        };
        match ids {
            Ok(rows) => {
                for row in rows {
                    let id: String = row.get(0);
                    self.record_test(&format!("AiSettings/{id}"), &id, true)
                        .await;
                }
            }
            Err(e) => eprintln!(
                "projection: ai_provider not read: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            ),
        }
    }

    /// Writes the last test `partition` holds on its provider's row, unless
    /// that row holds a later one (or, `replay`, an as late one), logging a
    /// failure as [`Self::repair`] does.
    async fn record_test(&self, partition: &str, provider: &str, replay: bool) {
        let row = match self.inner.row(partition).await {
            Ok(row) => row,
            Err(e) => {
                eprintln!("projection: {partition} not read: {e}");
                return;
            }
        };
        if !row.contains_key("lastTestAt") {
            return;
        }
        let latency = row
            .get("lastTestLatencyMs")
            .and_then(Value::as_i64)
            .and_then(|ms| i32::try_from(ms).ok());
        let written = self
            .inner
            .client()
            .lock()
            .await
            .execute(
                LAST_TEST,
                &[
                    &(provider != NO_PROVIDER).then_some(provider),
                    &row.get("lastTestOk").and_then(Value::as_bool),
                    &text(&row, "lastTestAt"),
                    &text(&row, "lastTestMessage"),
                    &latency,
                    &replay,
                ],
            )
            .await;
        if let Err(e) = written {
            let db = e.as_db_error();
            eprintln!(
                "projection: {partition} not projected: {} {}",
                db.map_or("-", |db| db.code().code()),
                db.and_then(|db| db.constraint()).unwrap_or_default()
            );
        }
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

    /// Writes `row`'s provider and makes it active, in one transaction.
    async fn project(
        &self,
        row: &BTreeMap<String, Value>,
    ) -> std::result::Result<(), tokio_postgres::Error> {
        let Some(id) = text(row, "aiProviderId") else {
            return Ok(());
        };
        let mut client = self.inner.client().lock().await;
        let tx = client.transaction().await?;
        tx.execute(
            UPSERT_PROVIDER,
            &[
                &id,
                &text(row, "mode"),
                &text(row, "label"),
                &text(row, "url"),
                &text(row, "detectedApiType"),
                &text(row, "apiKey"),
                &text(row, "binary"),
                &text(row, "generationModel"),
                &text(row, "summaryModel"),
            ],
        )
        .await?;
        tx.execute(
            ACTIVATE,
            &[
                &text(row, "activeProviderId").unwrap_or(id),
                &text(row, "updatedAt"),
            ],
        )
        .await?;
        tx.commit().await
    }
}

/// Whether `write` reaches the tables: a live write of the settings row.
fn projects(write: &Write) -> bool {
    !write.tombstone && write.partition == AI_SETTINGS
}

/// Whether `write` reaches the element's tables: a live write of an
/// `Entity/<id>` partition.
fn element(write: &Write) -> bool {
    !write.tombstone && element_of(write).is_some()
}

/// The provider whose last test `write` records: a live write of a
/// provider's partition, `AiSettings/<uuid>`.
fn tested(write: &Write) -> Option<&str> {
    if write.tombstone {
        return None;
    }
    write
        .partition
        .strip_prefix("AiSettings/")
        .filter(|id| uuid(id))
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

    /// A campaign's row is its table's over the engine's own: a campaign the
    /// fixtures hold exists for the engine before any command wrote it, and
    /// what the engine copied of it when it applied a command is never
    /// judged over what the table holds now. Tables not migrated yet hold
    /// none.
    async fn row(&self, partition: &str) -> Result<BTreeMap<String, Value>> {
        let mut row = self.inner.row(partition).await?;
        if !partition.starts_with(CAMPAIGN) {
            return Ok(row);
        }
        let table = match self.campaign_row(partition).await {
            Ok(row) => row,
            Err(e) => {
                eprintln!(
                    "projection: campaign not read: {}",
                    e.as_db_error().map_or("-", |db| db.code().code())
                );
                BTreeMap::new()
            }
        };
        row.extend(table);
        Ok(row)
    }

    async fn collision(&self, entry: &Entry) -> Result<Collision> {
        self.inner.collision(entry).await
    }

    async fn incoming(&self, partition: &str) -> Result<Vec<(String, u64)>> {
        self.inner.incoming(partition).await
    }

    async fn apply(&self, write: &Write, result: &CommandResult) -> Result<u64> {
        let version = self.inner.apply(write, result).await?;
        if projects(write) {
            self.repair_settings().await;
        } else if let Some(provider) = tested(write) {
            self.record_test(&write.partition, provider, false).await;
        } else if element(write) {
            self.repair_elements().await;
        } else if !write.tombstone && tokens::of_aggregate(&write.partition) {
            // An instance and its tokens share the aggregate's partitions:
            // each projection inserts only the row whose id the partition names.
            {
                let client = self.inner.client().lock().await;
                instances::project(&client, &write.partition).await;
                tokens::project(&client, &write.partition).await;
                tokens::show(&client, Some(&write.partition)).await;
                tokens::hit_points(&client, Some(&write.partition)).await;
                turns::turn(&client, Some(&write.partition)).await;
            }
            // A rename, a badge set, or any later write, makes the token's name
            // and conditions follow the engine's.
            self.repair_names(Some(&write.partition)).await;
            self.repair_conditions(Some(&write.partition)).await;
        } else if write.partition.starts_with(CAMPAIGN) {
            // A campaign's tombstone stamps nothing: STAMP_EXPORTS skips it.
            self.repair_exports(Some(&write.partition)).await;
        } else if let Some(element) = element_of(write) {
            // A removal: tombstones are not projected, its terms and its
            // versions go.
            self.aliases.forget(element);
            self.history.forget(element);
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
    use crate::{Engine, bound};

    const MIGRATION: &str = include_str!("../../../data/migrations/0001_ai_settings.sql");
    const CLOUD: &str = "a1000000-0000-4000-8000-000000000001";
    const HOME: &str = "a1000000-0000-4000-8000-000000000002";

    #[test]
    fn only_a_live_write_of_a_provider_s_partition_records_a_test() {
        let write = |partition: &str, tombstone| Write {
            partition: partition.to_string(),
            values: BTreeMap::new(),
            tombstone,
        };
        let cloud = format!("AiSettings/{CLOUD}");
        assert_eq!(tested(&write(&cloud, false)), Some(CLOUD));
        assert_eq!(
            tested(&write(&format!("AiSettings/{NO_PROVIDER}"), false)),
            Some(NO_PROVIDER)
        );
        assert_eq!(tested(&write(&cloud, true)), None);
        assert_eq!(tested(&write(AI_SETTINGS, false)), None);
        assert_eq!(tested(&write(&format!("Other/{CLOUD}"), false)), None);
        // 36 characters, hex and hyphens, but not where a uuid has them.
        for id in [
            "0123456789abcdef0123456789abcdef0123",
            "a1000000-0000-4000-8000-00000000-001",
        ] {
            assert_eq!(tested(&write(&format!("AiSettings/{id}"), false)), None);
        }
    }

    #[test]
    fn only_a_live_write_of_the_settings_row_is_projected() {
        let write = |partition: &str, tombstone| Write {
            partition: partition.to_string(),
            values: BTreeMap::new(),
            tombstone,
        };
        assert!(projects(&write(AI_SETTINGS, false)));
        assert!(!projects(&write(AI_SETTINGS, true)));
        assert!(!projects(&write(&format!("AiSettings/{CLOUD}"), false)));
    }

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

    /// The state ai.saveSettings writes for `provider`, its url `url`.
    fn state(provider: &str, url: &str, api_type: Option<&str>) -> BTreeMap<String, Value> {
        let state = json!({
            "aiProviderId": provider,
            "activeProviderId": provider,
            "mode": "api",
            "label": "API",
            "url": url,
            "detectedApiType": api_type,
            "apiKey": null,
            "binary": null,
            "generationModel": "g",
            "summaryModel": "s",
            "updatedAt": "2026-10-17T10:00:00Z",
        });
        state
            .as_object()
            .expect("an object")
            .clone()
            .into_iter()
            .collect()
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

    /// Every column of both tables, as text, `last_test_*` aside or not.
    async fn snapshot(tables: &Client, last_test: bool) -> Vec<String> {
        let drop = if last_test {
            ""
        } else {
            " - 'last_test_ok' - 'last_test_at' - 'last_test_message' - 'last_test_latency_ms'"
        };
        let sql = format!(
            "SELECT (to_jsonb(p){drop})::text FROM ai_provider p \
             UNION ALL SELECT to_jsonb(s)::text FROM ai_settings s ORDER BY 1"
        );
        tables
            .query(&sql, &[])
            .await
            .expect("the tables")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    #[tokio::test]
    async fn a_connection_test_reaches_the_last_test_columns_of_its_row_and_nothing_else() {
        let url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL");
        let name = format!("projection_last_test_{}", std::process::id());
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
        tables
            .batch_execute(&format!(
                "UPDATE ai_settings SET active_provider_id = '{HOME}' WHERE id = 1"
            ))
            .await
            .expect("the home server active");
        let store = Arc::new(ProjectingStore::new(
            PgStore::open(connect(&scratch).await)
                .await
                .expect("the store"),
            Arc::default(),
        ));
        let engine = bound(
            Engine::new(
                store.clone(),
                Arc::new(MemoryNotifier::default()),
                Arc::new(SystemClock),
            ),
            Arc::default(),
        )
        .expect("the product's manifests parse");
        engine.load().await.expect("an empty queue");
        let before = snapshot(&tables, false).await;

        // The home server tested first, then a test that reached no
        // provider lands on it, then a test applied late, older than both.
        for (target, ok, message, latency, at) in [
            (
                HOME,
                true,
                "Connexion réussie",
                json!(30),
                "2026-10-17T09:00:00Z",
            ),
            (
                CLOUD,
                true,
                "Connexion réussie",
                json!(12),
                "2026-10-17T10:00:00Z",
            ),
            (
                NO_PROVIDER,
                false,
                "type d'API non reconnu",
                Value::Null,
                "2026-10-17T10:00:00Z",
            ),
            (
                HOME,
                true,
                "Connexion réussie",
                json!(40),
                "2026-10-17T08:00:00Z",
            ),
        ] {
            let payload = json!({
                "lastTestOk": ok,
                "lastTestAt": at,
                "lastTestMessage": message,
                "lastTestLatencyMs": latency,
            });
            engine
                .submit(Submission {
                    data_capability: "ai.testConnection@1".to_string(),
                    payload: payload
                        .as_object()
                        .expect("an object")
                        .clone()
                        .into_iter()
                        .collect(),
                    target_id: Some(target.to_string()),
                    based_on: None,
                    idempotency_key: None,
                    caller: Caller {
                        by: "dm".to_string(),
                        from: "AiSettings".to_string(),
                        permissions: Vec::new(),
                        roles: Vec::new(),
                    },
                })
                .await
                .expect("queued");
            let settled = engine.run_once().await.expect("applied");
            assert_eq!(settled[0].status, State::Applied, "{settled:?}");
        }

        let tested = |id: &str| {
            format!(
                "SELECT last_test_ok, last_test_message, last_test_latency_ms, \
                 last_test_at = '2026-10-17T10:00:00Z' FROM ai_provider WHERE id = '{id}'"
            )
        };
        let cloud = tables.query_one(&tested(CLOUD), &[]).await.expect("cloud");
        assert_eq!(cloud.get::<_, Option<bool>>(0), Some(true));
        assert_eq!(cloud.get::<_, Option<&str>>(1), Some("Connexion réussie"));
        assert_eq!(cloud.get::<_, Option<i32>>(2), Some(12));
        assert_eq!(cloud.get::<_, Option<bool>>(3), Some(true));
        // The test that reached no provider is the active one's.
        let home = tables.query_one(&tested(HOME), &[]).await.expect("home");
        assert_eq!(home.get::<_, Option<bool>>(0), Some(false));
        assert_eq!(
            home.get::<_, Option<&str>>(1),
            Some("type d'API non reconnu")
        );
        assert_eq!(home.get::<_, Option<i32>>(2), None);
        let untested = snapshot(&tables, true).await;
        assert_eq!(
            untested
                .iter()
                .filter(|row| row.contains("\"last_test_ok\": null"))
                .count(),
            2,
            "{untested:?}"
        );
        assert_eq!(snapshot(&tables, false).await, before);

        // A projection lost after its apply is made again at a start; the
        // test that reached no provider is left where it landed, over the
        // home server's own older one.
        let tests = snapshot(&tables, true).await;
        tables
            .batch_execute(&format!(
                "UPDATE ai_provider SET last_test_ok = NULL, last_test_at = NULL, \
                 last_test_message = NULL, last_test_latency_ms = NULL WHERE id = '{CLOUD}'"
            ))
            .await
            .expect("the cloud's test lost");
        store.repair_tests().await;
        assert_eq!(snapshot(&tables, true).await, tests);

        drop((engine, store, tables));
        admin
            .batch_execute(&format!("DROP DATABASE {name} WITH (FORCE)"))
            .await
            .expect("the scratch database dropped");
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
            Arc::default(),
        )
        .expect("the product's manifests parse");
        engine.load().await.expect("an empty queue");

        let approver = ["role:approver"];
        let keyed = submitted(
            &engine,
            state(CLOUD, "https://api.anthropic.com/v1", Some("cloud")),
            Some("k-1"),
            &approver,
        )
        .await;
        engine.run_once().await.expect("applied");
        submitted(
            &engine,
            state(HOME, "http://192.168.1.40:4096", Some("opencode")),
            None,
            &approver,
        )
        .await;
        engine.run_once().await.expect("applied");
        let refused = submitted(
            &engine,
            state(HOME, "https://inconnu.example.org", None),
            None,
            &approver,
        )
        .await;
        let settled = engine.run_once().await.expect("rejected");
        assert_eq!(settled[0].status, State::Rejected, "{settled:?}");
        // Without the approver's role: it waits, pending, at the head.
        let waiting = submitted(
            &engine,
            state(CLOUD, "https://api.anthropic.com/v1", Some("cloud")),
            None,
            &[],
        )
        .await;
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
