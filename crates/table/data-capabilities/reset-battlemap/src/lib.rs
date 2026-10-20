//! DataCapability `table.resetBattlemap` (`data-capability.json`):
//! « Réinitialiser la carte » — the DM wipes a scene's map clean to replay
//! it, without losing the scene's instance nor the original battlemap.
//!
//! The engine writes one partition per command, and each token, each combat
//! and each initiative entry of the instance is its own,
//! `BattlemapInstance/<its id>`: a reset is one deletion on each, which the
//! engine keeps as a tombstone (a soft delete, reversible through the
//! command journal). The instance's own partition is never written — a
//! deletion there would take the instance — nor any row of another
//! instance, of the battlemap or of its grid (`instance-isolated`). Each
//! deletion carries the instance, the reset's time and size, which the
//! instance's row is stamped with once the tables follow (`resetAt`): what the
//! instance holds is read before the reset is queued — the engine cannot
//! see it from one row —; [`request`] reads the instance, and [`prepare`]
//! makes what it holds the commands:
//!
//! - the instance is the command's target, or the payload's `instance`;
//! - an instance the tables do not hold is none to reset, and one holding
//!   no token, no combat and no entry has nothing to wipe;
//! - what the tables could never hold is refused before anything is queued
//!   ([`Refusal`]).
//!
//! Closing the combat while the map keeps its tokens is `table.endCombat`'s.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.resetBattlemap";

/// A row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What a reset asks for: the instance, by its id, lowercase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The battlemap instance wiped clean.
    pub instance: String,
}

/// What the instance holds, as the commands queued will leave it: what a
/// reset takes off. Ids the tables hold, in any case.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Map {
    /// Whether the tables hold the instance.
    pub instance: bool,
    /// Its tokens.
    pub tokens: Vec<String>,
    /// Its combats, open or ended.
    pub combats: Vec<String>,
    /// The initiative entries of its combats and of its tokens.
    pub entries: Vec<String>,
}

/// Why a reset is refused before it is queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The instance is not named by a uuid.
    NotAnId,
    /// The tables hold no such instance.
    UnknownInstance,
    /// The instance holds no token, no combat and no entry.
    NothingToReset,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId => f.write_str("instance n'est pas un identifiant"),
            Self::UnknownInstance => f.write_str("aucune instance de carte de ce nom"),
            Self::NothingToReset => f.write_str("la carte n'a rien à réinitialiser"),
        }
    }
}

impl std::error::Error for Refusal {}

/// Whether `id` names this DataCapability, with or without its version.
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The reset `wire` asks for on `target`, the instance the command names —
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

/// The deletions a [`Request`] makes of `map`, what its instance holds,
/// stamped `now`, RFC 3339: one per partition, by id — its entries, then
/// its combats, then its tokens, so a reset that fails half queued leaves
/// the tokens on the map, and may be sent again. Each carries the instance
/// (`battlemapInstanceId`), the time it is reset at (`resetAt`), its row's
/// stamp, and how many deletions the reset is (`resetSize`), which the
/// stamp waits for; a deletion writes no field of its own partition.
///
/// # Errors
///
/// [`Refusal::UnknownInstance`] when the tables hold no such instance;
/// [`Refusal::NothingToReset`] when it holds no row to take off.
pub fn prepare(request: &Request, map: &Map, now: &str) -> Result<Vec<(String, Row)>, Refusal> {
    if !map.instance {
        return Err(Refusal::UnknownInstance);
    }
    let mut stamp = Row::from([
        (
            "battlemapInstanceId".to_owned(),
            Value::String(request.instance.clone()),
        ),
        ("resetAt".to_owned(), Value::String(now.to_owned())),
    ]);
    let mut seen = Vec::new();
    for group in [&map.entries, &map.combats, &map.tokens] {
        let mut ids: Vec<String> = group.iter().filter_map(|text| id(text)).collect();
        ids.sort_unstable();
        ids.dedup();
        // The instance's own partition is never one to delete.
        ids.retain(|one| *one != request.instance && !seen.contains(one));
        seen.extend(ids);
    }
    if seen.is_empty() {
        return Err(Refusal::NothingToReset);
    }
    stamp.insert("resetSize".to_owned(), Value::from(seen.len()));
    Ok(seen.into_iter().map(|one| (one, stamp.clone())).collect())
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
