//! DataCapability `table.reorderInitiative` (`data-capability.json`): the DM
//! drags a participant to another place in the carousel — a delayed action,
//! a ruling — and the new order is saved.
//!
//! The ranks are computed at the launch (`initiative-tiebreak`), then the
//! DM's to edit. What the DM sends is the whole order of the instance's open
//! combat, every participant once, named by its token or by its initiative
//! entry, under `order`, and the instance as the command's target — or under
//! `instance`; [`request`] reads it. The engine writes one partition per
//! command and judges a row alone: each entry is its own row,
//! `BattlemapInstance/<its id>`, as `table.startCombat` seats it. A reorder
//! is the command that writes the first entry's rank, then its follow-ups
//! ([`plugins::Prepared::then`], [`follow_ups`]), one per other participant,
//! each carrying the order and the entry it ranks, judged again, and taken
//! only once the entries ahead of its own hold their new rank in the queue
//! ([`follows`]): one sent alone, off a reorder, is refused. Every
//! entry takes its place in the order as its rank, 1 to n, without gap or
//! duplicate ([`ordered`]), and nothing else: never a visibility
//! (`hidden-keeps-rank`: a hidden participant takes the rank it is given,
//! like any other), never the turn nor the round, never a participant added
//! or removed.
//!
//! What one row cannot tell — the instance's open combat and its
//! participants — is read from the engine's rows before anything is queued
//! ([`LINEUP`], [`Lineup`]), and the combat's own row as the queue leaves it:
//!
//! - an order the tables could never hold is refused before it is queued
//!   ([`Refusal`]): the instance and each participant are uuids, the order
//!   a list of them, not empty;
//! - an instance neither the tables nor the engine hold is not found;
//! - an instance with no open combat has no order to rewrite;
//! - an order that omits a participant, repeats one, or names one that is
//!   not in the combat is refused: no participant is added or removed.
//!
//! A reorder is not atomic with its follow-ups: one refused — a participant
//! removed in between — stops the ones after it, and the ranks written so
//! far stay. The DM sends the order again. That is the milestone's accepted
//! risk, as for `table.startCombat`.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::api::Submission;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::{Value, json};
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.reorderInitiative";

/// The aggregate a combat and an entry are rows of.
const AGGREGATE: &str = "BattlemapInstance";

/// What the tables and the engine's rows hold of the instance `$1`: whether
/// the table or the engine holds the instance; its open combat, the latest
/// started whose row the engine holds without an end; and that combat's
/// participants, each its entry and its token, by rank — an entry the
/// engine holds whose token it has not removed. What the engine tombstoned
/// is not there.
pub const LINEUP: &str = "WITH gone AS (SELECT partition FROM dataguard_rows WHERE tombstoned),
    combat AS (
        SELECT lower(r.fields ->> 'combatId') AS id
        FROM dataguard_rows r
        WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
            AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'combatId')
            AND lower(r.fields ->> 'combatInstanceId') = $1::text
            AND r.fields ->> 'endedAt' IS NULL
        ORDER BY r.fields ->> 'startedAt' DESC, r.partition
        LIMIT 1
    )
    SELECT
        EXISTS (SELECT 1 FROM battlemap_instance i WHERE i.id = $1::text::uuid
                AND 'BattlemapInstance/' || $1::text NOT IN (SELECT partition FROM gone))
            OR EXISTS (SELECT 1 FROM dataguard_rows r
                WHERE r.partition = 'BattlemapInstance/' || $1::text AND NOT r.tombstoned
                    AND r.fields ->> 'battlemapInstanceId' IS NOT NULL)
            AS instance,
        (SELECT id FROM combat) AS combat,
        (SELECT coalesce(jsonb_agg(jsonb_build_object(
                    'entry', lower(e.fields ->> 'initiativeEntryId'),
                    'token', lower(e.fields ->> 'initiativeEntryTokenId'))
                ORDER BY e.fields -> 'rank', e.partition), '[]')
            FROM dataguard_rows e
            JOIN combat c ON lower(e.fields ->> 'initiativeEntryCombatId') = c.id
            WHERE e.partition LIKE 'BattlemapInstance/%' AND NOT e.tombstoned
                AND e.partition = 'BattlemapInstance/' || lower(e.fields ->> 'initiativeEntryId')
                AND EXISTS (SELECT 1 FROM dataguard_rows t
                    WHERE t.partition = 'BattlemapInstance/' || lower(e.fields ->> 'initiativeEntryTokenId')
                        AND NOT t.tombstoned))
            AS participants";

/// One partition's row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// The partition of the aggregate a combat or an entry of id `id` is
/// written on.
#[must_use]
pub fn partition(id: &str) -> String {
    format!("{AGGREGATE}/{id}")
}

/// What a reorder asks for, ids lowercase and hyphenated.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    /// The battlemap instance whose open combat is reordered.
    pub instance: String,
    /// Each participant's token, or entry, first rank first.
    pub order: Vec<String>,
    /// The entry a follow-up ranks; `None` for the DM's command, which ranks
    /// the first.
    pub entry: Option<String>,
}

/// A participant of the open combat: its initiative entry, and its token.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Participant {
    /// The entry's id: its partition.
    pub entry: String,
    /// The token's id.
    pub token: String,
}

/// What the tables and the queue hold of the instance a reorder names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lineup {
    /// Whether the tables or the engine hold the instance.
    pub instance: bool,
    /// Its open combat's id; `None` when it has none.
    pub combat: Option<String>,
    /// That combat's participants.
    pub participants: Vec<Participant>,
}

/// Why a reorder is refused before it is queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The instance, a participant of the order, or a follow-up's entry, is
    /// not named by a uuid.
    NotAnId(&'static str),
    /// The order is absent, not a list, or empty.
    NoOrder,
    /// The tables hold no such instance.
    UnknownInstance,
    /// The instance has no open combat.
    NoOpenCombat,
    /// The order omits a participant, repeats one, or names one that is not
    /// in the combat; or a follow-up's entry is not in its order.
    NotTheParticipants,
    /// A follow-up whose entries ahead in the order do not hold their new
    /// rank yet: no reorder queued it.
    OutOfSequence,
}

impl Refusal {
    /// Whether the refusal is of something the request names that does not
    /// exist (404) rather than of a request that cannot be done (422).
    #[must_use]
    pub fn not_found(&self) -> bool {
        matches!(self, Self::UnknownInstance)
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::NoOrder => f.write_str("un ordre est une liste de participants"),
            Self::UnknownInstance => f.write_str("cette instance de battlemap n'existe pas"),
            Self::NoOpenCombat => f.write_str("cette instance n'a pas de combat en cours"),
            Self::NotTheParticipants => {
                f.write_str("l'ordre nomme chaque participant du combat une fois, et eux seuls")
            }
            Self::OutOfSequence => {
                f.write_str("ce rang suit ceux des participants qui le précèdent dans l'ordre")
            }
        }
    }
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Reorders)
}

/// The reorder `wire` asks for on `target`, the instance the command names
/// — else the payload's `instance`.
///
/// # Errors
///
/// [`Refusal::NotAnId`] for an instance, a participant or an entry that is
/// no uuid, [`Refusal::NoOrder`] for an order that is no list, or an empty
/// one.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    let instance = target
        .or_else(|| wire.get("instance").and_then(Value::as_str))
        .and_then(uuid)
        .ok_or(Refusal::NotAnId("instance"))?;
    let order = wire
        .get("order")
        .and_then(Value::as_array)
        .filter(|order| !order.is_empty())
        .ok_or(Refusal::NoOrder)?
        .iter()
        .map(|named| {
            named
                .as_str()
                .and_then(uuid)
                .ok_or(Refusal::NotAnId("order"))
        })
        .collect::<Result<_, _>>()?;
    // An `entry` of null names no follow-up, as an absent one.
    let entry = match wire.get("entry") {
        None | Some(Value::Null) => None,
        Some(entry) => Some(
            entry
                .as_str()
                .and_then(uuid)
                .ok_or(Refusal::NotAnId("entry"))?,
        ),
    };
    Ok(Request {
        instance,
        order,
        entry,
    })
}

/// The entries of `lineup`'s open combat in the order `request` gives,
/// first rank first: each named participant's entry, its rank its place.
///
/// # Errors
///
/// [`Refusal::UnknownInstance`], [`Refusal::NoOpenCombat`], and
/// [`Refusal::NotTheParticipants`] when the order is not every participant
/// once.
pub fn ordered(request: &Request, lineup: &Lineup) -> Result<Vec<String>, Refusal> {
    if !lineup.instance {
        return Err(Refusal::UnknownInstance);
    }
    if lineup.combat.is_none() {
        return Err(Refusal::NoOpenCombat);
    }
    let participants = &lineup.participants;
    if request.order.len() != participants.len() {
        return Err(Refusal::NotTheParticipants);
    }
    let mut placed = vec![false; participants.len()];
    let mut entries = Vec::with_capacity(participants.len());
    for named in &request.order {
        let at = participants
            .iter()
            .position(|p| p.token == *named || p.entry == *named)
            .filter(|at| !placed[*at])
            .ok_or(Refusal::NotTheParticipants)?;
        placed[at] = true;
        entries.push(participants[at].entry.clone());
    }
    Ok(entries)
}

/// The rank of `entry` in `entries`, the order [`ordered`] gave: its place,
/// from 1.
///
/// # Errors
///
/// [`Refusal::NotTheParticipants`] when `entry` is not in the order.
pub fn rank(entries: &[String], entry: &str) -> Result<i64, Refusal> {
    entries
        .iter()
        .zip(1_i64..)
        .find_map(|(each, rank)| (each == entry).then_some(rank))
        .ok_or(Refusal::NotTheParticipants)
}

/// Whether a follow-up comes after the ranks it follows: `before`, the rank
/// each entry ahead of its own in the order holds as the queue leaves it, is
/// that entry's place — the reorder's command and the follow-ups before it
/// queued. A follow-up never ranks the first entry, the command's; one sent
/// alone, off a reorder, finds the ranks ahead as they were.
#[must_use]
pub fn follows(before: &[Value]) -> bool {
    !before.is_empty()
        && before
            .iter()
            .zip(1_i64..)
            .all(|(rank, place)| rank.as_i64() == Some(place))
}

/// The follow-ups of a reorder of `instance` whose order is `entries`, the
/// command itself ranking the first: one for each other entry, in rank
/// order, carrying the whole order.
#[must_use]
pub fn follow_ups(instance: &str, entries: &[String]) -> Vec<Submission> {
    entries
        .iter()
        .skip(1)
        .map(|entry| Submission {
            data_capability: DATA_CAPABILITY.to_owned(),
            payload: Row::from([
                ("order".to_owned(), json!(entries)),
                ("entry".to_owned(), json!(entry)),
            ]),
            target_id: Some(instance.to_owned()),
            ..Submission::default()
        })
        .collect()
}

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// A reorder made the rank of one entry ([`ordered`], [`rank`]), judged
/// against the engine's rows ([`LINEUP`]) and the combat's row as the queue
/// leaves it; the DM's command ranks the first entry and makes the others
/// its follow-ups ([`follow_ups`]), each taken once the entries ahead of its
/// own hold their new rank ([`follows`]). One naming no id, or no order, is
/// refused, 422; one naming an instance that is not there, 404; one whose
/// order is not the combat's participants, or a follow-up out of sequence,
/// 422 — nothing queued.
struct Reorders;

#[async_trait]
impl Prepare for Reorders {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let request = request(payload, target_id).map_err(refused)?;
        // Over memory the tables hold nothing: the participants cannot be
        // listed.
        let Some(rows) = context.rows else {
            return Err(plugins::Refusal::unprocessable(
                "les participants du combat n'ont pas pu être lus",
            ));
        };
        let held = rows
            .rows(LINEUP, &[json!(request.instance)])
            .await
            .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
        let held = &held[0];
        let mut combat = held["combat"].as_str().map(str::to_owned);
        // An end queued since closes the combat.
        if let Some(id) = &combat {
            let row = context.row(&partition(id)).await?;
            if row.get("endedAt").is_some_and(|ended| !ended.is_null()) {
                combat = None;
            }
        }
        let lineup = Lineup {
            instance: held["instance"].as_bool().unwrap_or(false),
            combat,
            participants: held["participants"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| Participant {
                    entry: p["entry"].as_str().unwrap_or_default().to_owned(),
                    token: p["token"].as_str().unwrap_or_default().to_owned(),
                })
                .collect(),
        };
        let entries = ordered(&request, &lineup).map_err(refused)?;
        // The DM's command ranks the first entry, and its follow-ups the
        // others.
        let Some(entry) = request.entry else {
            return Ok(plugins::Prepared {
                target_id: Some(entries[0].clone()),
                payload: Row::from([("rank".to_owned(), json!(1))]),
                then: follow_ups(&request.instance, &entries),
                ..plugins::Prepared::default()
            });
        };
        let rank = rank(&entries, &entry).map_err(refused)?;
        let mut before = Vec::new();
        for ahead in entries.iter().take_while(|ahead| **ahead != entry) {
            let row = context.row(&partition(ahead)).await?;
            before.push(row.get("rank").cloned().unwrap_or(Value::Null));
        }
        if !follows(&before) {
            return Err(refused(Refusal::OutOfSequence));
        }
        Ok(plugins::Prepared {
            target_id: Some(entry),
            payload: Row::from([("rank".to_owned(), json!(rank))]),
            ..plugins::Prepared::default()
        })
    }
}

/// The engine's refusal of `refusal`: 404 for what is not there, 422
/// otherwise.
fn refused(refusal: Refusal) -> plugins::Refusal {
    if refusal.not_found() {
        plugins::Refusal::not_found(refusal.to_string())
    } else {
        plugins::Refusal::unprocessable(refusal.to_string())
    }
}
