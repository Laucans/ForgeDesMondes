//! A removed sound in the product's tables (`crates/data/migrations/
//! 0007_sound.sql`), written by [`Removals`] inside the transaction that
//! applies its tombstone.
//!
//! The product reads its tables, not the engine: a sound removed is taken
//! out of `sound`, so no reader sees it, and its playbacks go with it
//! (`sound_playback.sound_id`, `ON DELETE CASCADE`, as `SoundPlayback.
//! soundId` declares). The engine follows the same cascade: the playbacks
//! it holds of the sound become tombstones too, kept as they were, so it
//! never holds a playback of a sound it removed. The tombstone in
//! `dataguard_rows` keeps the sound as it was: the engine keeps the fields
//! of a row it wrote, and a sound only the tables held — the fixtures',
//! written by no command — gets them from its table row, which would be
//! lost otherwise. A write that is no tombstone, or of a partition whose id
//! is no uuid, writes nothing here; nor does a database whose `sound` table
//! is not migrated yet, which holds no sound.
//!
//! A sound only the tables hold is the engine's row as the table holds it
//! ([`Removals`]' `hydrate`): what a removal is judged against.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::types::ToSql;
use harness_dataguard::tokio_postgres::{Client, Error, GenericClient, Transaction};
use serde_json::Value;

use crate::{PARTITION, uuid};

/// A `sound` row `s` under the Sound aggregate's names, as the engine
/// holds a sound it added.
macro_rules! fields {
    () => {
        "jsonb_build_object('id', s.id::text, 'campaignId', s.campaign_id::text,
            'name', s.name, 'filePath', s.file_path, 'tags', to_jsonb(s.tags),
            'volume', s.volume, 'createdAt', s.created_at)"
    };
}

/// The playbacks of the sound `$1` the engine holds, made tombstones as the
/// table cascades them: read before the sound leaves the table, found by
/// the table's index on `sound_id` and the engine's primary key.
const REMOVE_PLAYBACKS: &str = "UPDATE dataguard_rows r SET tombstoned = true
    FROM sound_playback p
    WHERE p.sound_id = $1::text::uuid AND r.partition = 'SoundPlayback/' || p.id::text
        AND NOT r.tombstoned";

/// The sound `$1` taken out of `sound`, its playbacks with it; the
/// tombstone of its partition `$2` given its fields when the engine held
/// none of them.
const REMOVE_SOUND: &str = concat!(
    "WITH gone AS (DELETE FROM sound s WHERE s.id = $1::text::uuid RETURNING ",
    fields!(),
    " AS fields)
    UPDATE dataguard_rows r SET fields = gone.fields FROM gone
    WHERE r.partition = $2::text AND r.tombstoned AND r.fields = '{}'::jsonb"
);

/// Whether the `sound` table exists: a database not migrated yet holds
/// none.
const MIGRATED: &str = "SELECT to_regclass('sound') IS NOT NULL";

/// The sound `$1` as its table holds it, under the aggregate's names.
const SOUND_ROW: &str = concat!(
    "SELECT ",
    fields!(),
    " FROM sound s WHERE s.id = $1::text::uuid"
);

/// The Sound aggregate's projection of a removed sound out of `sound`, and
/// the fixtures' sounds hydrated.
pub struct Removals;

/// Whether `client`'s database has the `sound` table.
async fn migrated(client: &impl GenericClient) -> Result<bool, ProjectionError> {
    Ok(client.query_one(MIGRATED, &[]).await?.get(0))
}

/// The sound id `partition` names, when it is a uuid.
fn sound(partition: &str) -> Option<String> {
    partition.strip_prefix(PARTITION).and_then(uuid)
}

#[async_trait]
impl ProjectPg for Removals {
    /// A tombstone's sound taken out of the table; any other write nothing.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        let Some(id) = sound(&write.partition).filter(|_| write.tombstone) else {
            return Ok(());
        };
        if !migrated(tx).await? {
            return Ok(());
        }
        for (sql, params) in [
            (REMOVE_PLAYBACKS, &[&id as &(dyn ToSql + Sync)][..]),
            (REMOVE_SOUND, &[&id, &write.partition]),
        ] {
            tx.execute(sql, params)
                .await
                .map_err(|error| judged(&write.partition, &error))?;
        }
        Ok(())
    }

    async fn hydrate(
        &self,
        partition: &str,
        client: &Client,
    ) -> Result<Option<BTreeMap<String, Value>>, ProjectionError> {
        let Some(id) = sound(partition) else {
            return Ok(None);
        };
        if !migrated(client).await? {
            return Ok(None);
        }
        let Some(row) = client.query_opt(SOUND_ROW, &[&id]).await? else {
            return Ok(None);
        };
        Ok(match row.get::<_, Value>(0) {
            Value::Object(fields) => Some(fields.into_iter().collect()),
            _ => None,
        })
    }
}

/// Why `partition` was not projected, as the author may read it: the
/// SQLSTATE and the constraint, never a value. What the tables refuse
/// (integrity, class 23) rejects the command; anything else fails the step.
fn judged(partition: &str, error: &Error) -> ProjectionError {
    let Some(db) = error.as_db_error() else {
        return ProjectionError::Failed(format!(
            "{partition} not removed: the database did not answer"
        ));
    };
    let code = db.code().code();
    let why = format!(
        "{partition} not removed: {code} {}",
        db.constraint().unwrap_or("-")
    );
    if code.starts_with("23") {
        ProjectionError::Refused(why)
    } else {
        ProjectionError::Failed(why)
    }
}
