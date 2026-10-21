//! The aggregate's writes, carried into the product's tables `ai_provider`
//! and `ai_settings` (`crates/data/migrations/0001_ai_settings.sql`) inside
//! the transaction that applies them ([`Projection`]).
//!
//! A save of the AI settings (`ai.saveSettings`) is one command on
//! [`SETTINGS_PARTITION`] carrying the active provider whole: its provider's row is
//! upserted and the settings row pointed at it. A connection test
//! (`ai.testConnection`) writes a provider's partition, `AiSettings/<its
//! id>`, with its outcome alone: the four `last_test_*` columns of that row
//! are written — the active provider's when the partition is
//! [`NO_PROVIDER`], a test that reached no provider — never over a later
//! test.
//!
//! The tables judge each write a second time: a write their CHECKs, their
//! keys or their types refuse rolls back, and the command is rejected. What
//! the author reads of it is the SQLSTATE and the constraint or column,
//! never a value: a row may hold the API key.
//!
//! When the store starts ([`ProjectPg::warm`]), what the engine holds and
//! the tables do not is written again: the settings row, and every
//! provider's last test, never over a later one — tables written before
//! the projection ran in the apply transaction, or reset since. Tables not
//! migrated yet hold none of it, and are left alone.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{Client, GenericClient, Transaction};
use serde_json::Value;

use crate::uuid;

/// The settings row's partition: the one row `ai.saveSettings` writes.
pub const SETTINGS_PARTITION: &str = "AiSettings/1";

/// The partition of a connection test that reached no provider: the nil
/// uuid, no provider's row. Its outcome is written on the active provider.
pub const NO_PROVIDER: &str = "00000000-0000-0000-0000-000000000000";

/// The prefix of every partition of the aggregate.
const PARTITION: &str = "AiSettings/";

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

/// Whether both tables exist: a database the product's migrations have not
/// reached holds neither.
const MIGRATED: &str =
    "SELECT to_regclass('ai_provider') IS NOT NULL AND to_regclass('ai_settings') IS NOT NULL";

/// The settings row as the engine holds it.
const SETTINGS_ROW: &str =
    "SELECT fields FROM dataguard_rows WHERE partition = $1 AND NOT tombstoned";

/// Every provider's last test the engine holds: the partitions of the
/// providers the tables hold — never the test that reached no provider,
/// whose active provider may have changed since.
const LAST_TESTS: &str = "SELECT p.id::text, r.fields FROM ai_provider p
    JOIN dataguard_rows r ON r.partition = 'AiSettings/' || p.id::text
    WHERE NOT r.tombstoned AND r.fields ? 'lastTestAt'
    ORDER BY 1";

/// The aggregate's projection into `ai_provider` and `ai_settings`.
pub struct Projection;

#[async_trait]
impl ProjectPg for Projection {
    /// A removal carries no field, so projects nothing: the settings row is
    /// never removed (`single-settings-row`), nor a provider it names
    /// (`restrict`).
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        let projected = if write.partition == SETTINGS_PARTITION {
            activate(tx, &write.values).await
        } else if let Some(provider) = tested(write) {
            record_test(tx, provider, &write.values, false).await
        } else {
            return Ok(());
        };
        projected.map_err(|e| refused(&write.partition, &e))
    }

    async fn warm(&self, client: &Client) -> Result<(), ProjectionError> {
        let migrated: bool = client
            .query_one(MIGRATED, &[])
            .await
            .map_err(|e| refused("ai_provider", &e))?
            .get(0);
        if !migrated {
            return Ok(());
        }
        let settings = client
            .query_opt(SETTINGS_ROW, &[&SETTINGS_PARTITION])
            .await
            .map_err(|e| refused(SETTINGS_PARTITION, &e))?;
        if let Some(row) = settings {
            let fields = object(row.get(0));
            let repaired: Result<(), harness_dataguard::tokio_postgres::Error> = async {
                client.batch_execute("BEGIN").await?;
                activate(client, &fields).await?;
                client.batch_execute("COMMIT").await
            }
            .await;
            if let Err(e) = repaired {
                // The error first: a rollback that fails too says less.
                let why = refused(SETTINGS_PARTITION, &e);
                let _ = client.batch_execute("ROLLBACK").await;
                return Err(why);
            }
        }
        let tests = client
            .query(LAST_TESTS, &[])
            .await
            .map_err(|e| refused("ai_provider", &e))?;
        for row in tests {
            let provider: String = row.get(0);
            record_test(client, &provider, &object(row.get(1)), true)
                .await
                .map_err(|e| refused(&format!("{PARTITION}{provider}"), &e))?;
        }
        Ok(())
    }
}

/// Writes `row`'s provider and makes it active: nothing when it carries
/// none.
async fn activate<C: GenericClient + Sync>(
    client: &C,
    row: &BTreeMap<String, Value>,
) -> Result<(), harness_dataguard::tokio_postgres::Error> {
    let Some(id) = text(row, "aiProviderId") else {
        return Ok(());
    };
    client
        .execute(
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
    client
        .execute(
            ACTIVATE,
            &[
                &text(row, "activeProviderId").unwrap_or(id),
                &text(row, "updatedAt"),
            ],
        )
        .await?;
    Ok(())
}

/// Writes the last test `row` holds on `provider`'s row, unless that row
/// holds a later one (or, `replay`, an as late one).
async fn record_test<C: GenericClient + Sync>(
    client: &C,
    provider: &str,
    row: &BTreeMap<String, Value>,
    replay: bool,
) -> Result<(), harness_dataguard::tokio_postgres::Error> {
    let latency = row
        .get("lastTestLatencyMs")
        .and_then(Value::as_i64)
        .and_then(|ms| i32::try_from(ms).ok());
    client
        .execute(
            LAST_TEST,
            &[
                &(provider != NO_PROVIDER).then_some(provider),
                &row.get("lastTestOk").and_then(Value::as_bool),
                &text(row, "lastTestAt"),
                &text(row, "lastTestMessage"),
                &latency,
                &replay,
            ],
        )
        .await?;
    Ok(())
}

/// The provider whose last test `write` records: a write of a provider's
/// partition, `AiSettings/<uuid>`, carrying a test.
fn tested(write: &Write) -> Option<&str> {
    write
        .partition
        .strip_prefix(PARTITION)
        .filter(|id| uuid(id) && write.values.contains_key("lastTestAt"))
}

/// Why `partition` was not projected, as the author may read it: the
/// SQLSTATE and the constraint or the column, never a value. What the
/// tables refuse (integrity, class 23; a value they cannot hold, class 22)
/// rejects the command; anything else fails the step.
fn refused(partition: &str, error: &harness_dataguard::tokio_postgres::Error) -> ProjectionError {
    let Some(db) = error.as_db_error() else {
        return ProjectionError::Failed(format!(
            "{partition} not projected: the database did not answer"
        ));
    };
    let code = db.code().code();
    let what = db.constraint().or_else(|| db.column()).unwrap_or("-");
    let why = format!("{partition} not projected: {code} {what}");
    if code.starts_with("23") || code.starts_with("22") {
        ProjectionError::Refused(why)
    } else {
        ProjectionError::Failed(why)
    }
}

/// A row as the engine keeps it: a JSON object, empty when it is not one.
fn object(fields: Value) -> BTreeMap<String, Value> {
    match fields {
        Value::Object(fields) => fields.into_iter().collect(),
        _ => BTreeMap::new(),
    }
}

/// A field as text; `None` when absent, `null` or not a string.
fn text<'a>(row: &'a BTreeMap<String, Value>, field: &str) -> Option<&'a str> {
    row.get(field).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const CLOUD: &str = "a1000000-0000-4000-8000-000000000001";

    fn write(partition: &str, values: Value, tombstone: bool) -> Write {
        Write {
            partition: partition.to_string(),
            values: object(values),
            tombstone,
        }
    }

    #[test]
    fn only_a_write_of_a_provider_s_partition_carrying_a_test_records_it() {
        let test = json!({ "lastTestAt": "2026-10-17T10:00:00.000Z" });
        let cloud = format!("AiSettings/{CLOUD}");
        assert_eq!(tested(&write(&cloud, test.clone(), false)), Some(CLOUD));
        assert_eq!(
            tested(&write(
                &format!("AiSettings/{NO_PROVIDER}"),
                test.clone(),
                false
            )),
            Some(NO_PROVIDER)
        );
        assert_eq!(
            tested(&write(&cloud, json!({ "mode": "api" }), false)),
            None
        );
        assert_eq!(
            tested(&write(SETTINGS_PARTITION, test.clone(), false)),
            None
        );
        assert_eq!(
            tested(&write(&format!("Other/{CLOUD}"), test.clone(), false)),
            None
        );
        // 36 characters, hex and hyphens, but not where a uuid has them.
        for id in [
            "0123456789abcdef0123456789abcdef0123",
            "a1000000-0000-4000-8000-00000000-001",
        ] {
            assert_eq!(
                tested(&write(&format!("AiSettings/{id}"), test.clone(), false)),
                None
            );
        }
    }

    #[test]
    fn a_row_is_an_object_or_nothing() {
        assert_eq!(object(json!(["a"])), BTreeMap::new());
        assert_eq!(object(json!({ "a": 1 }))["a"], 1);
    }
}
