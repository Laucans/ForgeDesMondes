//! DataCapability `soundbox.addSound` (`data-capability.json`): the DM adds a
//! sound to a campaign's library — its name, its file, its tags, its own
//! volume. The sound is one command on `Sound/<its id>`, a new partition, and
//! [`prepare`] turns the wire payload of the screen into the row it writes:
//!
//! - the sound is given its id by the server ([`Fresh`]): an addition never
//!   names an existing sound;
//! - its tags are normalised as `SoundTag@1` says ([`normalized`]), so
//!   « Taverne » and « taverne » are one tag;
//! - its volume is given in percent and stored as a fraction of 1, to the
//!   hundredth the `sound` table holds — full volume when none is given. One
//!   out of 0–100, or no number, is still submitted: the invariant
//!   `sound-volume-range` rejects it and nothing is applied;
//! - what the `sound` table could never hold — no name, no file, no campaign
//!   — is refused before anything is queued ([`Refusal`]). Whether the
//!   campaign exists is the server's to ask, against the rows it holds.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "soundbox.addSound";

/// The volume a sound takes when none is given, in percent: full volume, as
/// the `sound` table's default.
const FULL_VOLUME: f64 = 100.0;

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Why an addition is refused before it is queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// It names no campaign, or one the product does not hold.
    NoCampaign,
    /// Its name is missing or blank.
    NoName,
    /// Its file is missing or blank.
    NoFile,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoCampaign => "cette campagne n'existe pas",
            Self::NoName => "un son a besoin d'un nom",
            Self::NoFile => "un son a besoin de son fichier",
        })
    }
}

/// What the server gives an addition: the new sound's id, and the time it is
/// added at (RFC 3339).
#[derive(Clone, Copy, Debug)]
pub struct Fresh<'a> {
    /// The sound's id.
    pub sound: &'a str,
    /// Now.
    pub now: &'a str,
}

/// What an addition becomes before it is submitted: the row it writes on
/// `Sound/<target_id>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The sound's fields, under the Sound aggregate's names.
    pub payload: Row,
    /// The sound's id: the command's target.
    pub target_id: String,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
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

/// The row an addition of `wire` writes, the sound being `fresh.sound`.
///
/// # Errors
///
/// [`Refusal`] when `wire` names no campaign, no name or no file.
pub fn prepare(wire: &Row, fresh: Fresh<'_>) -> Result<Prepared, Refusal> {
    let campaign = campaign_id(wire).ok_or(Refusal::NoCampaign)?;
    let name = text(wire, "name").ok_or(Refusal::NoName)?;
    let file = text(wire, "file").ok_or(Refusal::NoFile)?;
    let mut payload = Row::new();
    payload.insert("id".into(), fresh.sound.into());
    payload.insert("campaignId".into(), campaign.into());
    payload.insert("name".into(), name.into());
    payload.insert("filePath".into(), file.into());
    payload.insert("tags".into(), tags(wire.get("tags")));
    payload.insert("volume".into(), volume(wire.get("volume")));
    payload.insert("createdAt".into(), fresh.now.into());
    Ok(Prepared {
        payload,
        target_id: fresh.sound.to_owned(),
    })
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
