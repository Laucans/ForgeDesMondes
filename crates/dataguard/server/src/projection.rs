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
        if !write.tombstone && write.partition == AI_SETTINGS {
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
