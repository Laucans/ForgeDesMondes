//! DataCapability `soundbox.updateSound` (`data-capability.json`): the DM
//! edits a sound of the library — renames it, points it to another file,
//! retags it or changes its own volume. The edit is one command on
//! `Sound/<its id>`, the sound's partition, and [`prepare`] turns the wire
//! payload of the edit form into the fields it writes:
//!
//! - only the fields the payload names are written: one it leaves out stays
//!   as it is;
//! - its tags are normalised as `SoundTag@1` says ([`normalized`]), as on an
//!   addition, so « Porte » and « porte » are one tag;
//! - its volume is given in percent and stored as a fraction of 1, to the
//!   hundredth the `sound` table holds. One out of 0–100, or no number, is
//!   still submitted: the invariant `sound-volume-range` rejects it and
//!   nothing is applied;
//! - what the `sound` table could never hold — no sound named, a blank name
//!   or file, nothing to change — is refused before anything is queued
//!   ([`Refusal`]), and so is a sound neither the engine nor the tables
//!   hold, 404.
//!
//! Two edits of one sound queue on its partition: the DataGuard asks the
//! author of the second to confirm it overwrites the first
//! (`confirm_on_stale`), and never rejects it for that.
//!
//! Over PostgreSQL, the plugin also projects the edit onto the sound's row
//! of the `sound` table ([`Edits`]) — its name, file, tags and volume, no
//! other column — and answers a sound only the tables hold, a fixture's, as
//! the engine's row, so it is judged and edited whole.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare, ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{self, Client, GenericClient, Transaction};
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "soundbox.updateSound";

/// The aggregate it writes.
const AGGREGATE: &str = "Sound";

/// The partitions of the aggregate: `Sound/<sound id>`.
const PARTITION: &str = "Sound/";

/// The full volume, in percent.
const FULL_VOLUME: f64 = 100.0;

/// Whether the `sound` table exists: a database not migrated yet holds none.
const MIGRATED: &str = "SELECT to_regclass('sound') IS NOT NULL";

/// A sound as its table holds it, under the aggregate's fields, `$1` its id
/// in whatever case, a uuid, read by its primary key:
/// what the engine judges an edit against when it never wrote the sound
/// itself — the fixtures' sounds.
const SOUND_ROW: &str = "SELECT jsonb_build_object(
        'id', s.id, 'campaignId', s.campaign_id, 'name', s.name, 'filePath', s.file_path,
        'tags', to_jsonb(s.tags), 'volume', s.volume, 'createdAt', s.created_at)
    FROM sound s WHERE s.id = $1::text::uuid";

/// The name, file, tags and volume the engine holds on the sound partition
/// `$1` names — on every one when it is null — written on its row when they
/// are not that already: the four columns an edit writes, no other. A field
/// the engine's row leaves out, or holds as something its column could not
/// take, keeps the row's. After each apply of a sound, and at a start, so an
/// edit the table lost is written again; a sound removed (a tombstone) is
/// never written. One partition's sound, a uuid, is found by its primary
/// key.
const EDITS: &str = "UPDATE sound s
    SET name = coalesce(n.name, s.name), file_path = coalesce(n.file_path, s.file_path),
        tags = coalesce(n.tags, s.tags), volume = coalesce(n.volume, s.volume)
    FROM dataguard_rows r
    CROSS JOIN LATERAL (SELECT
        CASE WHEN jsonb_typeof(r.fields -> 'name') = 'string' THEN r.fields ->> 'name' END AS name,
        CASE WHEN jsonb_typeof(r.fields -> 'filePath') = 'string'
            THEN r.fields ->> 'filePath' END AS file_path,
        CASE WHEN jsonb_typeof(r.fields -> 'tags') = 'array' THEN
            CASE WHEN NOT EXISTS (SELECT FROM jsonb_array_elements(r.fields -> 'tags') AS t
                    WHERE jsonb_typeof(t) <> 'string')
                THEN ARRAY(SELECT jsonb_array_elements_text(r.fields -> 'tags')) END
        END AS tags,
        CASE WHEN pg_input_is_valid(r.fields ->> 'volume', 'numeric(3, 2)') THEN
            CASE WHEN (r.fields ->> 'volume')::numeric(3, 2) BETWEEN 0 AND 1
                THEN (r.fields ->> 'volume')::numeric(3, 2) END
        END AS volume) AS n
    WHERE r.partition = 'Sound/' || s.id::text AND NOT r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text
            AND s.id = substr($1::text, length('Sound/') + 1)::uuid)
        AND (s.name, s.file_path, s.tags, s.volume) IS DISTINCT FROM
            (coalesce(n.name, s.name), coalesce(n.file_path, s.file_path),
             coalesce(n.tags, s.tags), coalesce(n.volume, s.volume))";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Why an edit is refused before it is queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// It names no sound, or one the product does not hold.
    NoSound,
    /// Its name is given blank, or is no text.
    NoName,
    /// Its file is given blank, or is no text.
    NoFile,
    /// It names none of the name, the file, the tags and the volume.
    Nothing,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoSound => "ce son n'existe pas",
            Self::NoName => "un son a besoin d'un nom",
            Self::NoFile => "un son a besoin de son fichier",
            Self::Nothing => "rien à modifier sur ce son",
        })
    }
}

impl From<Refusal> for plugins::Refusal {
    /// No sound is a 404; anything else the table could not hold, a 422.
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::NoSound => Self::not_found(refusal.to_string()),
            _ => Self::unprocessable(refusal.to_string()),
        }
    }
}

/// What an edit becomes before it is submitted: the fields it writes on
/// `Sound/<target_id>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The fields it changes, under the Sound aggregate's names.
    pub payload: Row,
    /// The sound's id, trimmed and lowercase: the command's target.
    pub target_id: String,
}

/// What the DataCapability plugs into the engine: its `Prepare`, and over
/// PostgreSQL the edit's projection onto the `sound` table.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .prepare(DATA_CAPABILITY, Editing)
        .project_pg(AGGREGATE, Edits)
}

/// The edit's `Prepare`: the form's fields made the sound's, on a sound the
/// engine or the tables hold.
struct Editing;

#[async_trait]
impl Prepare for Editing {
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

/// The fields an edit of `wire` writes on the sound `target` names — the
/// command's target, else the payload's `id` — trimmed and lowercase as the
/// `sound` table writes its ids.
///
/// # Errors
///
/// [`Refusal`] when no sound is named, the name or the file is given blank,
/// or nothing is given to change.
pub fn prepare(wire: &Row, target: Option<&str>) -> Result<Prepared, Refusal> {
    let target_id = target
        .or_else(|| wire.get("id").and_then(Value::as_str))
        .map(|id| id.trim().to_lowercase())
        .filter(|id| !id.is_empty())
        .ok_or(Refusal::NoSound)?;
    let mut payload = Row::new();
    if wire.contains_key("name") {
        payload.insert(
            "name".into(),
            text(wire, "name").ok_or(Refusal::NoName)?.into(),
        );
    }
    if wire.contains_key("file") {
        let file = text(wire, "file").ok_or(Refusal::NoFile)?;
        payload.insert("filePath".into(), file.into());
    }
    if let Some(given) = wire.get("tags") {
        payload.insert("tags".into(), tags(given));
    }
    if let Some(given) = wire.get("volume") {
        payload.insert("volume".into(), volume(given));
    }
    if payload.is_empty() {
        return Err(Refusal::Nothing);
    }
    Ok(Prepared { payload, target_id })
}

/// `tags` normalised as `SoundTag@1` says: each tag trimmed and lowercased,
/// character by character, a tag left empty dropped, then a tag seen earlier
/// dropped — the first occurrence stays, the order is kept.
#[must_use]
pub fn normalized<'a>(tags: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for tag in tags {
        let tag: String = tag.trim().chars().flat_map(char::to_lowercase).collect();
        if !tag.is_empty() && !kept.contains(&tag) {
            kept.push(tag);
        }
    }
    kept
}

/// The tags written: none when `null` is given, the list normalised when it
/// is one of texts. Anything else is kept as given, and `tags-normalized`
/// rejects it.
fn tags(given: &Value) -> Value {
    match given {
        Value::Null => Value::Array(Vec::new()),
        Value::Array(tags) => match tags.iter().map(Value::as_str).collect::<Option<Vec<_>>>() {
            Some(tags) => normalized(tags).into(),
            None => given.clone(),
        },
        other => other.clone(),
    }
}

/// The volume written, a fraction of 1: the percent given — a number, or the
/// text of one — divided by 100. One from 0 to 100 is kept to the whole
/// percent, the hundredth `sound.volume` holds; one out of range is kept
/// unrounded, and anything else as given, for `sound-volume-range` to
/// reject.
fn volume(given: &Value) -> Value {
    let percent = match given {
        Value::Number(n) => n.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    let Some(percent) = percent.filter(|percent| percent.is_finite()) else {
        return given.clone();
    };
    let percent = if (0.0..=FULL_VOLUME).contains(&percent) {
        percent.round()
    } else {
        percent
    };
    (percent / FULL_VOLUME).into()
}

/// A field as trimmed text; `None` when not a string or blank.
fn text(wire: &Row, field: &str) -> Option<String> {
    wire.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// The edits in the `sound` table: written from the engine's row after each
/// apply of a sound, and again at a start for every one the table lost; a
/// sound only the table holds answered as the engine's row.
struct Edits;

/// Whether `client`'s database has the `sound` table.
async fn migrated(client: &impl GenericClient) -> Result<bool, ProjectionError> {
    Ok(client.query_one(MIGRATED, &[]).await?.get(0))
}

/// Writes the edits of the partition `partition` names — of every one,
/// `None` — on the sounds' rows, when the table exists.
async fn write_edits(
    client: &impl GenericClient,
    partition: Option<&str>,
) -> Result<(), ProjectionError> {
    if migrated(client).await? {
        client
            .execute(EDITS, &[&partition])
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
impl ProjectPg for Edits {
    /// The edit of the sound written, on its row; a removal writes nothing
    /// (`EDITS` skips a tombstone), nor does an addition the table does not
    /// hold yet, nor a partition whose id the table could never hold.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        match write.partition.strip_prefix(PARTITION) {
            Some(id) if uuid(id) => write_edits(tx, Some(&write.partition)).await,
            _ => Ok(()),
        }
    }

    /// Every edit the table lost, written again: one whose write failed
    /// before this projection ran in the apply's transaction.
    async fn warm(&self, client: &Client) -> Result<(), ProjectionError> {
        write_edits(client, None).await
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
