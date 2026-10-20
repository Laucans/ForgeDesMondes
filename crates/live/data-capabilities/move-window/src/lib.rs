//! DataCapability `live.moveWindow` (`data-capability.json`): the DM drags
//! the floating window to where it hides the least of the map, and the
//! projected screen follows.
//!
//! The engine writes one partition per command: a move is one command on
//! `Diffusion/<the session's id>`, the one row both player views read
//! (single-diffusion-source). The session is the live session of the
//! campaign the payload names; the server looks it up, and [`prepare`] turns
//! it, with the position [`named`] reads, into the state written:
//!
//! - the window's `x` and `y`, fractions of the screen, and the stamp —
//!   nothing else: the width (`live.resizeWindow`'s) and the media stay as
//!   the row holds them. A position outside 0 to 1 is brought back to it,
//!   the screen's edge, which the table's columns can hold;
//! - a window that is not open is moved all the same, for the DataGuard to
//!   refuse: a position with no media and no width breaks
//!   window-fields-together, and the row stays;
//! - drags in a row are queued one after the other and each applied, the
//!   last one's position kept: the mode is `overwrite`, nothing is asked;
//! - a payload naming no campaign or no position, or a campaign with no live
//!   session, is refused before it is queued ([`Refusal`]): nothing changes.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "live.moveWindow";

/// A row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Whether `id` — `live.moveWindow`, with or without its `@version` — names
/// this DataCapability.
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// What a move names: the campaign, the server looks the live session of
/// up, and where the window goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Named {
    /// The campaign whose live session's player view shows the window: a
    /// uuid in lower case.
    pub campaign: String,
    /// The window's left edge, a fraction of the screen's width, as asked.
    pub x: f64,
    /// The window's top edge, a fraction of the screen's height, as asked.
    pub y: f64,
}

/// What a move becomes before it is submitted.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The state it writes on the session's diffusion row.
    pub payload: Row,
    /// The partition it writes: the session's id.
    pub target_id: String,
}

/// Why a move is refused before it is queued: nothing changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload names no campaign.
    NoCampaign,
    /// The campaign named is not a uuid.
    NotAnId,
    /// The payload names no `x` or no `y`.
    NoPosition,
    /// The `x` or the `y` named is not a number.
    NotANumber,
    /// No session of the campaign is live: no player view is shown.
    NoLiveSession,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoCampaign => "campaign manque",
            Self::NotAnId => "campaign n'est pas un identifiant",
            Self::NoPosition => "x ou y manque",
            Self::NotANumber => "x ou y n'est pas un nombre",
            Self::NoLiveSession => "aucune séance n'est en cours",
        })
    }
}

/// What the payload names: the campaign, `x` and `y`.
///
/// # Errors
///
/// [`Refusal::NoCampaign`] or [`Refusal::NoPosition`] when it names none,
/// [`Refusal::NotAnId`] when the campaign is no uuid,
/// [`Refusal::NotANumber`] when the position is no number.
pub fn named(wire: &Row) -> Result<Named, Refusal> {
    let campaign = match wire.get("campaign") {
        None | Some(Value::Null) => return Err(Refusal::NoCampaign),
        Some(Value::String(text)) if is_uuid(text) => text.to_ascii_lowercase(),
        Some(_) => return Err(Refusal::NotAnId),
    };
    Ok(Named {
        campaign,
        x: number(wire, "x")?,
        y: number(wire, "y")?,
    })
}

/// The state moving the window to `named`'s position writes on the diffusion
/// row of `session`, the campaign's live session, stamped `now`.
///
/// # Errors
///
/// [`Refusal::NoLiveSession`] when the campaign has no live session.
pub fn prepare(named: &Named, session: Option<String>, now: &str) -> Result<Prepared, Refusal> {
    let session = session.ok_or(Refusal::NoLiveSession)?;
    Ok(Prepared {
        payload: Row::from([
            ("windowX".to_string(), Value::from(on_screen(named.x))),
            ("windowY".to_string(), Value::from(on_screen(named.y))),
            ("updatedAt".to_string(), Value::from(now)),
        ]),
        target_id: session,
    })
}

/// `fraction` brought back onto the screen: from 0 to 1.
fn on_screen(fraction: f64) -> f64 {
    fraction.clamp(0.0, 1.0)
}

/// The number `wire` names under `field`.
fn number(wire: &Row, field: &str) -> Result<f64, Refusal> {
    match wire.get(field) {
        None | Some(Value::Null) => Err(Refusal::NoPosition),
        Some(value) => value.as_f64().ok_or(Refusal::NotANumber),
    }
}

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}
