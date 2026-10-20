//! DataCapability `table.changeHitPoints` (`data-capability.json`): the DM
//! applies damage or healing to one NPC, and to that token alone.
//!
//! The engine writes one partition per command, and a token is its own,
//! `BattlemapInstance/<its id>`: a change of HP is one command there, carrying
//! its current HP alone — the other tokens of the same character, on their
//! own partitions, are never written (`hp-change-one-token`). The command is
//! relative: the engine adds its step to the HP as the commands ahead leave
//! them, so two blows sent at once both land. What the DM sends names the
//! token, under `token`, and the instance it stands on as the command's
//! target — or the token itself as the target — with the change, `delta`;
//! [`request`] reads it, and [`prepare`] makes it the command, against the
//! token's row as the queue will leave it:
//!
//! - a command the tables could never hold is refused before it is queued
//!   ([`Refusal`]): an id is a uuid, and the change a whole number;
//! - a token the engine does not hold, or one of another instance than the
//!   target, is none to wound (`instance-isolated`);
//! - an NPC's HP land between 0 and its HP max, never beyond: the step is the
//!   delta cut there (`pj-hp-not-tracked`);
//! - a PC's token tracks no HP: its step is the delta as sent, which the
//!   DataGuard rejects on a token with no HP (`pj-hp-not-tracked`).
//!
//! At 0 the token is dead: the table derives it from its HP alone
//! (`dead-derived`), as it derives the band the players see; the command
//! writes neither, and moves no turn.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.changeHitPoints";

/// The aggregate whose partition a token is.
const AGGREGATE: &str = "BattlemapInstance";

/// A row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What the DM asked: the token, the instance it should stand on when the
/// target named one, and the change of its HP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The token's id, lowercase.
    pub token: String,
    /// The instance the command targets, lowercase; `None` when the token is
    /// the target.
    pub instance: Option<String>,
    /// The damage, negative, or the healing, positive.
    pub delta: i64,
}

/// The command a change of HP is: the token's id, which its partition is,
/// and the one field it steps.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    /// The token's id.
    pub target_id: String,
    /// `hpCurrent`, the step the engine adds, and nothing else.
    pub payload: Row,
}

/// Why a change of HP is refused before it is queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The token, or the instance, is not named by a uuid.
    NotAnId(&'static str),
    /// The change is absent, or not a whole number.
    NotADelta,
    /// The engine holds no such token, or holds it on another instance.
    UnknownToken,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::NotADelta => f.write_str("delta est un nombre entier de PV"),
            Self::UnknownToken => f.write_str("ce token n'est pas sur cette instance"),
        }
    }
}

impl std::error::Error for Refusal {}

/// Whether `id` names this DataCapability, with or without its version.
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The partition of the token `token`.
pub fn partition(token: &str) -> String {
    format!("{AGGREGATE}/{token}")
}

/// What `wire`, sent to `target`, asks: the token is `wire.token`, on the
/// instance `target` — or `target` itself when `wire` names no token.
///
/// # Errors
///
/// [`Refusal::NotAnId`] when the token or the instance is not a uuid,
/// [`Refusal::NotADelta`] when `delta` is not a whole number.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    let target = target.map(str::trim).filter(|target| !target.is_empty());
    let (token, instance) = match wire.get("token") {
        Some(token) => (
            token
                .as_str()
                .and_then(id)
                .ok_or(Refusal::NotAnId("token"))?,
            target
                .map(|target| id(target).ok_or(Refusal::NotAnId("instance")))
                .transpose()?,
        ),
        None => (target.and_then(id).ok_or(Refusal::NotAnId("token"))?, None),
    };
    let delta = wire
        .get("delta")
        .and_then(Value::as_i64)
        .ok_or(Refusal::NotADelta)?;
    Ok(Request {
        // The token named as its own target stands on no instance named.
        instance: instance.filter(|instance| *instance != token),
        token,
        delta,
    })
}

/// `request` made the command written on the token's partition, against
/// `base`, its row as the commands queued there will leave it: an NPC's step
/// is the delta cut so its HP land between 0 and its HP max; a PC's, which
/// tracks no HP, the delta as sent, for the DataGuard to reject.
///
/// # Errors
///
/// [`Refusal::UnknownToken`] when `base` holds no such token, or holds it on
/// another instance than the one the request names.
pub fn prepare(request: &Request, base: &Row) -> Result<Prepared, Refusal> {
    let held = |field: &str| {
        base.get(field)
            .and_then(Value::as_str)
            .map(str::to_lowercase)
    };
    let on = held("tokenInstanceId").ok_or(Refusal::UnknownToken)?;
    if held("tokenId").as_deref() != Some(request.token.as_str())
        || request
            .instance
            .as_ref()
            .is_some_and(|instance| *instance != on)
    {
        return Err(Refusal::UnknownToken);
    }
    let hp = |field: &str| base.get(field).and_then(Value::as_i64);
    let step = match (hp("hpCurrent"), hp("hpMax")) {
        (Some(current), Some(max)) => {
            let landed = current.saturating_add(request.delta).clamp(0, max.max(0));
            landed.saturating_sub(current)
        }
        _ => request.delta,
    };
    Ok(Prepared {
        target_id: request.token.clone(),
        payload: Row::from([("hpCurrent".to_owned(), Value::from(step))]),
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
