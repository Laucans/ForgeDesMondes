//! DataCapability `live.showCover` (`data-capability.json`): the DM brings
//! the player view back to the campaign's cover, the neutral screen nothing
//! of the previous map or illustration lingers on.
//!
//! The engine writes one partition per command: a return to the cover is one
//! command on `Diffusion/<the session's id>`, the one row both player views
//! read (single-diffusion-source) — made by it when the session had none,
//! its base replaced otherwise. The session is the live session of the
//! campaign the payload names, which the server looks up from [`campaign`];
//! [`prepare`] turns it into the state written:
//!
//! - the base is the cover, its media and its instance emptied, so the base
//!   is the cover alone (base-coherent);
//! - the floating window and the framing are not written: they stay as the
//!   row holds them (`live.closeWindow` closes the window);
//! - a payload naming no campaign, or one with no live session, is refused
//!   before it is queued ([`Refusal`]): nothing changes.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "live.showCover";

/// The base it shows.
const COVER: &str = "cover";

/// A row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Whether `id` — `live.showCover`, with or without its `@version` — names
/// this DataCapability.
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// What a return to the cover becomes before it is submitted.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The state it writes on the session's diffusion row.
    pub payload: Row,
    /// The partition it writes: the session's id.
    pub target_id: String,
}

/// Why a return to the cover is refused before it is queued: nothing
/// changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload names no campaign.
    NoCampaign,
    /// The campaign named is not a uuid.
    NotAnId,
    /// No session of the campaign is live: no player view is shown.
    NoLiveSession,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoCampaign => "campaign manque",
            Self::NotAnId => "campaign n'est pas un identifiant",
            Self::NoLiveSession => "aucune séance n'est en cours",
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

/// The state a return to the cover writes on the diffusion row of
/// `session`, the campaign's live session, stamped `now`.
///
/// # Errors
///
/// [`Refusal::NoLiveSession`] when the campaign has no live session.
pub fn prepare(session: Option<String>, now: &str) -> Result<Prepared, Refusal> {
    let session = session.ok_or(Refusal::NoLiveSession)?;
    let payload = Row::from([
        ("sessionId".to_string(), Value::from(session.as_str())),
        ("baseKind".to_string(), Value::from(COVER)),
        ("baseMediaId".to_string(), Value::Null),
        ("baseInstanceId".to_string(), Value::Null),
        ("updatedAt".to_string(), Value::from(now)),
    ]);
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
