//! DataCapability `table.removeParticipant` (`data-capability.json`): a
//! creature flees or surrenders — the DM takes it out of the turn order, its
//! token stays on the map, and the combat carries on without it.
//!
//! The engine writes one partition per command, and a participant's place in
//! the combat is its own, `BattlemapInstance/<its entry's id>`: taking it out
//! is one deletion there, which the engine keeps as a tombstone (a soft
//! delete, reversible through the command journal), and nothing else. Its
//! token is a row of its own, which the command never writes. The order the
//! carousel shows closes up by itself, each place counted from the ranks that
//! remain (`orderIndex`, derived); a participant that holds the turn is
//! passed by `table.nextTurn` first, sent before this command: the next
//! living participant then has it. What the DM sends names the entry, under
//! `entry`, and the instance it is a combat of as the command's target — or
//! the entry itself as the target; [`request`] reads it, and [`prepare`]
//! makes it the command, against the entry's row and its combat's as the
//! queue will leave them:
//!
//! - a command naming no uuid is refused before it is queued ([`Refusal`]);
//! - an entry the engine does not hold, or one of a combat of another
//!   instance than the target, is none to remove (`instance-isolated`);
//! - an entry of a combat that ended is out of no running order.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.removeParticipant";

/// The aggregate whose partition an entry, and its combat, is.
const AGGREGATE: &str = "BattlemapInstance";

/// A row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What the DM asked: the entry, and the instance its combat should be of
/// when the target named one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The entry's id, lowercase.
    pub entry: String,
    /// The instance the command targets, lowercase; `None` when the entry is
    /// the target.
    pub instance: Option<String>,
}

/// The command a removal is: the entry's id, which its partition is, and no
/// field — a deletion writes none, its partition is tombstoned whole.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    /// The entry's id.
    pub target_id: String,
    /// Empty.
    pub payload: Row,
}

/// Why a removal is refused before it is queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The entry, or the instance, is not named by a uuid.
    NotAnId(&'static str),
    /// The engine holds no such entry, or holds it in a combat of another
    /// instance.
    UnknownEntry,
    /// The entry's combat has ended: there is no running order to leave.
    CombatEnded,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::UnknownEntry => {
                f.write_str("ce participant n'est pas dans un combat de cette instance")
            }
            Self::CombatEnded => f.write_str("ce combat est terminé"),
        }
    }
}

impl std::error::Error for Refusal {}

/// Whether `id` names this DataCapability, with or without its version.
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The partition of the entry, or the combat, `id`.
pub fn partition(id: &str) -> String {
    format!("{AGGREGATE}/{id}")
}

/// What `wire`, sent to `target`, asks: the entry is `wire.entry`, in a
/// combat of the instance `target` — or `target` itself when `wire` names no
/// entry.
///
/// # Errors
///
/// [`Refusal::NotAnId`] when the entry or the instance is not a uuid.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    let target = target.map(str::trim).filter(|target| !target.is_empty());
    let (entry, instance) = match wire.get("entry") {
        Some(entry) => (
            entry
                .as_str()
                .and_then(id)
                .ok_or(Refusal::NotAnId("entry"))?,
            target
                .map(|target| id(target).ok_or(Refusal::NotAnId("instance")))
                .transpose()?,
        ),
        None => (target.and_then(id).ok_or(Refusal::NotAnId("entry"))?, None),
    };
    Ok(Request {
        // The entry named as its own target is in no instance named.
        instance: instance.filter(|instance| *instance != entry),
        entry,
    })
}

/// The combat `entry`, an entry's row, is of: its id, lowercase; `None` when
/// the row is no entry.
pub fn combat_of(entry: &Row) -> Option<String> {
    held(entry, "initiativeEntryCombatId")
}

/// `request` made the deletion of the entry's partition, against `entry`,
/// its row, and `combat`, its combat's — each as the commands queued there
/// will leave it.
///
/// # Errors
///
/// [`Refusal::UnknownEntry`] when `entry` holds no such entry, `combat` no
/// combat of the instance the request names; [`Refusal::CombatEnded`] when
/// the combat has an end.
pub fn prepare(request: &Request, entry: &Row, combat: &Row) -> Result<Prepared, Refusal> {
    let of = combat_of(entry).ok_or(Refusal::UnknownEntry)?;
    let on = held(combat, "combatInstanceId").ok_or(Refusal::UnknownEntry)?;
    if held(entry, "initiativeEntryId").as_deref() != Some(request.entry.as_str())
        || held(combat, "combatId").as_deref() != Some(of.as_str())
        || request
            .instance
            .as_ref()
            .is_some_and(|instance| *instance != on)
    {
        return Err(Refusal::UnknownEntry);
    }
    if !combat.get("endedAt").is_none_or(Value::is_null) {
        return Err(Refusal::CombatEnded);
    }
    Ok(Prepared {
        target_id: request.entry.clone(),
        payload: Row::new(),
    })
}

/// `row`'s `field`, an id, lowercase.
fn held(row: &Row, field: &str) -> Option<String> {
    row.get(field)
        .and_then(Value::as_str)
        .map(str::to_lowercase)
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
