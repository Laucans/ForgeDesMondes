//! An added sound in the product's tables (`crates/data/migrations/
//! 0007_sound.sql`), written by [`Sounds`] inside the transaction that
//! applies it.
//!
//! A write that leaves a live sound the `sound` table does not hold adds its
//! row, as the engine holds it after the write: its campaign, name, file,
//! tags and volume, and the time it was added at. A sound the table holds
//! already is never written again here, whatever the write: an edit, a
//! volume or a removal is its own DataCapability's to project. A tombstone,
//! or a row whose id is no uuid, writes nothing.
//!
//! The table judges the row a second time — its CHECKs under the
//! invariants' ids (`tags_normalized`, `sound_volume_range`) and its
//! campaign's foreign key: what it refuses rejects the command, whose write
//! rolls back with its projection.

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{Error, Transaction};

/// The live sound the partition `$1` holds, as the engine holds it, added to
/// `sound` when the table holds no row of its id, its tags in their order.
/// A row whose id is no uuid, or not its partition's, is none the table
/// could hold.
const ADD_SOUND: &str =
    "INSERT INTO sound (id, campaign_id, name, file_path, tags, volume, created_at)
    SELECT (v ->> 'id')::uuid, (v ->> 'campaignId')::uuid, v ->> 'name', v ->> 'filePath',
        ARRAY(SELECT tag FROM jsonb_array_elements_text(v -> 'tags') WITH ORDINALITY AS t(tag, n)
            ORDER BY n),
        (v ->> 'volume')::numeric, (v ->> 'createdAt')::timestamptz
    FROM dataguard_rows r
    CROSS JOIN LATERAL (SELECT r.fields::jsonb AS v) added
    WHERE r.partition = $1::text AND NOT r.tombstoned
        AND r.partition = 'Sound/' || (v ->> 'id')
        AND pg_input_is_valid(v ->> 'id', 'uuid')
        AND NOT EXISTS (SELECT 1 FROM sound s WHERE s.id = CASE
            WHEN pg_input_is_valid(v ->> 'id', 'uuid') THEN (v ->> 'id')::uuid END)";

/// The Sound aggregate's projection of an added sound into `sound`.
pub struct Sounds;

#[async_trait]
impl ProjectPg for Sounds {
    /// A live sound the table does not hold, added; any other write —
    /// a tombstone, a sound the table holds — nothing.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        tx.execute(ADD_SOUND, &[&write.partition])
            .await
            .map(drop)
            .map_err(|error| judged(&write.partition, &error))
    }
}

/// Why `partition` was not projected, as the author may read it: the
/// SQLSTATE and the constraint or the column, never a value. What the table
/// refuses (integrity, class 23; a value it cannot hold, class 22) rejects
/// the command; anything else fails the step.
fn judged(partition: &str, error: &Error) -> ProjectionError {
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
