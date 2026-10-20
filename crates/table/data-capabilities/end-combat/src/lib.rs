//! DataCapability `table.endCombat` (`data-capability.json`): the DM closes
//! the fight on a battlemap instance — the carousel goes, the map keeps its
//! tokens and their wounds, and a new combat may start later.
//!
//! The engine writes one partition per command and judges a row alone: the
//! combat is its own row, `BattlemapInstance/<its id>`, and an end is one
//! command there, carrying its end and no current turn. Its tokens, their HP
//! and their conditions are rows of their own, which it never writes; its
//! entries stay too, the order as it stood. Which combat of the instance is
//! still open is read before the end is queued — the engine cannot see it
//! from one row —; [`request`] reads the instance, and [`prepare`] makes
//! its open combat the command:
//!
//! - the instance is the command's target, or the payload's `instance`;
//! - an instance with no combat open, applied or queued, ends none
//!   (`one-active-combat`);
//! - what the tables could never hold is refused before anything is queued
//!   ([`Refusal`]).
//!
//! Wiping the map — its tokens, its combat and their initiative — is
//! `table.resetBattlemap`'s, not this command's.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.endCombat";

/// A row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What an end asks for: the instance, by its id, lowercase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The battlemap instance whose combat ends.
    pub instance: String,
}

/// The command an end is: the combat's id, which its partition is, and the
/// two fields it writes.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The combat's id.
    pub target_id: String,
    /// `endedAt`, and `currentEntryId` null: no turn is anyone's.
    pub payload: Row,
}

/// Why an end is refused before it is queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The instance is not named by a uuid.
    NotAnId,
    /// No combat of the instance is open (`one-active-combat`).
    NoOpenCombat,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId => f.write_str("instance n'est pas un identifiant"),
            Self::NoOpenCombat => f.write_str("aucun combat en cours sur cette carte"),
        }
    }
}

impl std::error::Error for Refusal {}

/// Whether `id` names this DataCapability, with or without its version.
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The end `wire` asks for on `target`, the instance the command names —
/// else the payload's `instance`.
///
/// # Errors
///
/// [`Refusal::NotAnId`] when the instance is no uuid.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    let instance = target
        .map(str::trim)
        .filter(|target| !target.is_empty())
        .or_else(|| wire.get("instance").and_then(Value::as_str))
        .and_then(id)
        .ok_or(Refusal::NotAnId)?;
    Ok(Request { instance })
}

/// The end written on the partition of `open`, the combat of the instance
/// a [`Request`] names with no end as the queue will leave it, stamped `now`,
/// RFC 3339.
///
/// # Errors
///
/// [`Refusal::NoOpenCombat`] when the instance has no combat open
/// (`one-active-combat`).
pub fn prepare(open: Option<&str>, now: &str) -> Result<Prepared, Refusal> {
    let combat = open.and_then(id).ok_or(Refusal::NoOpenCombat)?;
    Ok(Prepared {
        target_id: combat,
        payload: Row::from([
            ("endedAt".to_owned(), Value::String(now.to_owned())),
            ("currentEntryId".to_owned(), Value::Null),
        ]),
    })
}

/// `text` as an id the tables hold: a uuid as PostgreSQL writes one, 8-4-4-4-12
/// hex digits, lowercase.
fn id(text: &str) -> Option<String> {
    let text = text.trim();
    let uuid = text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });
    uuid.then(|| text.to_ascii_lowercase())
}
