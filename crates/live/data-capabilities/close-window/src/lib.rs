//! DataCapability `live.closeWindow` (`data-capability.json`): the DM takes
//! the illustration's floating window off the players' screen in one action,
//! leaving the map or the cover underneath.
//!
//! The engine writes one partition per command: a close is one command on
//! `Diffusion/<the session's id>`, the one row both player views read
//! (single-diffusion-source). The session is the live session of the
//! campaign the payload names, which the server looks up from [`campaign`];
//! [`prepare`] turns it, with the row as the engine will leave it, into the
//! state written:
//!
//! - the window's media, `x`, `y` and width all emptied together, and the
//!   stamp — so no half-open window remains (window-fields-together);
//! - the base and the framing are not written: they stay as the row holds
//!   them (`live.showCover` brings the base back to the cover);
//! - a window already closed is closed again: the row is unchanged but its
//!   stamp, and the mode is `overwrite`, so nothing is asked;
//! - a payload naming no campaign, a campaign with no live session, or a
//!   session nothing was ever shown on — its row, which a close may not
//!   make, holds no window to close — is refused before it is queued
//!   ([`Refusal`]): nothing changes.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "live.closeWindow";

/// The window's fields, all emptied by a close: its media, its position and
/// its width (its height is never stored).
const WINDOW: [&str; 4] = ["windowMediaId", "windowX", "windowY", "windowWidth"];

/// A row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Whether `id` — `live.closeWindow`, with or without its `@version` — names
/// this DataCapability.
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// What a close becomes before it is submitted.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The state it writes on the session's diffusion row.
    pub payload: Row,
    /// The partition it writes: the session's id.
    pub target_id: String,
}

/// Why a close is refused before it is queued: nothing changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload names no campaign.
    NoCampaign,
    /// The campaign named is not a uuid.
    NotAnId,
    /// No session of the campaign is live: no player view is shown.
    NoLiveSession,
    /// Nothing was ever shown on the live session's player view: it has no
    /// diffusion row, so no window to close.
    NothingShown,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoCampaign => "campaign manque",
            Self::NotAnId => "campaign n'est pas un identifiant",
            Self::NoLiveSession => "aucune séance n'est en cours",
            Self::NothingShown => "rien n'est diffusé aux joueurs : aucune fenêtre à fermer",
        })
    }
}

/// The campaign the payload names, its uuid in lower case: the one whose
/// live session the server looks up.
///
/// # Errors
///
/// [`Refusal::NoCampaign`] when it names none, [`Refusal::NotAnId`] when
/// what it names is no uuid.
pub fn campaign(wire: &Row) -> Result<String, Refusal> {
    match wire.get("campaign") {
        None | Some(Value::Null) => Err(Refusal::NoCampaign),
        Some(Value::String(text)) if is_uuid(text) => Ok(text.to_ascii_lowercase()),
        Some(_) => Err(Refusal::NotAnId),
    }
}

/// The state closing the window writes on the diffusion row of `session`,
/// the campaign's live session, which the engine will leave as `row`,
/// stamped `now`.
///
/// # Errors
///
/// [`Refusal::NoLiveSession`] when the campaign has no live session,
/// [`Refusal::NothingShown`] when `row` is no session's.
pub fn prepare(session: Option<String>, row: &Row, now: &str) -> Result<Prepared, Refusal> {
    let session = session.ok_or(Refusal::NoLiveSession)?;
    if row.get("sessionId").is_none_or(Value::is_null) {
        return Err(Refusal::NothingShown);
    }
    let mut payload: Row = WINDOW
        .iter()
        .map(|field| ((*field).to_string(), Value::Null))
        .collect();
    payload.insert("updatedAt".to_string(), Value::from(now));
    Ok(Prepared {
        payload,
        target_id: session,
    })
}

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}
