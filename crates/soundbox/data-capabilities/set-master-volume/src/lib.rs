//! DataCapability `soundbox.setMasterVolume` (`data-capability.json`): the
//! DM moves the soundbox's master slider and every sound of the campaign
//! follows, keeping their ratio (EffectiveVolume@1). The slider speaks in
//! percent, from 0 to 100; the campaign stores a fraction from 0 to 1, the
//! percent divided by 100, as `soundMasterVolume` — one command on
//! `Campaign/<its id>`, which writes nothing else of it.
//!
//! [`prepare`] turns what the Micro-UI sends — `{campaign, volume}`, or the
//! volume alone on the command's target — into that command's payload and
//! target:
//!
//! - the target is the campaign, lowercase as the table writes it — one
//!   partition per campaign whatever case the request used;
//! - its payload is `soundMasterVolume`, the percent divided by 100, and
//!   nothing else;
//! - a volume out of range is never clamped: its fraction is written as it
//!   is, and the campaign's own invariant refuses it (`master-volume-range`);
//!   a volume that is no number is written `null`, which the invariant
//!   refuses too;
//! - a request naming no campaign is refused before anything is queued
//!   ([`Refusal`]).

use std::collections::BTreeMap;

use serde_json::{Number, Value};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "soundbox.setMasterVolume";

/// The one field it writes: the master volume, a fraction from 0 to 1.
pub const MASTER_VOLUME: &str = "soundMasterVolume";

/// What a percent is divided by to be stored.
const PERCENT: f64 = 100.0;

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Why a change of the master volume is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The request names no campaign.
    NoCampaign,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCampaign => f.write_str("cette campagne n'existe pas"),
        }
    }
}

/// What a change of the master volume becomes before it is submitted: what
/// the command writes on `Campaign/<target_id>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The fields written: the master volume alone.
    pub payload: Row,
    /// The campaign's id, lowercase: the command's target.
    pub target_id: String,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The change `wire` asks for — `{campaign, volume}`, the volume in percent
/// — on the campaign `target` names, else the one its `campaign` does.
///
/// # Errors
///
/// [`Refusal::NoCampaign`] when neither names a campaign.
pub fn prepare(wire: &Row, target: Option<&str>) -> Result<Prepared, Refusal> {
    let target_id = named(target)
        .or_else(|| named(wire.get("campaign").and_then(Value::as_str)))
        .ok_or(Refusal::NoCampaign)?
        .to_ascii_lowercase();
    let volume = wire
        .get("volume")
        .and_then(Value::as_f64)
        .and_then(|percent| Number::from_f64(percent / PERCENT))
        .map_or(Value::Null, Value::Number);
    Ok(Prepared {
        payload: Row::from([(MASTER_VOLUME.to_owned(), volume)]),
        target_id,
    })
}

/// `id`, trimmed, when it names something.
fn named(id: Option<&str>) -> Option<&str> {
    id.map(str::trim).filter(|id| !id.is_empty())
}
