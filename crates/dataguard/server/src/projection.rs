//! The engine's writes, carried into the product's tables.
//!
//! The engine keeps a row per partition in its own table; the product reads
//! its tables (`crates/data/migrations/`). The AiSettings aggregate projects
//! its own writes, in the apply transaction (`aggregate_ai_settings`), and so
//! do the Entity and NoteImport aggregates. An export (`campaign.exportCampaign`) writes
//! its campaign's partition, `Campaign/<its id>`, with the stamp alone:
//! [`ProjectingStore`] writes `campaign.last_exported_at`, and no other
//! column. A
//! campaign's row is its table's over the engine's, and an archive is read
//! from the tables ([`ProjectingStore::archived`]). Every other method is
//! the inner store's.
//!
//! The projection runs after the engine's own transaction: when it fails,
//! the engine's row stays applied and the error is logged by its SQLSTATE
//! alone — never a value. [`ProjectingStore::repair`]
//! projects the row again at the next start. The tables' CHECKs, their
//! foreign keys and their triggers judge it a second time. A stamp the table
//! lost is written again at the next start, never over a later one.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::Result;
use harness_dataguard::command::{CommandResult, Entry};
use harness_dataguard::postgres::PgStore;
use harness_dataguard::queue::Hold;
use harness_dataguard::store::{Collision, Store, Write};
use serde_json::Value;

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

/// A [`PgStore`] whose applied writes of the aggregates it still projects
/// reach the tables.
pub struct ProjectingStore {
    inner: PgStore,
}

impl ProjectingStore {
    /// The store over `inner`.
    pub const fn new(inner: PgStore) -> Self {
        Self { inner }
    }

    /// Projects what the engine holds and the tables do not, logging a
    /// failure: at a start, so a projection that failed after its apply is
    /// made again.
    pub async fn repair(&self) {
        // --- Campaign
        self.repair_exports(None).await;
        // ---
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
    async fn table_row(
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
        let table = match self.table_row(partition).await {
            Ok(row) => row,
            Err(e) => {
                eprintln!(
                    "projection: {partition} not read: {}",
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

    /// The inner store's apply, then each aggregate's projection — one
    /// block each, on its own partitions, deleted when the aggregate
    /// projects in the apply transaction (its `ProjectPg`).
    async fn apply(&self, write: &Write, result: &CommandResult) -> Result<u64> {
        let version = self.inner.apply(write, result).await?;
        // --- Campaign
        if write.partition.starts_with(CAMPAIGN) {
            // A campaign's tombstone stamps nothing: STAMP_EXPORTS skips it.
            self.repair_exports(Some(&write.partition)).await;
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

    use std::sync::Arc;

    use super::*;
    use crate::{Engine, bound, plugins};

    const MIGRATION: &str = include_str!("../../../data/migrations/0001_ai_settings.sql");
    /// The settings row's partition: a write the tables of no aggregate
    /// projected here reach.
    const AI_SETTINGS: &str = "AiSettings/1";
    const HOME: &str = "a1000000-0000-4000-8000-000000000002";

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
    /// ai.saveSettings detects the server's API type through; and the
    /// `speech-to-text` port's, which an analysis of notes needs.
    fn home_server() -> harness_dataguard::plugins::Shared {
        let llm = ports_llm::fake::FakeLlm::new().with_endpoint(
            "http://192.168.1.40:4096",
            json!({ "object": "list", "data": [] }),
        );
        let mut shared = harness_dataguard::plugins::Shared::default();
        shared.insert(Arc::new(ports_llm::SharedLlm::new(llm)));
        shared.insert(Arc::new(ports_speech_to_text::SharedSpeechToText::new(
            ports_speech_to_text::fake::FakeSpeechToText::new(String::new()),
        )));
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
