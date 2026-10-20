//! DataCapability `table.nextTurn` (`data-capability.json`): the DM presses
//! « Suivant » and the turn of the instance's open combat passes on, as
//! Initiative@1's NEXT says.
//!
//! The engine writes one partition per command, and a combat is its own,
//! `BattlemapInstance/<its id>`: a turn is one command there, carrying the
//! entry whose turn it now is and the round's step. The command is relative:
//! the engine adds the step to the round as the commands ahead leave it, so
//! two « Suivant » sent at once both count. What the DM sends names the
//! instance, as the command's target or under `instance`; [`request`] reads
//! it, and [`prepare`] makes it the command, against the combat and its
//! participants as the queue will leave them:
//!
//! - an instance not named by a uuid is refused before it is queued
//!   ([`Refusal::NotAnId`]); one with no open combat has no turn to pass
//!   ([`Refusal::NoOpenCombat`]), and a combat whose participants are all
//!   dead none to pass it to ([`Refusal::NoOneAlive`]);
//! - the turn goes to the next living participant after the current one in
//!   rank order (`next-turn`): a dead NPC is skipped (`dead-derived`), a
//!   hidden one takes its turn at its rank (`hidden-keeps-rank`);
//! - when the search wraps past the last rank, the round goes up by one,
//!   even when the first participants are dead and the turn lands later.
//!
//! No rank, participant or token is written: the command moves the turn and
//! the round alone.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.nextTurn";

/// The aggregate whose partition a combat is.
const AGGREGATE: &str = "BattlemapInstance";

/// A row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What the DM asked: « Suivant » on this instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The instance's id, lowercase.
    pub instance: String,
}

/// The instance's open combat, as the queue will leave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combat {
    /// The combat's id, which its partition is.
    pub id: String,
    /// The entry whose turn it is; `None` when no turn is current.
    pub current: Option<String>,
}

/// One participant of the combat: its entry, its rank, and whether its
/// token is alive — a PC always is, an NPC until its HP reach 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    /// The entry's id.
    pub entry: String,
    /// Its rank, 1 first.
    pub rank: i64,
    /// Whether its token is alive.
    pub alive: bool,
}

/// The command a turn is: the combat's id, which its partition is, and the
/// two fields it writes.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    /// The combat's id.
    pub target_id: String,
    /// `currentEntryId`, the entry whose turn it now is, and `round`, the
    /// step the engine adds: 1 when the turn wrapped, 0 otherwise.
    pub payload: Row,
}

/// Why a « Suivant » is refused before it is queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The instance is not named by a uuid.
    NotAnId,
    /// The instance has no open combat.
    NoOpenCombat,
    /// Every participant of the combat is dead, or it has none.
    NoOneAlive,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId => f.write_str("instance n'est pas un identifiant"),
            Self::NoOpenCombat => f.write_str("aucun combat n'est en cours sur cette instance"),
            Self::NoOneAlive => f.write_str("aucun participant vivant à qui passer le tour"),
        }
    }
}

impl std::error::Error for Refusal {}

/// Whether `id` names this DataCapability, with or without its version.
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The partition of the combat `id` — of any row of the aggregate, an entry
/// or a token: each is its own.
pub fn partition(id: &str) -> String {
    format!("{AGGREGATE}/{id}")
}

/// What `wire`, sent to `target`, asks: « Suivant » on the instance
/// `target` — or `wire.instance` when the command names no target.
///
/// # Errors
///
/// [`Refusal::NotAnId`] when the instance is absent or not a uuid.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    let target = target.map(str::trim).filter(|target| !target.is_empty());
    let named = target.or_else(|| wire.get("instance").and_then(Value::as_str));
    let instance = named.and_then(id).ok_or(Refusal::NotAnId)?;
    Ok(Request { instance })
}

/// Initiative@1's NEXT over the participants in rank order, alive or dead:
/// the index the turn passes to, and whether the search wrapped past the
/// last rank. From `current`, the next living one after it, wrapping to the
/// start — itself last, when it is the only one alive; with no current turn,
/// the first living one, no wrap. `None` when no one is alive.
pub fn next(alive: &[bool], current: Option<usize>) -> Option<(usize, bool)> {
    let living = |at: &usize| alive[*at];
    match current {
        Some(current) => (current + 1..alive.len())
            .find(living)
            .map(|at| (at, false))
            .or_else(|| (0..=current).find(living).map(|at| (at, true))),
        None => (0..alive.len()).find(living).map(|at| (at, false)),
    }
}

/// `request` made the command written on the combat's partition, against
/// `combat`, the instance's open combat, and `participants`, its entries, as
/// the commands queued will leave them. The participants are taken in rank
/// order — a tie by entry id, as the carousel orders them; a current entry
/// that is none of them is no current turn.
///
/// # Errors
///
/// [`Refusal::NoOpenCombat`] when `combat` is none, [`Refusal::NoOneAlive`]
/// when no participant is alive.
pub fn prepare(
    _request: &Request,
    combat: Option<&Combat>,
    participants: &[Participant],
) -> Result<Prepared, Refusal> {
    let combat = combat.ok_or(Refusal::NoOpenCombat)?;
    let mut order: Vec<&Participant> = participants.iter().collect();
    order.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.entry.cmp(&b.entry)));
    let alive: Vec<bool> = order.iter().map(|p| p.alive).collect();
    let current = combat
        .current
        .as_ref()
        .and_then(|current| order.iter().position(|p| p.entry == *current));
    let (at, wrapped) = next(&alive, current).ok_or(Refusal::NoOneAlive)?;
    Ok(Prepared {
        target_id: combat.id.clone(),
        payload: Row::from([
            (
                "currentEntryId".to_owned(),
                Value::from(order[at].entry.clone()),
            ),
            ("round".to_owned(), Value::from(i64::from(wrapped))),
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
