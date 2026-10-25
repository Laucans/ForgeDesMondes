//! The Suggestion aggregate's table, `live_suggestion`
//! (`crates/data/migrations/0011_suggestion.sql`), written by [`Table`]
//! inside the transaction that applies a write of a suggestion.
//!
//! - A suggestion is added and never rewritten: its row is inserted under the
//!   id of its partition, `Suggestion/<id>`, and a write of a suggestion the
//!   table already holds leaves it as it is.
//! - A deletion, the engine's tombstone, inserts nothing. The row a deleted
//!   suggestion left is kept: the pill's Capability leaves out what the
//!   engine deleted.
//!
//! The table's foreign keys and CHECKs judge the write a second time, before
//! the commit: a session, an element or a sound it does not hold, a text it
//! cannot read as a uuid or an instant, rejects the command, whose write rolls
//! back with its projection.

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{Error, Transaction};
use serde_json::Value;

/// The partitions of the Suggestion aggregate: `Suggestion/<id>`.
const PARTITION: &str = "Suggestion/";

/// The suggestion `$1` names, once: its target is the element or the sound
/// the row names, its instant read as text by the table. Every value is bound
/// as text and cast here, so that a text which is no uuid is the table's
/// refusal, not a failure of the step.
const INSERT: &str = "INSERT INTO live_suggestion
        (id, session_id, entity_id, sound_id, matched_text, detected_at)
    VALUES ($1::text::uuid, $2::text::uuid, $3::text::uuid, $4::text::uuid, $5::text,
        $6::text::timestamptz)
    ON CONFLICT (id) DO NOTHING";

/// The Suggestion aggregate's projection into its table.
pub struct Table;

#[async_trait]
impl ProjectPg for Table {
    /// The suggestion the write leaves, as one `live_suggestion` row; a
    /// tombstone writes nothing.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        if write.tombstone {
            return Ok(());
        }
        let id = write.partition.strip_prefix(PARTITION);
        let text = |field: &str| write.values.get(field).and_then(Value::as_str);
        tx.execute(
            INSERT,
            &[
                &id,
                &text("sessionId"),
                &text("entityId"),
                &text("soundId"),
                &text("matchedText"),
                &text("detectedAt"),
            ],
        )
        .await
        .map_err(judged)?;
        Ok(())
    }
}

/// What the table refuses — a foreign key, a NOT NULL (class 23), a value it
/// cannot hold (class 22) — rejects the command; any other error fails the
/// step.
fn judged(error: Error) -> ProjectionError {
    match error.as_db_error() {
        Some(db)
            if ["22", "23"]
                .iter()
                .any(|class| db.code().code().starts_with(class)) =>
        {
            ProjectionError::Refused(db.message().to_owned())
        }
        _ => error.into(),
    }
}
