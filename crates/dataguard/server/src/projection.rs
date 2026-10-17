//! The engine's writes, carried into the product's tables.
//!
//! The engine keeps a row per partition in its own table; the product reads
//! `ai_provider` and `ai_settings` (`crates/data/migrations/`). A save of the
//! AI settings is one command on `AiSettings/1` carrying the active provider
//! whole (`ai.saveSettings`): once the engine applied it, [`ProjectingStore`]
//! upserts that provider's row and points the settings row at it. Every other
//! method is the inner store's.
//!
//! The projection runs after the engine's own transaction: when it fails,
//! the engine's row stays applied and the error is logged by its SQLSTATE
//! alone — never a value, which may hold the API key. [`ProjectingStore::repair`]
//! projects the row again at the next start. The tables' CHECKs and their
//! foreign key judge it a second time.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::Result;
use harness_dataguard::command::{CommandResult, Entry};
use harness_dataguard::postgres::PgStore;
use harness_dataguard::queue::Hold;
use harness_dataguard::store::{Collision, Store, Write};
use serde_json::Value;

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

/// A [`PgStore`] whose applied `AiSettings/1` writes reach the tables.
pub struct ProjectingStore {
    inner: PgStore,
}

impl ProjectingStore {
    /// The store over `inner`.
    pub const fn new(inner: PgStore) -> Self {
        Self { inner }
    }

    /// Projects `AiSettings/1` as the engine holds it, when it carries a
    /// provider, logging a failure: after each apply, and at a start, so a
    /// projection that failed after its apply is made again.
    pub async fn repair(&self) {
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

    async fn apply(&self, write: &Write, result: &CommandResult) -> Result<u64> {
        let version = self.inner.apply(write, result).await?;
        if projects(write) {
            self.repair().await;
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

    use std::sync::Arc;

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
        let engine = bound(Engine::new(
            projecting.clone(),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        ))
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
