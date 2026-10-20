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
//!   ([`Refusal`]). Whether the sound exists is the server's to ask, against
//!   the rows it holds.
//!
//! Two edits of one sound queue on its partition: the DataGuard asks the
//! author of the second whether it overwrites the first, and never rejects
//! it for that.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "soundbox.updateSound";

/// The full volume, in percent.
const FULL_VOLUME: f64 = 100.0;

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

/// What an edit becomes before it is submitted: the fields it writes on
/// `Sound/<target_id>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The fields it changes, under the Sound aggregate's names.
    pub payload: Row,
    /// The sound's id: the command's target.
    pub target_id: String,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The fields an edit of `wire` writes on the sound `target` names — the
/// command's target — trimmed and lowercase as the `sound` table writes its
/// ids.
///
/// # Errors
///
/// [`Refusal`] when no sound is named, the name or the file is given blank,
/// or nothing is given to change.
pub fn prepare(wire: &Row, target: Option<&str>) -> Result<Prepared, Refusal> {
    let target_id = target
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
    (percent / 100.0).into()
}

/// A field as trimmed text; `None` when not a string or blank.
fn text(wire: &Row, field: &str) -> Option<String> {
    wire.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}
