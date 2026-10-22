//! DataCapability `table.nextTurn` (`data-capability.json`): the DM presses
//! « Suivant » and the turn of the instance's open combat passes on, as
//! Initiative@1's NEXT says.
//!
//! The engine writes one partition per command and judges a row alone: a
//! combat is its own row, `BattlemapInstance/<its id>`, and so is each
//! participant's place in it, `BattlemapInstance/<its entry id>`, and each
//! token, `BattlemapInstance/<its id>`, as `table.startCombat` and
//! `table.placeTokens` write them. A turn is one command on the combat's
//! row, carrying the entry whose turn it now is and the round's step. The
//! command is relative: the engine adds the step to the round as the
//! commands ahead leave it, so two « Suivant » sent one after the other both
//! count.
//!
//! What the DM sends names the instance, as the command's target or under
//! `instance`; [`request`] reads it. The instance's open combats and their
//! entries are found among the engine's rows ([`COMBATS`]), then each is
//! read as the queue leaves it — an end, a removal or a blow queued ahead is
//! in it ([`combat`], [`participant`]) — and [`prepare`] makes the command:
//!
//! - an instance not named by a uuid is refused before it is queued
//!   ([`Refusal::NotAnId`]); one with no open combat has no turn to pass
//!   ([`Refusal::NoOpenCombat`]), and a combat whose participants are all
//!   dead none to pass it to ([`Refusal::NoOneAlive`]);
//! - the turn goes to the next living participant after the current one in
//!   rank order (`next-turn`): a dead NPC — at 0 HP — is skipped
//!   (`dead-derived`), a hidden one takes its turn at its rank
//!   (`hidden-keeps-rank`), one whose token left the map keeps its rank and
//!   takes no turn;
//! - when the search wraps past the last rank, the round goes up by one,
//!   even when the first participants are dead and the turn lands later.
//!
//! No rank, participant or token is written: the command moves the turn and
//! the round alone.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.nextTurn";

/// The aggregate a combat, an entry and a token are rows of.
const AGGREGATE: &str = "BattlemapInstance";

/// The instance `$1`'s combats with no end among the engine's rows, the
/// latest started first, each with its entries' ids: what the engine
/// tombstoned is not there. A combat or an entry queued and not yet applied
/// is not either: « Suivant » follows a start.
pub const COMBATS: &str = "SELECT lower(c.fields ->> 'combatId') AS combat,
        coalesce((SELECT jsonb_agg(lower(e.fields ->> 'initiativeEntryId')
                ORDER BY lower(e.fields ->> 'initiativeEntryId'))
            FROM dataguard_rows e
            WHERE e.partition LIKE 'BattlemapInstance/%' AND NOT e.tombstoned
                AND e.partition = 'BattlemapInstance/' || lower(e.fields ->> 'initiativeEntryId')
                AND lower(e.fields ->> 'initiativeEntryCombatId') = lower(c.fields ->> 'combatId')),
            '[]') AS entries
    FROM dataguard_rows c
    WHERE c.partition LIKE 'BattlemapInstance/%' AND NOT c.tombstoned
        AND c.partition = 'BattlemapInstance/' || lower(c.fields ->> 'combatId')
        AND lower(c.fields ->> 'combatInstanceId') = $1::text
        AND c.fields ->> 'endedAt' IS NULL
    ORDER BY c.fields ->> 'startedAt' DESC NULLS LAST, 1";

/// One partition's row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What the DM asked: « Suivant » on this instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The instance's id, lowercase and hyphenated.
    pub instance: String,
}

/// The instance's open combat, as the queue leaves it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Combat {
    /// The combat's id, which its partition is.
    pub id: String,
    /// The entry whose turn it is; `None` when no turn is current.
    pub current: Option<String>,
}

/// One participant of the combat: its entry, its rank, and whether it takes
/// a turn — a PC always does, an NPC until its HP reach 0, and a token off
/// the map never.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Participant {
    /// The entry's id, lowercase.
    pub entry: String,
    /// Its rank, 1 first.
    pub rank: i64,
    /// Whether its token is on the map and alive.
    pub alive: bool,
}

/// The command a turn is: the combat's id, which its partition is, and the
/// two fields it writes.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The combat's id.
    pub target_id: String,
    /// `currentEntryId`, the entry whose turn it now is, and `round`, the
    /// step the engine adds: 1 when the turn wrapped, 0 otherwise.
    pub payload: Row,
}

/// Why a « Suivant » is refused before it is queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
            Self::NoOpenCombat => {
                f.write_str("next-turn: aucun combat n'est en cours sur cette instance")
            }
            Self::NoOneAlive => {
                f.write_str("dead-derived: aucun participant vivant à qui passer le tour")
            }
        }
    }
}

/// The partition of the row of id `id`: a combat's, an entry's or a
/// token's, each its own.
#[must_use]
pub fn partition(id: &str) -> String {
    format!("{AGGREGATE}/{id}")
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Turns)
}

/// What `wire`, sent to `target`, asks: « Suivant » on the instance
/// `target` — or `wire.instance` when the command names no target.
///
/// # Errors
///
/// [`Refusal::NotAnId`] when the instance is absent or not a uuid.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    let instance = target
        .or_else(|| wire.get("instance").and_then(Value::as_str))
        .and_then(uuid)
        .ok_or(Refusal::NotAnId)?;
    Ok(Request { instance })
}

/// The combat `row` is, as the queue leaves it, when it is an open combat
/// of `instance`: `None` when the row is no combat's, another instance's,
/// or ended — an end queued ahead included.
#[must_use]
pub fn combat(row: &Row, instance: &str) -> Option<Combat> {
    let text = |field: &str| row.get(field).and_then(Value::as_str).and_then(uuid);
    let id = text("combatId")?;
    let open = text("combatInstanceId").as_deref() == Some(instance)
        && row.get("endedAt").is_none_or(Value::is_null);
    open.then(|| Combat {
        id,
        current: text("currentEntryId"),
    })
}

/// The participant `entry` is in `combat`, `token` being its token's row,
/// both as the queue leaves them: `None` when the entry is no longer one of
/// the combat's. A token off the map — no row, or a row on another instance
/// than `instance` (`instance-isolated`) — takes no turn; one at 0 HP is
/// dead (`dead-derived`); a PC's, which tracks no HP, is always alive.
/// Whether it is hidden does not count (`hidden-keeps-rank`).
#[must_use]
pub fn participant(entry: &Row, token: &Row, combat: &str, instance: &str) -> Option<Participant> {
    let text = |field: &str| entry.get(field).and_then(Value::as_str).and_then(uuid);
    let id = text("initiativeEntryId")?;
    if text("initiativeEntryCombatId").as_deref() != Some(combat) {
        return None;
    }
    let rank = entry.get("rank").and_then(Value::as_i64)?;
    let on_map = token
        .get("tokenInstanceId")
        .and_then(Value::as_str)
        .and_then(uuid)
        .is_some_and(|on| on == instance);
    let alive = on_map
        && token
            .get("hpCurrent")
            .and_then(Value::as_i64)
            .is_none_or(|hp| hp > 0);
    Some(Participant {
        entry: id,
        rank,
        alive,
    })
}

/// Initiative@1's NEXT over the participants in rank order, alive or dead:
/// the index the turn passes to, and whether the search wrapped past the
/// last rank. From `current`, the next living one after it, wrapping to the
/// start — itself last, when it is the only one alive; with no current turn,
/// the first living one, no wrap. `None` when no one is alive.
#[must_use]
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

/// The command written on the combat's partition, against `combat`, the
/// instance's open combat, and `participants`, its entries, as the queue
/// leaves them. The participants are taken in rank order — a tie by entry
/// id; a current entry that is none of them is no current turn.
///
/// # Errors
///
/// [`Refusal::NoOpenCombat`] when `combat` is none, [`Refusal::NoOneAlive`]
/// when no participant is alive.
pub fn prepare(combat: Option<&Combat>, participants: &[Participant]) -> Result<Prepared, Refusal> {
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

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// A « Suivant » made the command on its combat's partition ([`prepare`]),
/// against the combat and its participants as the queue leaves them: one
/// naming no instance is refused, 422; one on an instance with no open
/// combat, or none alive in it, 422 — nothing queued. Over memory the engine
/// holds no SQL to find a combat by: no combat is open.
struct Turns;

#[async_trait]
impl Prepare for Turns {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let request = request(payload, target_id).map_err(refused)?;
        let Some(rows) = context.rows else {
            return Err(refused(Refusal::NoOpenCombat));
        };
        let found = rows
            .rows(COMBATS, &[Value::from(request.instance.as_str())])
            .await
            .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
        for found in found.as_array().into_iter().flatten() {
            let Some(id) = found["combat"].as_str() else {
                continue;
            };
            let row = context.row(&partition(id)).await?;
            let Some(combat) = combat(&row, &request.instance) else {
                continue;
            };
            let mut participants = Vec::new();
            for entry in found["entries"].as_array().into_iter().flatten() {
                let Some(entry) = entry.as_str() else {
                    continue;
                };
                let entry = context.row(&partition(entry)).await?;
                let token = match entry
                    .get("initiativeEntryTokenId")
                    .and_then(Value::as_str)
                    .and_then(uuid)
                {
                    Some(token) => context.row(&partition(&token)).await?,
                    None => Row::new(),
                };
                participants.extend(participant(&entry, &token, &combat.id, &request.instance));
            }
            let prepared = prepare(Some(&combat), &participants).map_err(refused)?;
            return Ok(plugins::Prepared {
                target_id: Some(prepared.target_id),
                payload: prepared.payload,
                ..plugins::Prepared::default()
            });
        }
        Err(refused(Refusal::NoOpenCombat))
    }
}

/// The engine's refusal of `refusal`: every one is of a request that cannot
/// be done, 422.
fn refused(refusal: Refusal) -> plugins::Refusal {
    plugins::Refusal::unprocessable(refusal.to_string())
}
