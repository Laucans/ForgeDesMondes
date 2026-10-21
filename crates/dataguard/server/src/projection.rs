//! The engine's writes, carried into the product's tables.
//!
//! The engine keeps a row per partition in its own table; the product reads
//! its tables (`crates/data/migrations/`). The AiSettings aggregate projects
//! its own writes, in the apply transaction (`aggregate_ai_settings`), and so
//! does the Entity aggregate (`aggregate_entity`); an element's terms are
//! read here at a start ([`ProjectingStore::elements`]), for an analysis of
//! notes to recognise. An analysis of notes (`ai.extractFromNotes`) writes a partition
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
//! projects the row again at the next start. The tables' CHECKs, their
//! foreign keys and their triggers judge it a second time.

use std::collections::BTreeMap;
use std::sync::Arc;

use aggregate_entity::Aliases;
use async_trait::async_trait;
use harness_dataguard::Result;
use harness_dataguard::command::{CommandResult, Entry};
use harness_dataguard::postgres::PgStore;
use harness_dataguard::queue::Hold;
use harness_dataguard::store::{Collision, Store, Write};
use serde_json::Value;

use crate::notes::Notes;
use crate::versioning::NOTE_IMPORT;

/// Every element of the tables with its campaign, name and aliases, as the
/// engine's row holds them: what [`Aliases`] starts from — the terms an
/// analysis of notes recognises (`Rows::known`).
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

/// A [`PgStore`] whose applied writes of the aggregates it still projects
/// reach the tables, reading at a start the elements' terms the tables hold.
pub struct ProjectingStore {
    inner: PgStore,
    aliases: Arc<Aliases>,
}

impl ProjectingStore {
    /// The store over `inner`, keeping `aliases`.
    pub fn new(inner: PgStore, aliases: Arc<Aliases>) -> Self {
        Self { inner, aliases }
    }

    /// Projects what the engine holds and the tables do not, logging a
    /// failure: at a start, so a projection that failed after its apply is
    /// made again.
    pub async fn repair(&self) {
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
            plugins(),
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
