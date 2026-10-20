//! DataCapability `session.createSession` (`data-capability.json`): the DM
//! adds the next session of a campaign, with its title, the day it is planned
//! for and the PCs ticked present.
//!
//! The engine writes one partition per command: a new session is one command
//! on `Session/<its id>`, carrying the session and the PCs present, which the
//! DataGuard's invariants judge and its store projects into `session` and
//! `session_present_character`. What the DM sends is the wire payload of
//! SessionList; [`prepare`] turns it into that state:
//!
//! - the session takes the position it is given — the one after the
//!   campaign's last, which the server reads from what it holds — starts in
//!   « prep » (`status-lifecycle`) and with its microphone off;
//! - the PCs present are kept in the order ticked, a duplicate once. That
//!   each is a PC of the same campaign (`present-only-pj`), and that a
//!   oneshot takes no second session (`oneshot-single-session`), is the
//!   DataGuard's to judge;
//! - a session the tables could never hold is refused before it is queued
//!   ([`Refusal`]): it names its campaign by a uuid, it has a title, its day
//!   is a date, its PCs are a list of ids, and no value holds the character
//!   U+0000.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "session.createSession";

/// The status a session starts in (`status-lifecycle`).
pub const PREP: &str = "prep";

/// The row's field holding the PCs present, an array of their ids.
pub const PRESENT: &str = "sessionPresentCharacterCharacterId";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a new session is given besides its wire payload: its id, its
/// position in the campaign, and the time.
#[derive(Clone, Copy, Debug)]
pub struct Fresh<'a> {
    /// The session's id, which the server made.
    pub session: &'a str,
    /// Its position: the one after the campaign's last session.
    pub position: u64,
    /// Now, RFC 3339: when the PCs were ticked present.
    pub now: &'a str,
}

/// Why a new session is refused before it is queued: what the tables could
/// never hold, which no invariant of the aggregate names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// No campaign is named, or not by a uuid.
    NoCampaign,
    /// No title, or one of blanks alone.
    NoTitle,
    /// The planned day is not a date `YYYY-MM-DD`.
    NotADate,
    /// The PCs present are not a list of ids.
    NotPresent,
    /// A value holds the character U+0000, which `text` cannot hold.
    Nul,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoCampaign => "une séance nomme sa campagne par son identifiant",
            Self::NoTitle => "une séance a un titre",
            Self::NotADate => "la date prévue d'une séance est un jour AAAA-MM-JJ",
            Self::NotPresent => "les PJ présents sont une liste d'identifiants",
            Self::Nul => "une séance ne contient pas de caractère nul",
        })
    }
}

/// What a new session becomes before it is submitted.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The session's id: the partition `Session/<target_id>`.
    pub target_id: String,
    /// What the command writes on it.
    pub payload: Row,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The campaign a new session names, when it names one by a uuid.
#[must_use]
pub fn campaign_of(wire: &Row) -> Option<String> {
    wire.get("campaign")
        .and_then(Value::as_str)
        .filter(|id| is_uuid(id))
        .map(str::to_lowercase)
}

/// Whether `text` is a uuid as PostgreSQL writes one: 8-4-4-4-12 hex digits.
fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Whether `text` is a day `YYYY-MM-DD` of the calendar.
fn is_date(text: &str) -> bool {
    let mut parts = text.split('-');
    let (Some(year), Some(month), Some(day), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let number = |part: &str, digits: usize| {
        (part.len() == digits && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<u32>().ok())
            .flatten()
    };
    let (Some(year), Some(month), Some(day)) = (number(year, 4), number(month, 2), number(day, 2))
    else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// The PCs ticked present, in the order ticked, each once: an id given again
/// in another case is the same one.
fn present_of(wire: &Row) -> Result<Vec<Value>, Refusal> {
    let Some(given) = wire.get("present").filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let ids = given.as_array().ok_or(Refusal::NotPresent)?;
    let mut kept: Vec<String> = Vec::new();
    for id in ids {
        let id = id.as_str().ok_or(Refusal::NotPresent)?;
        let id = if is_uuid(id) {
            id.to_lowercase()
        } else {
            id.to_owned()
        };
        if !kept.contains(&id) {
            kept.push(id);
        }
    }
    Ok(kept.into_iter().map(Value::from).collect())
}

/// The command a new session of `wire` submits on the partition
/// `Session/<fresh.session>`.
///
/// # Errors
///
/// [`Refusal`] for a session the tables could never hold.
pub fn prepare(wire: &Row, fresh: Fresh<'_>) -> Result<Prepared, Refusal> {
    let campaign = campaign_of(wire).ok_or(Refusal::NoCampaign)?;
    let title = wire
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .ok_or(Refusal::NoTitle)?;
    let planned = match wire.get("plannedOn") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(day)) if day.trim().is_empty() => Value::Null,
        Some(Value::String(day)) if is_date(day.trim()) => day.trim().into(),
        Some(_) => return Err(Refusal::NotADate),
    };
    let present = present_of(wire)?;
    let nul = |value: &Value| value.as_str().is_some_and(|text| text.contains('\0'));
    if title.contains('\0') || present.iter().any(nul) {
        return Err(Refusal::Nul);
    }
    let id = fresh.session.to_owned();
    let mut payload = Row::new();
    for (field, value) in [
        ("sessionId", Value::from(id.clone())),
        ("sessionCampaignId", campaign.into()),
        ("sessionPosition", fresh.position.into()),
        ("sessionTitle", title.into()),
        ("plannedOn", planned),
        ("sessionStatus", PREP.into()),
        ("microphoneOn", false.into()),
        ("sessionPresentCharacterSessionId", id.clone().into()),
        (PRESENT, Value::Array(present)),
        ("addedAt", fresh.now.into()),
    ] {
        payload.insert(field.into(), value);
    }
    Ok(Prepared {
        target_id: id,
        payload,
    })
}
