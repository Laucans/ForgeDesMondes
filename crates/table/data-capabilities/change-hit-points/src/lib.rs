//! DataCapability `table.changeHitPoints` (`data-capability.json`): the DM
//! applies damage or healing to one NPC, and to that token alone.
//!
//! The engine writes one partition per command and judges a row alone: a
//! token is its own row, `BattlemapInstance/<its id>`, as `table.placeTokens`
//! drops it. A change of HP names the instance — the command's target, or
//! the payload's `instance` — the token, and the change, `delta`; it lands
//! on the token's own row and steps its `hpCurrent` alone. The other tokens
//! of the same character, on their own partitions, are never written
//! (`hp-change-one-token`), nor a token of another instance
//! (`instance-isolated`).
//!
//! The command is relative: the engine adds its step to the HP as the
//! commands ahead leave them, so two blows sent at once both land. The step
//! is cut so an NPC's HP land between 0 and its HP max, never beyond
//! (`pj-hp-not-tracked`), against the token's row as the queue leaves it. A
//! PC's token tracks no HP: its step is the delta as sent, which the
//! DataGuard rejects on a token with no HP (`pj-hp-not-tracked`) — a
//! rejection the DM's command settles on, never a refusal before it queues.
//!
//! At 0 the token is dead: the table derives it from its HP alone
//! (`dead-derived`), as it derives the band the players see; the command
//! writes neither, and moves no turn. [`request`] reads the DM's request,
//! [`prepare`] turns it and the token's row into the step written, or into
//! the [`Refusal`] that says why it is not.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.changeHitPoints";

/// The aggregate a token is a row of.
const AGGREGATE: &str = "BattlemapInstance";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a change of HP asks for: the instance and the token by their ids,
/// lowercase and hyphenated, and the change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The battlemap instance the token stands on.
    pub instance: String,
    /// The token wounded or healed.
    pub token: String,
    /// The damage, negative, or the healing, positive.
    pub delta: i64,
}

/// Why a change of HP is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A field that must be a uuid is not one: `instance` or `token`.
    NotAnId(&'static str),
    /// `delta` is absent, or not a whole number.
    NotADelta,
    /// The engine holds no such token.
    UnknownToken,
    /// The token stands on another instance (`instance-isolated`).
    TokenOfAnotherInstance,
}

impl Refusal {
    /// Whether the refusal is of something the request names that does not
    /// exist (404) rather than of a request that cannot be done (422).
    #[must_use]
    pub fn not_found(&self) -> bool {
        matches!(self, Self::UnknownToken)
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::NotADelta => f.write_str("delta est un nombre entier de PV"),
            Self::UnknownToken => f.write_str("ce token n'existe pas"),
            Self::TokenOfAnotherInstance => {
                f.write_str("instance-isolated: ce token est sur une autre instance de battlemap")
            }
        }
    }
}

/// What a change of HP becomes before it is submitted: the partition it
/// writes and the step it adds there.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The token's id: the partition the change lands on.
    pub target_id: String,
    /// The fields written: `hpCurrent`, the step the engine adds.
    pub payload: Row,
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, HitPoints)
}

/// The change `wire` asks for on `target`, the instance the command names —
/// else the payload's `instance`.
///
/// # Errors
///
/// [`Refusal::NotAnId`] for an instance or a token that is no uuid,
/// [`Refusal::NotADelta`] for a `delta` that is no whole number.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    let instance = target
        .or_else(|| wire.get("instance").and_then(Value::as_str))
        .and_then(uuid)
        .ok_or(Refusal::NotAnId("instance"))?;
    let token = wire
        .get("token")
        .and_then(Value::as_str)
        .and_then(uuid)
        .ok_or(Refusal::NotAnId("token"))?;
    let delta = wire
        .get("delta")
        .and_then(Value::as_i64)
        .ok_or(Refusal::NotADelta)?;
    Ok(Request {
        instance,
        token,
        delta,
    })
}

/// The step changing the HP of the token `request` names, `token` being
/// that token's row as the queue leaves it: its `hpCurrent` on its own
/// partition, and nothing else. An NPC's step is the delta cut so its HP
/// land between 0 and its HP max; a PC's, which tracks no HP, the delta as
/// sent, for the DataGuard to reject.
///
/// # Errors
///
/// [`Refusal::UnknownToken`] when the row is no token's,
/// [`Refusal::TokenOfAnotherInstance`] when the token stands on another
/// instance than the one named.
pub fn prepare(request: &Request, token: &Row) -> Result<Prepared, Refusal> {
    let text = |field: &str| token.get(field).and_then(Value::as_str);
    if text("tokenId").is_none() {
        return Err(Refusal::UnknownToken);
    }
    if !text("tokenInstanceId")
        .is_some_and(|instance| instance.eq_ignore_ascii_case(&request.instance))
    {
        return Err(Refusal::TokenOfAnotherInstance);
    }
    let hp = |field: &str| token.get(field).and_then(Value::as_i64);
    let step = match (hp("hpCurrent"), hp("hpMax")) {
        // A negative max is no row the Judge lets through; `max(0)` only
        // keeps `clamp` from panicking on one.
        (Some(current), Some(max)) => current
            .saturating_add(request.delta)
            .clamp(0, max.max(0))
            .saturating_sub(current),
        _ => request.delta,
    };
    Ok(Prepared {
        target_id: request.token.clone(),
        payload: Row::from([("hpCurrent".to_owned(), Value::from(step))]),
    })
}

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// A change of HP made the step it adds on the token's partition
/// ([`prepare`]), judged against that partition as the queue leaves it: one
/// naming no id or no whole change is refused, 422; one naming a token that
/// is not there, 404; a token of another instance, 422 — nothing queued.
struct HitPoints;

#[async_trait]
impl Prepare for HitPoints {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let request = request(payload, target_id).map_err(refused)?;
        let token = context
            .row(&format!("{AGGREGATE}/{}", request.token))
            .await?;
        let prepared = prepare(&request, &token).map_err(refused)?;
        Ok(plugins::Prepared {
            target_id: Some(prepared.target_id),
            payload: prepared.payload,
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
