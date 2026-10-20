//! DataCapability `table.reorderInitiative` (`data-capability.json`): the DM
//! drags a participant to another place in the carousel — a delayed action,
//! a ruling — and the new order is saved.
//!
//! The ranks are computed at the launch (`initiative-tiebreak`), then the
//! DM's to edit: what the DM sends is the whole order of the instance's open
//! combat, every participant once, named by its token — or its initiative
//! entry — under `order`, and the instance as the command's target, or
//! under `instance`; [`request`] reads it. The engine writes one partition
//! per command, and each entry is its own, `BattlemapInstance/<its id>`:
//! [`prepare`] makes the order one rank per entry, 1 to n in the order
//! given, without gap or duplicate, each carrying its entry's rank alone —
//! never a visibility (`hidden-keeps-rank`: a hidden participant takes the
//! rank it is given, like any other), never the turn, never another field:
//!
//! - an order the tables could never hold is refused before it is queued
//!   ([`Refusal`]): the instance and each participant are uuids, and the
//!   order is a list of them, not empty;
//! - an instance with no open combat has no order to rewrite;
//! - an order that omits a participant, repeats one, or names one that is
//!   not in the combat is refused: no participant is added or removed.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.reorderInitiative";

/// The aggregate whose partition an initiative entry is.
const AGGREGATE: &str = "BattlemapInstance";

/// A row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What the DM asked: the instance, and its participants in their new order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The instance's id, lowercase.
    pub instance: String,
    /// Each participant's token, or entry, id, lowercase, first rank first.
    pub order: Vec<String>,
}

/// A participant of the open combat: its initiative entry, and its token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    /// The entry's id, lowercase: its partition.
    pub entry: String,
    /// The token's id, lowercase.
    pub token: String,
}

/// One command of a reorder: an entry's id, which its partition is, and its
/// new rank.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    /// The entry's id.
    pub target_id: String,
    /// `rank`, and nothing else.
    pub payload: Row,
}

/// Why a reorder is refused before it is queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The instance, or a participant of the order, is not named by a uuid.
    NotAnId(&'static str),
    /// The order is absent, not a list, or empty.
    NoOrder,
    /// The instance has no open combat.
    NoOpenCombat,
    /// The order omits a participant, repeats one, or names one that is not
    /// in the combat.
    NotTheParticipants,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::NoOrder => f.write_str("un ordre est une liste de participants"),
            Self::NoOpenCombat => f.write_str("cette instance n'a pas de combat en cours"),
            Self::NotTheParticipants => {
                f.write_str("l'ordre nomme chaque participant du combat une fois, et eux seuls")
            }
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

/// What `wire`, sent to `target`, asks: the instance is `target` — or
/// `wire.instance` when the command names no target —, the order
/// `wire.order`.
///
/// # Errors
///
/// [`Refusal::NotAnId`] when the instance or a participant is not a uuid,
/// [`Refusal::NoOrder`] when the order is no list, or an empty one.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    let instance = target
        .map(str::trim)
        .filter(|target| !target.is_empty())
        .or_else(|| wire.get("instance").and_then(Value::as_str))
        .and_then(id)
        .ok_or(Refusal::NotAnId("instance"))?;
    let order = wire
        .get("order")
        .and_then(Value::as_array)
        .filter(|order| !order.is_empty())
        .ok_or(Refusal::NoOrder)?
        .iter()
        .map(|participant| {
            participant
                .as_str()
                .and_then(id)
                .ok_or(Refusal::NotAnId("order"))
        })
        .collect::<Result<_, _>>()?;
    Ok(Request { instance, order })
}

/// `request` made one command per participant of the open combat, against
/// `combat`, its participants as the commands queued will leave them —
/// `None` when the instance has none open: each entry's rank, 1 to n in the
/// order given.
///
/// # Errors
///
/// [`Refusal::NoOpenCombat`] when `combat` is `None`,
/// [`Refusal::NotTheParticipants`] when the order is not every participant
/// once.
pub fn prepare(
    request: &Request,
    combat: Option<&[Participant]>,
) -> Result<Vec<Prepared>, Refusal> {
    let participants = combat.ok_or(Refusal::NoOpenCombat)?;
    if request.order.len() != participants.len() {
        return Err(Refusal::NotTheParticipants);
    }
    let mut placed = vec![false; participants.len()];
    let mut ranks = Vec::with_capacity(participants.len());
    for (rank, named) in (1_u64..).zip(&request.order) {
        let at = participants
            .iter()
            .position(|p| p.token == *named || p.entry == *named)
            .filter(|at| !placed[*at])
            .ok_or(Refusal::NotTheParticipants)?;
        placed[at] = true;
        ranks.push(Prepared {
            target_id: participants[at].entry.clone(),
            payload: Row::from([("rank".to_owned(), Value::from(rank))]),
        });
    }
    Ok(ranks)
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
