//! DataCapability `soundbox.setSoundVolume` (`data-capability.json`): during
//! a session the DM moves one sound's slider and only that sound gets louder
//! or quieter — what it plays at is its own volume times the campaign's
//! master volume (EffectiveVolume@1), so the others keep theirs. The slider
//! speaks in percent, from 0 to 100; the sound stores a fraction from 0 to
//! 1, the percent divided by 100, as `volume` — one command on
//! `Sound/<its id>`, which writes nothing else of it.
//!
//! [`prepare`] turns what the Micro-UI sends — `{volume}` on the command's
//! target, the sound, or `{sound, volume}` — into that command's payload and
//! target:
//!
//! - the target is the sound, trimmed and lowercase as the `sound` table
//!   writes its ids — one partition per sound whatever case the request
//!   used;
//! - its payload is `volume`, the percent divided by 100, and nothing else —
//!   to the whole percent, the hundredths `sound.volume` holds, so what the
//!   engine judges is what the table keeps;
//! - a volume out of range is never clamped nor rounded into range: its
//!   fraction is written as it is, and the sound's own invariant refuses it
//!   (`sound-volume-range`); a volume that is no number is written `null`,
//!   which the invariant refuses too;
//! - a request naming no sound, or one neither the engine nor the tables
//!   hold, is refused before anything is queued, 404 ([`Refusal`]).
//!
//! Two moves of one slider queue on the sound's partition and the last one
//! wins (`overwrite`): a slider's value replaces the one before it, nobody
//! is asked to confirm it.
//!
//! Over PostgreSQL, the plugin also projects the volume onto the sound's row
//! of the `sound` table ([`Volumes`]) — that column, no other — and answers
//! a sound only the tables hold, a fixture's, as the engine's row, so it is
//! judged and written whole.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare, ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{self, Client, GenericClient, Transaction};
use serde_json::{Number, Value};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "soundbox.setSoundVolume";

/// The aggregate it writes.
const AGGREGATE: &str = "Sound";

/// The partitions of the aggregate: `Sound/<sound id>`.
const PARTITION: &str = "Sound/";

/// The one field it writes: the sound's own volume, a fraction from 0 to 1.
pub const VOLUME: &str = "volume";

/// What a percent is divided by to be stored.
const PERCENT: f64 = 100.0;

/// Whether the `sound` table exists: a database not migrated yet holds none.
const MIGRATED: &str = "SELECT to_regclass('sound') IS NOT NULL";

/// A sound as its table holds it, under the aggregate's fields, `$1` its id
/// in whatever case, a uuid, read by its primary key: what the engine judges
/// a change against when it never wrote the sound itself — the fixtures'
/// sounds.
const SOUND_ROW: &str = "SELECT jsonb_build_object(
        'id', s.id, 'campaignId', s.campaign_id, 'name', s.name, 'filePath', s.file_path,
        'tags', to_jsonb(s.tags), 'volume', s.volume, 'createdAt', s.created_at)
    FROM sound s WHERE s.id = $1::text::uuid";

/// The volume the engine holds on the sound partition `$1` names — on every
/// one when it is null — written on its row when it is not that already: the
/// one column a change of it writes. After each apply of a sound, and at a
/// start, so a volume the table lost is written again; a sound removed (a
/// tombstone) is never written. A volume the column could not hold, or
/// outside `sound-volume-range`, is left aside, so the row keeps its own.
/// One partition's sound, a uuid, is found by its primary key.
const VOLUMES: &str = "UPDATE sound s
    SET volume = n.volume
    FROM dataguard_rows r
    CROSS JOIN LATERAL (SELECT
        CASE WHEN pg_input_is_valid(r.fields ->> 'volume', 'numeric(3, 2)') THEN
            CASE WHEN (r.fields ->> 'volume')::numeric(3, 2) BETWEEN 0 AND 1
                THEN (r.fields ->> 'volume')::numeric(3, 2) END
        END AS volume) AS n
    WHERE r.partition = 'Sound/' || s.id::text AND NOT r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text
            AND s.id = substr($1::text, length('Sound/') + 1)::uuid)
        AND n.volume IS NOT NULL AND s.volume IS DISTINCT FROM n.volume";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Why a change of a sound's volume is refused before it is queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// It names no sound, or one the product does not hold.
    NoSound,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSound => f.write_str("ce son n'existe pas"),
        }
    }
}

impl From<Refusal> for plugins::Refusal {
    /// No sound is a 404.
    fn from(refusal: Refusal) -> Self {
        Self::not_found(refusal.to_string())
    }
}

/// What a change of a sound's volume becomes before it is submitted: what
/// the command writes on `Sound/<target_id>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The fields written: the volume alone.
    pub payload: Row,
    /// The sound's id, trimmed and lowercase: the command's target.
    pub target_id: String,
}

/// What the DataCapability plugs into the engine: its `Prepare`, and over
/// PostgreSQL the volume's projection onto the `sound` table.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .prepare(DATA_CAPABILITY, Setting)
        .project_pg(AGGREGATE, Volumes)
}

/// The change's `Prepare`: the percent made the sound's fraction, on a sound
/// the engine or the tables hold.
struct Setting;

#[async_trait]
impl Prepare for Setting {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let prepared = prepare(payload, target_id)?;
        let held = context
            .row(&format!("{PARTITION}{}", prepared.target_id))
            .await?;
        if held.is_empty() {
            return Err(Refusal::NoSound.into());
        }
        Ok(plugins::Prepared {
            payload: prepared.payload,
            target_id: Some(prepared.target_id),
            ..plugins::Prepared::default()
        })
    }
}

/// The change `wire` asks for — `{volume}`, in percent — on the sound
/// `target` names, else the one its `sound` does.
///
/// # Errors
///
/// [`Refusal::NoSound`] when neither names a sound.
pub fn prepare(wire: &Row, target: Option<&str>) -> Result<Prepared, Refusal> {
    let target_id = named(target)
        .or_else(|| named(wire.get("sound").and_then(Value::as_str)))
        .ok_or(Refusal::NoSound)?
        .to_lowercase();
    let volume = wire
        .get(VOLUME)
        .and_then(Value::as_f64)
        .map(|percent| {
            let percent = if (0.0..=PERCENT).contains(&percent) {
                percent.round()
            } else {
                percent
            };
            percent / PERCENT
        })
        .and_then(Number::from_f64)
        .map_or(Value::Null, Value::Number);
    Ok(Prepared {
        payload: Row::from([(VOLUME.to_owned(), volume)]),
        target_id,
    })
}

/// `id`, trimmed, when it names something.
fn named(id: Option<&str>) -> Option<&str> {
    id.map(str::trim).filter(|id| !id.is_empty())
}

/// The volumes in the `sound` table: written from the engine's row after
/// each apply of a sound, and again at a start for every one the table lost;
/// a sound only the table holds answered as the engine's row.
struct Volumes;

/// Whether `client`'s database has the `sound` table.
async fn migrated(client: &impl GenericClient) -> Result<bool, ProjectionError> {
    Ok(client.query_one(MIGRATED, &[]).await?.get(0))
}

/// Writes the volume of the partition `partition` names — of every one,
/// `None` — on the sounds' rows, when the table exists.
async fn write_volumes(
    client: &impl GenericClient,
    partition: Option<&str>,
) -> Result<(), ProjectionError> {
    if migrated(client).await? {
        client
            .execute(VOLUMES, &[&partition])
            .await
            .map_err(projection_error)?;
    }
    Ok(())
}

/// A write the table refuses — its integrity (class 23) or a value it cannot
/// read (class 22) — is the command's fault: refused. Anything else is the
/// store's.
fn projection_error(error: tokio_postgres::Error) -> ProjectionError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") || db.code().code().starts_with("22") => {
            ProjectionError::Refused(format!("sound: {}", db.message()))
        }
        _ => ProjectionError::from(error),
    }
}

#[async_trait]
impl ProjectPg for Volumes {
    /// The volume of the sound written, on its row; a removal writes nothing
    /// (`VOLUMES` skips a tombstone), nor does a partition whose id the table
    /// could never hold.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        match write.partition.strip_prefix(PARTITION) {
            Some(id) if uuid(id) => write_volumes(tx, Some(&write.partition)).await,
            _ => Ok(()),
        }
    }

    /// Every volume the table lost, written again: one whose write failed
    /// before this projection ran in the apply's transaction.
    async fn warm(&self, client: &Client) -> Result<(), ProjectionError> {
        write_volumes(client, None).await
    }

    /// A sound only the tables hold — a fixture's — as its table holds it;
    /// an id that is no uuid names none.
    async fn hydrate(
        &self,
        partition: &str,
        client: &Client,
    ) -> Result<Option<Row>, ProjectionError> {
        let Some(id) = partition.strip_prefix(PARTITION).filter(|id| uuid(id)) else {
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

/// Whether `text` is a uuid, in whatever case: what the `sound` table's ids
/// are. Checked before a statement casts it, which PostgreSQL may fold
/// ahead of any guard the statement holds.
fn uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(at, c)| match at {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}
