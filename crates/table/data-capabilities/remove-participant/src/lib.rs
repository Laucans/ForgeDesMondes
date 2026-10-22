//! DataCapability `table.removeParticipant` (`data-capability.json`): a
//! creature flees or surrenders — the DM takes it out of the turn order, its
//! token stays on the map, and the combat carries on without it.
//!
//! The engine writes one partition per command, and a participant's place in
//! the combat is its own, `BattlemapInstance/<its entry's id>`, as
//! `table.startCombat` seats it: taking it out is one deletion there, which
//! the engine keeps as a tombstone (a soft delete, reversible through the
//! command journal), and nothing else. Its token is a row of its own, which
//! the command never writes. The order the carousel shows closes up by
//! itself, each place counted from the ranks that remain (`orderIndex`,
//! derived): no rank is written. A participant that holds the turn is passed
//! by `table.nextTurn` first, sent before this command: the next living
//! participant then has it, and this command takes out one whose turn it no
//! longer is.
//!
//! What the DM sends names the participant, under `participant` — by its
//! initiative entry, or by its token as the carousel shows it — and the
//! instance as the command's target; or the participant itself as the
//! target.
//! [`request`] reads it; a token is its entry in the instance's open combat
//! among the engine's rows ([`ENTRIES_OF_TOKEN`]), and [`judge`] weighs the
//! entry against its row and its combat's as the queue will leave them:
//!
//! - a command naming no uuid is refused before it is queued ([`Refusal`]);
//! - an entry or a token the engine does not hold, removed, a token in no
//!   open combat, or an entry of a combat of another instance than the
//!   target, is no participant to remove (`instance-isolated`);
//! - an entry of a combat that ended, an end queued included, is out of no
//!   running order;
//! - the entry that holds the turn is not removed: the combat would keep
//!   naming a tombstone, and « Suivant » would restart from the first rank.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.removeParticipant";

/// The aggregate an entry, its combat and its token are rows of.
const AGGREGATE: &str = "BattlemapInstance";

/// The entries of the token `$1` in a combat with no end among the engine's
/// rows — a combat of the instance `$2`, when it is not null — the latest
/// started first: what the engine tombstoned is not there.
pub const ENTRIES_OF_TOKEN: &str = "SELECT lower(e.fields ->> 'initiativeEntryId') AS entry
    FROM dataguard_rows e
    JOIN dataguard_rows c
        ON c.partition = 'BattlemapInstance/' || lower(e.fields ->> 'initiativeEntryCombatId')
            AND NOT c.tombstoned AND c.fields ->> 'endedAt' IS NULL
            AND ($2::text IS NULL OR lower(c.fields ->> 'combatInstanceId') = $2::text)
    WHERE e.partition LIKE 'BattlemapInstance/%' AND NOT e.tombstoned
        AND e.partition = 'BattlemapInstance/' || lower(e.fields ->> 'initiativeEntryId')
        AND lower(e.fields ->> 'initiativeEntryTokenId') = $1::text
    ORDER BY c.fields ->> 'startedAt' DESC NULLS LAST, 1";

/// One partition's row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What the DM asked: the participant, and the instance its combat should be
/// of when the target named one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The participant's entry or token, lowercase and hyphenated.
    pub participant: String,
    /// The instance the command targets, lowercase and hyphenated; `None`
    /// when the entry is the target.
    pub instance: Option<String>,
}

/// What a participant's id names, by the row of its partition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Named {
    /// An initiative entry.
    Entry,
    /// A token, whose entry is to be found.
    Token,
    /// Nothing the engine holds.
    Nothing,
}

/// Why a removal is refused before it is queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The participant, or the instance, is not named by a uuid.
    NotAnId(&'static str),
    /// The engine holds no such entry, no token in an open combat, or holds
    /// it in a combat of another instance.
    UnknownParticipant,
    /// The entry's combat has ended: there is no running order to leave.
    CombatEnded,
    /// The entry holds the turn: `table.nextTurn` passes it first.
    HoldsTurn,
    /// A token's entry cannot be looked up: the engine runs over memory,
    /// with no rows to read.
    Unlisted,
}

impl Refusal {
    /// Whether the refusal is of a participant that is not there (404)
    /// rather than of a request that cannot be done (422).
    #[must_use]
    pub fn not_found(&self) -> bool {
        matches!(self, Self::UnknownParticipant)
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::UnknownParticipant => {
                f.write_str("ce participant n'est pas dans un combat de cette instance")
            }
            Self::CombatEnded => f.write_str("ce combat est terminé"),
            Self::HoldsTurn => {
                f.write_str("next-turn: ce participant a la main, passer le tour d'abord")
            }
            Self::Unlisted => f.write_str("les participants du combat n'ont pas pu être lus"),
        }
    }
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Removes)
}

/// The partition of the row of id `id`: an entry's, a combat's or a
/// token's, each its own.
#[must_use]
pub fn partition(id: &str) -> String {
    format!("{AGGREGATE}/{id}")
}

/// What `wire`, sent to `target`, asks: the participant is
/// `wire.participant`, in a combat of the instance `target` — or `target`
/// itself, an entry or a token, when `wire.participant` is absent or null.
///
/// # Errors
///
/// [`Refusal::NotAnId`] when the participant or the instance is not a uuid.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    // A `participant` of null names none, as an absent one.
    let (participant, instance) = match wire.get("participant").filter(|named| !named.is_null()) {
        Some(named) => (
            named
                .as_str()
                .and_then(uuid)
                .ok_or(Refusal::NotAnId("participant"))?,
            target
                .map(|target| uuid(target).ok_or(Refusal::NotAnId("instance")))
                .transpose()?,
        ),
        None => (
            target
                .and_then(uuid)
                .ok_or(Refusal::NotAnId("participant"))?,
            None,
        ),
    };
    Ok(Request {
        // The entry named as its own target is in no instance named.
        instance: instance.filter(|instance| *instance != participant),
        participant,
    })
}

/// What `id` names, `row` being its partition's row as the queue leaves it:
/// an entry, a token, or nothing the engine holds.
#[must_use]
pub fn named(row: &Row, id: &str) -> Named {
    if held(row, "initiativeEntryId").as_deref() == Some(id) {
        Named::Entry
    } else if held(row, "tokenId").as_deref() == Some(id) {
        Named::Token
    } else {
        Named::Nothing
    }
}

/// The combat `entry`, an entry's row, is of; `None` when the row is no
/// entry.
#[must_use]
pub fn combat_of(entry: &Row) -> Option<String> {
    held(entry, "initiativeEntryCombatId")
}

/// Whether the entry `id` may be taken out of its combat, against `entry`,
/// its row, and `combat`, its combat's — each as the commands queued there
/// will leave them — when the request names `instance`. A deletion writes
/// no field: its partition is tombstoned whole.
///
/// # Errors
///
/// [`Refusal::UnknownParticipant`] when `entry` holds no such entry or
/// `combat` no combat of the instance named; [`Refusal::CombatEnded`] when
/// the combat has an end; [`Refusal::HoldsTurn`] when the combat's turn is
/// the entry's.
pub fn judge(id: &str, instance: Option<&str>, entry: &Row, combat: &Row) -> Result<(), Refusal> {
    let of = combat_of(entry).ok_or(Refusal::UnknownParticipant)?;
    let on = held(combat, "combatInstanceId").ok_or(Refusal::UnknownParticipant)?;
    if held(entry, "initiativeEntryId").as_deref() != Some(id)
        || held(combat, "combatId").as_deref() != Some(of.as_str())
        || instance.is_some_and(|instance| instance != on)
    {
        return Err(Refusal::UnknownParticipant);
    }
    if !combat.get("endedAt").is_none_or(Value::is_null) {
        return Err(Refusal::CombatEnded);
    }
    if held(combat, "currentEntryId").as_deref() == Some(id) {
        return Err(Refusal::HoldsTurn);
    }
    Ok(())
}

/// `row`'s `field`, a uuid, lowercase and hyphenated; `None` when it is
/// absent or no uuid.
fn held(row: &Row, field: &str) -> Option<String> {
    row.get(field).and_then(Value::as_str).and_then(uuid)
}

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// A removal made the deletion of the entry's partition ([`judge`]), against
/// that partition and its combat's as the queue leaves them — the command's
/// target may be the instance, and the participant its token. One naming no
/// id is refused, 422; one naming a participant that is not there, or not
/// in a combat of the instance named, 404; one of an ended combat, or
/// holding the turn, 422 — nothing queued.
struct Removes;

#[async_trait]
impl Prepare for Removes {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let request = request(payload, target_id).map_err(refused)?;
        // A partition the engine removed is empty, as one it never wrote.
        let row = context.row(&partition(&request.participant)).await?;
        let mut entries = Vec::new();
        match named(&row, &request.participant) {
            Named::Entry => entries.push((request.participant.clone(), Some(row))),
            Named::Token => {
                let Some(rows) = context.rows else {
                    return Err(refused(Refusal::Unlisted));
                };
                let found = rows
                    .rows(
                        ENTRIES_OF_TOKEN,
                        &[
                            Value::from(request.participant.as_str()),
                            request.instance.as_deref().map_or(Value::Null, Value::from),
                        ],
                    )
                    .await
                    .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
                entries.extend(
                    found
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|found| found["entry"].as_str())
                        .map(|entry| (entry.to_owned(), None)),
                );
            }
            Named::Nothing => {}
        }
        let mut first = None;
        for (id, entry) in entries {
            // The participant's own row, when it is the entry, is read once.
            let entry = match entry {
                Some(entry) => entry,
                None => context.row(&partition(&id)).await?,
            };
            let combat = match combat_of(&entry) {
                Some(combat) => context.row(&partition(&combat)).await?,
                None => Row::new(),
            };
            match judge(&id, request.instance.as_deref(), &entry, &combat) {
                Ok(()) => {
                    return Ok(plugins::Prepared {
                        target_id: Some(id),
                        ..plugins::Prepared::default()
                    });
                }
                Err(refusal) => {
                    first.get_or_insert(refusal);
                }
            }
        }
        Err(refused(first.unwrap_or(Refusal::UnknownParticipant)))
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
