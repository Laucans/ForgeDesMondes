//! DataCapability `table.endCombat` (`data-capability.json`): the DM closes
//! the fight on a battlemap instance — the carousel goes, the map keeps its
//! tokens and their wounds, and a new combat may start later.
//!
//! The engine writes one partition per command and judges a row alone: a
//! combat is its own row, `BattlemapInstance/<its id>`, as `table.startCombat`
//! writes it, and an end is one command there, carrying its end and no
//! current turn. Its tokens, their HP and their conditions are rows of their
//! own, which it never writes; its entries stay too, the order as it stood.
//!
//! What the DM sends names the instance, as the command's target or under
//! `instance`; [`request`] reads it. The instance's combats with no end are
//! found among the engine's rows ([`COMBATS`]), then each is read as the
//! queue leaves it — an end queued ahead is in it ([`open`]) — and
//! [`prepare`] makes the latest one still open the command:
//!
//! - an instance not named by a uuid is refused before it is queued
//!   ([`Refusal::NotAnId`]);
//! - one with no combat open, applied or ended by a command queued ahead,
//!   ends none ([`Refusal::NoOpenCombat`], `one-active-combat`): once ended,
//!   a combat stays ended, and the instance may start another.
//!
//! Wiping the map — its tokens, its combat and their initiative — is
//! `table.resetBattlemap`'s, not this command's.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.endCombat";

/// The aggregate a combat is a row of.
const AGGREGATE: &str = "BattlemapInstance";

/// The instance `$1`'s combats with no end among the engine's rows, the
/// latest started first, and now, `$2` milliseconds since the epoch: what
/// the engine tombstoned is not there. A combat queued and not yet applied
/// is not either: an end follows a start.
pub const COMBATS: &str = "SELECT coalesce((SELECT jsonb_agg(lower(c.fields ->> 'combatId')
                ORDER BY c.fields ->> 'startedAt' DESC NULLS LAST, lower(c.fields ->> 'combatId'))
            FROM dataguard_rows c
            WHERE c.partition LIKE 'BattlemapInstance/%' AND NOT c.tombstoned
                AND c.partition = 'BattlemapInstance/' || lower(c.fields ->> 'combatId')
                AND lower(c.fields ->> 'combatInstanceId') = $1::text
                AND c.fields ->> 'endedAt' IS NULL),
            '[]') AS combats,
        to_timestamp($2::int8 / 1000.0) AS now";

/// One partition's row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What the DM asked: the end of this instance's combat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The instance's id, lowercase and hyphenated.
    pub instance: String,
}

/// The command an end is: the combat's id, which its partition is, and the
/// two fields it writes.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The combat's id.
    pub target_id: String,
    /// `endedAt`, now, and `currentEntryId` null: no turn is anyone's.
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
            Self::NoOpenCombat => {
                f.write_str("one-active-combat: aucun combat n'est en cours sur cette instance")
            }
        }
    }
}

/// The partition of the combat of id `id`.
#[must_use]
pub fn partition(id: &str) -> String {
    format!("{AGGREGATE}/{id}")
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Ends)
}

/// What `wire`, sent to `target`, asks: the end of the instance `target`'s
/// combat — or `wire.instance`'s when the command names no target.
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

/// The id of the combat `row` is, as the queue leaves it, when it is an
/// open combat of `instance`: `None` when the row is no combat's, another
/// instance's, or ended — an end queued ahead included.
#[must_use]
pub fn open(row: &Row, instance: &str) -> Option<String> {
    let text = |field: &str| row.get(field).and_then(Value::as_str).and_then(uuid);
    let id = text("combatId")?;
    (text("combatInstanceId").as_deref() == Some(instance)
        && row.get("endedAt").is_none_or(Value::is_null))
    .then_some(id)
}

/// The end written on the partition of `open`, the instance's open combat,
/// stamped `now`.
///
/// # Errors
///
/// [`Refusal::NoOpenCombat`] when the instance has no combat open
/// (`one-active-combat`).
pub fn prepare(open: Option<&str>, now: &str) -> Result<Prepared, Refusal> {
    let combat = open.ok_or(Refusal::NoOpenCombat)?;
    Ok(Prepared {
        target_id: combat.to_owned(),
        payload: Row::from([
            ("endedAt".to_owned(), Value::from(now)),
            ("currentEntryId".to_owned(), Value::Null),
        ]),
    })
}

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// An end made the command on its combat's partition ([`prepare`]), against
/// the instance's combats as the queue leaves them: one naming no instance
/// is refused, 422; one on an instance with no open combat, 422 — nothing
/// queued. Over memory the engine holds no SQL to find a combat by: no
/// combat is open.
struct Ends;

#[async_trait]
impl Prepare for Ends {
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
            .rows(
                COMBATS,
                &[
                    Value::from(request.instance.as_str()),
                    Value::from(context.now_millis),
                ],
            )
            .await
            .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
        let found = &found[0];
        let mut combat = None;
        for id in found["combats"].as_array().into_iter().flatten() {
            let Some(id) = id.as_str() else {
                continue;
            };
            combat = open(&context.row(&partition(id)).await?, &request.instance);
            if combat.is_some() {
                break;
            }
        }
        let now = found["now"]
            .as_str()
            .ok_or_else(|| plugins::Refusal::unprocessable("l'heure n'a pas pu être lue"))?;
        let prepared = prepare(combat.as_deref(), now).map_err(refused)?;
        Ok(plugins::Prepared {
            target_id: Some(prepared.target_id),
            payload: prepared.payload,
            ..plugins::Prepared::default()
        })
    }
}

/// The engine's refusal of `refusal`: every one is of a request that cannot
/// be done, 422.
fn refused(refusal: Refusal) -> plugins::Refusal {
    plugins::Refusal::unprocessable(refusal.to_string())
}
