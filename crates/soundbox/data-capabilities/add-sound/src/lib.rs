//! DataCapability `soundbox.addSound` (`data-capability.json`): the DM adds a
//! sound to a campaign's library — its name, its file, its tags, its own
//! volume. The sound is one command on `Sound/<its id>`, a new partition, and
//! [`prepare`] turns the wire payload of the screen into the row it writes:
//!
//! - the sound is given a fresh id ([`Fresh`]): an addition never names an
//!   existing sound;
//! - its tags are normalised as `SoundTag@1` says ([`normalized`]), so
//!   « Taverne » and « taverne » are one tag (`tags-normalized`);
//! - its volume is given in percent and stored as a fraction of 1, to the
//!   hundredth the `sound` table holds — full volume when none is given. One
//!   out of 0–100, or no number, is still submitted: the invariant
//!   `sound-volume-range` rejects it and nothing is applied;
//! - what the `sound` table could never hold — no campaign, no name, no
//!   file, a text holding U+0000 — is refused before anything is queued
//!   ([`Refusal`], 422), and so is a campaign the product does not hold (404).
//!
//! Its `Prepare` ([`plugin`]) reads the campaign over PostgreSQL as the
//! tables hold it — the fixtures' among them — and over the engine's memory,
//! which has no table, as the engine's row. Its projection ([`tables`])
//! writes the added sound's `sound` row in the transaction that applies it.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;

pub mod tables;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "soundbox.addSound";

/// Its target aggregate, as `data-capability.json` names it.
pub const AGGREGATE: &str = "Sound";

/// The partitions of the Campaign aggregate, which a sound belongs to.
const CAMPAIGN: &str = "Campaign/";

/// The volume a sound takes when none is given, in percent: full volume, as
/// the `sound` table's default.
const FULL_VOLUME: f64 = 100.0;

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Why an addition is refused before it is queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// It names no campaign.
    NoCampaign,
    /// The campaign it names is none the product holds.
    UnknownCampaign,
    /// Its name is missing or blank.
    NoName,
    /// Its file is missing or blank.
    NoFile,
    /// A text of it holds U+0000, which PostgreSQL's `text` cannot hold.
    Nul(&'static str),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCampaign => f.write_str("un son nomme sa campagne"),
            Self::UnknownCampaign => f.write_str("cette campagne n'existe pas"),
            Self::NoName => f.write_str("un son a besoin d'un nom"),
            Self::NoFile => f.write_str("un son a besoin de son fichier"),
            Self::Nul(field) => write!(f, "le champ {field} contient un caractère nul"),
        }
    }
}

/// What an addition is given beside its payload: the new sound's id, and
/// the time it is added at (RFC 3339).
#[derive(Clone, Copy, Debug)]
pub struct Fresh<'a> {
    /// The sound's id.
    pub sound: &'a str,
    /// Now.
    pub now: &'a str,
}

/// The campaign the payload names, trimmed and lowercase as the `campaign`
/// table writes its ids; `None` when it names none.
#[must_use]
pub fn campaign_id(wire: &Row) -> Option<String> {
    wire.get("campaign")
        .and_then(Value::as_str)
        .map(|id| id.trim().to_lowercase())
        .filter(|id| !id.is_empty())
}

/// The row an addition of `wire` writes, under the Sound aggregate's names,
/// the sound being `fresh.sound`: its `id` names its partition (the
/// manifest's `target.id`, `$id`).
///
/// # Errors
///
/// [`Refusal`] when `wire` names no campaign, no name or no file, or holds
/// U+0000 in a text.
pub fn prepare(wire: &Row, fresh: Fresh<'_>) -> Result<Row, Refusal> {
    let campaign = campaign_id(wire).ok_or(Refusal::NoCampaign)?;
    let name = text(wire, "name").ok_or(Refusal::NoName)?;
    let file = text(wire, "file").ok_or(Refusal::NoFile)?;
    let tags = tags(wire.get("tags"));
    let nul = |text: &str| text.contains('\0');
    for (field, found) in [
        ("campaign", nul(&campaign)),
        ("name", nul(&name)),
        ("file", nul(&file)),
        (
            "tags",
            tags.as_array()
                .is_some_and(|tags| tags.iter().filter_map(Value::as_str).any(nul)),
        ),
    ] {
        if found {
            return Err(Refusal::Nul(field));
        }
    }
    Ok(Row::from([
        ("id".to_owned(), fresh.sound.into()),
        ("campaignId".to_owned(), campaign.into()),
        ("name".to_owned(), name.into()),
        ("filePath".to_owned(), file.into()),
        ("tags".to_owned(), tags),
        ("volume".to_owned(), volume(wire.get("volume"))),
        ("createdAt".to_owned(), fresh.now.into()),
    ]))
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

/// The tags written: none when none are given, the list normalised when it
/// is one of texts. Anything else is kept as given, and `tags-normalized`
/// rejects it.
fn tags(given: Option<&Value>) -> Value {
    match given {
        None | Some(Value::Null) => Value::Array(Vec::new()),
        Some(Value::Array(tags)) => {
            match tags.iter().map(Value::as_str).collect::<Option<Vec<_>>>() {
                Some(tags) => normalized(tags).into(),
                None => Value::Array(tags.clone()),
            }
        }
        Some(other) => other.clone(),
    }
}

/// The volume written, a fraction of 1: the percent given — a number, or the
/// text of one — divided by 100, full volume when none is given. One from 0
/// to 100 is kept to the whole percent, the hundredth `sound.volume` holds;
/// one out of range is kept unrounded, and anything else as given, for
/// `sound-volume-range` to reject.
fn volume(given: Option<&Value>) -> Value {
    let percent = match given {
        None | Some(Value::Null) => Some(FULL_VOLUME),
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        Some(_) => None,
    };
    let Some(percent) = percent.filter(|percent| percent.is_finite()) else {
        return given.cloned().unwrap_or(Value::Null);
    };
    let percent = if (0.0..=FULL_VOLUME).contains(&percent) {
        percent.round()
    } else {
        percent
    };
    (percent / 100.0).into()
}

/// A field as trimmed text; `None` when absent, not a string or blank.
fn text(wire: &Row, field: &str) -> Option<String> {
    wire.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// What the DataCapability plugs into the engine: its `Prepare`, and the
/// projection of an added sound into `sound`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .prepare(DATA_CAPABILITY, AddSound)
        .project_pg(AGGREGATE, tables::Sounds)
}

/// A live campaign the tables hold, `$1` its id in whatever case. A
/// campaign the engine deleted (its tombstone) is none.
const HELD_CAMPAIGN: &str = "SELECT c.id::text AS id FROM campaign c WHERE c.id::text = lower($1)
    AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
        WHERE r.partition = 'Campaign/' || c.id::text AND r.tombstoned)";

/// An addition made the row it writes on a new sound's partition: checked
/// first, then its campaign read. What the tables could never hold is
/// refused, 422; a campaign the product does not hold, 404.
struct AddSound;

#[async_trait]
impl Prepare for AddSound {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &Row,
        _target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let sound = context.ids.fresh();
        let now = stamp(context.now_millis);
        let row = prepare(
            payload,
            Fresh {
                sound: &sound,
                now: &now,
            },
        )
        .map_err(refused)?;
        let campaign = row["campaignId"].as_str().unwrap_or_default();
        if !held(context, campaign).await? {
            return Err(refused(Refusal::UnknownCampaign));
        }
        Ok(plugins::Prepared {
            payload: row,
            ..plugins::Prepared::default()
        })
    }
}

/// An addition refused: 404 for a campaign the product does not hold, 422
/// for anything else, saying why.
fn refused(refusal: Refusal) -> plugins::Refusal {
    match refusal {
        Refusal::UnknownCampaign => plugins::Refusal::not_found(refusal.to_string()),
        _ => plugins::Refusal::unprocessable(refusal.to_string()),
    }
}

/// Whether the product holds the campaign `id` names, live: over PostgreSQL
/// as the tables hold it, over the engine's memory as its row.
async fn held(context: &Context<'_>, id: &str) -> Result<bool, plugins::Refusal> {
    let Some(rows) = context.rows else {
        return Ok(!context.row(&format!("{CAMPAIGN}{id}")).await?.is_empty());
    };
    let found = rows
        .rows(HELD_CAMPAIGN, &[Value::from(id)])
        .await
        .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
    Ok(found.as_array().is_some_and(|found| !found.is_empty()))
}

/// `millis` since the epoch as RFC 3339 in UTC, to the millisecond
/// (`2026-10-21T09:00:00.123Z`): what `sound.created_at` takes.
#[must_use]
pub fn stamp(millis: u64) -> String {
    let unix_seconds = millis / 1000;
    // Howard Hinnant's civil_from_days: the days since the epoch, a u64
    // divided by 86 400, always fit an i64.
    let days = (unix_seconds / 86_400).cast_signed() + 719_468;
    let seconds = unix_seconds % 86_400;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60,
        millis % 1000
    )
}
