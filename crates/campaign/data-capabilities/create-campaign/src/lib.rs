//! DataCapability `campaign.createCampaign` (`data-capability.json`): the DM
//! creates a campaign or a oneshot from the home page. One command on
//! `Campaign/<its new id>` writes the campaign whole; a oneshot's carries its
//! single session too, so the DM never adds it by hand
//! (`oneshot-single-session`). [`prepare`] turns the wire's payload —
//! `{name, type, pitch?, art?, glyph?}` — into that command:
//!
//! - the campaign is given its id, and its creation's time, by the server
//!   ([`Fresh`]); it is the command's target, lowercase as the table writes
//!   it;
//! - its name is the DM's, trimmed, and never blank; its kind, `campagne` or
//!   `oneshot`, the wire's `type`; its pitch, art and glyph the DM's, or
//!   `null`;
//! - its master volume starts at full, [`MASTER_VOLUME`]: inside 0–1
//!   (`master-volume-range`), as the table's default holds it;
//! - a oneshot's session is given its id by the server too: it is the
//!   campaign's first and only one — at position 1, in `prep`, the microphone
//!   off — titled after the campaign. A campagne carries none: its sessions
//!   are added one by one, later;
//! - a payload with no name, or of a kind there is not, is refused before
//!   anything is queued ([`Refusal`]).

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "campaign.createCampaign";

/// A campaign's kind with several sessions.
pub const CAMPAGNE: &str = "campagne";

/// A campaign's kind with a single session.
pub const ONESHOT: &str = "oneshot";

/// A new campaign's master volume: full, until the DM moves it.
pub const MASTER_VOLUME: f64 = 1.0;

/// A oneshot's session's position: the campaign's first.
pub const SESSION_POSITION: u64 = 1;

/// A oneshot's session's status: in preparation (`status-lifecycle`).
pub const SESSION_STATUS: &str = "prep";

/// A oneshot's session's microphone: off.
pub const MICROPHONE_ON: bool = false;

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// The ids and the time the server gives a new campaign.
#[derive(Clone, Copy, Debug)]
pub struct Fresh<'a> {
    /// The campaign's id.
    pub campaign: &'a str,
    /// Its session's id, should it be a oneshot.
    pub session: &'a str,
    /// Now, RFC 3339.
    pub now: &'a str,
}

/// Why a creation is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload names no campaign, or only blanks.
    NoName,
    /// The payload's `type` is neither `campagne` nor `oneshot`.
    UnknownType,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoName => "une campagne a un nom",
            Self::UnknownType => "une campagne est une « campagne » ou un « oneshot »",
        })
    }
}

/// What a creation becomes before it is submitted: what the command writes
/// on `Campaign/<target_id>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The campaign's fields — and a oneshot's session's.
    pub payload: Row,
    /// The campaign's id, lowercase: the command's target.
    pub target_id: String,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The creation the wire's payload asks for, under the ids and at the time
/// `fresh` gives.
///
/// # Errors
///
/// [`Refusal`] when the payload has no name, or a `type` that is neither
/// `campagne` nor `oneshot`.
pub fn prepare(wire: &Row, fresh: Fresh<'_>) -> Result<Prepared, Refusal> {
    let name = wire
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or(Refusal::NoName)?;
    let kind = wire
        .get("type")
        .and_then(Value::as_str)
        .filter(|kind| [CAMPAGNE, ONESHOT].contains(kind))
        .ok_or(Refusal::UnknownType)?;
    let id = fresh.campaign.to_ascii_lowercase();
    let optional = |field: &str| {
        wire.get(field)
            .filter(|value| value.is_string())
            .cloned()
            .unwrap_or(Value::Null)
    };
    let mut payload = Row::from([
        ("id".into(), id.clone().into()),
        ("name".into(), name.into()),
        ("kind".into(), kind.into()),
        ("pitch".into(), optional("pitch")),
        ("art".into(), optional("art")),
        ("glyph".into(), optional("glyph")),
        ("createdAt".into(), fresh.now.into()),
        ("soundMasterVolume".into(), MASTER_VOLUME.into()),
    ]);
    if kind == ONESHOT {
        payload.extend([
            (
                "sessionId".into(),
                fresh.session.to_ascii_lowercase().into(),
            ),
            ("sessionCampaignId".into(), id.clone().into()),
            ("sessionPosition".into(), SESSION_POSITION.into()),
            ("sessionTitle".into(), name.into()),
            ("sessionStatus".into(), SESSION_STATUS.into()),
            ("microphoneOn".into(), MICROPHONE_ON.into()),
        ]);
    }
    Ok(Prepared {
        payload,
        target_id: id,
    })
}
