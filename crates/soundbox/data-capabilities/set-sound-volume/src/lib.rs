//! DataCapability `soundbox.setSoundVolume` (`data-capability.json`): during
//! a session the DM moves one sound's slider and only that sound gets louder
//! or quieter — what it plays at is its own volume times the campaign's
//! master volume (EffectiveVolume@1), so the others keep theirs. The slider
//! speaks in percent, from 0 to 100; the sound stores a fraction from 0 to
//! 1, the percent divided by 100, as `volume` — one command on
//! `Sound/<its id>`, which writes nothing else of it.
//!
//! [`prepare`] turns what the Micro-UI sends — `{volume}` on the command's
//! target, the sound — into that command's payload and target:
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
//! - a request naming no sound is refused before anything is queued
//!   ([`Refusal`]). Whether the sound exists is the server's to ask, against
//!   the rows it holds.

use std::collections::BTreeMap;

use serde_json::{Number, Value};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "soundbox.setSoundVolume";

/// The one field it writes: the sound's own volume, a fraction from 0 to 1.
pub const VOLUME: &str = "volume";

/// What a percent is divided by to be stored.
const PERCENT: f64 = 100.0;

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

/// What a change of a sound's volume becomes before it is submitted: what
/// the command writes on `Sound/<target_id>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The fields written: the volume alone.
    pub payload: Row,
    /// The sound's id, lowercase: the command's target.
    pub target_id: String,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The change `wire` asks for — `{volume}`, in percent — on the sound
/// `target` names.
///
/// # Errors
///
/// [`Refusal::NoSound`] when `target` names no sound.
pub fn prepare(wire: &Row, target: Option<&str>) -> Result<Prepared, Refusal> {
    let target_id = target
        .map(str::trim)
        .filter(|id| !id.is_empty())
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
