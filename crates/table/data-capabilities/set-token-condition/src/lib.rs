//! DataCapability `table.setTokenCondition` (`data-capability.json`): the DM
//! puts a condition badge on one token of a battlemap instance — « Bandit 2 »
//! is « Empoisonné », « À terre » — or takes it off, and nothing else
//! changes.
//!
//! The engine writes one partition per command, and a token is its own,
//! `BattlemapInstance/<its id>`: a badge set is one command there, carrying
//! the token's conditions alone. Its hit points, its death, and the other
//! tokens of the same character — partitions of their own — are never
//! written (`instance-isolated`). What the DM sends names the token, under
//! `token`, and the instance it stands on as the command's target — or the
//! token itself as the target —, the condition under `state`, and whether it
//! is on; [`request`] reads it, and [`prepare`] makes it the command, against
//! the token's row as the queue will leave it — the conditions it holds then,
//! with the badge put on or taken off (`state-persisted-reset`):
//!
//! - a badge the tables could never hold is refused before it is queued
//!   ([`Refusal`]): an id is a uuid, the condition is text with something
//!   besides blanks, and holds no character U+0000, and `on` is true or
//!   false;
//! - the condition is kept without the blanks around it;
//! - a badge put on is added after the others, once; one taken off is gone,
//!   the others keep their order;
//! - a token the engine does not hold, or one of another instance than the
//!   target, is none to mark (`instance-isolated`).

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.setTokenCondition";

/// The aggregate whose partition a token is.
const AGGREGATE: &str = "BattlemapInstance";

/// A row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What the DM asked: the token, the instance it should stand on when the
/// target named one, the condition, and whether it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The token's id, lowercase.
    pub token: String,
    /// The instance the command targets, lowercase; `None` when the token is
    /// the target.
    pub instance: Option<String>,
    /// The condition, without the blanks around it.
    pub state: String,
    /// Whether the badge is put on, or taken off.
    pub on: bool,
}

/// The command a badge set is: the token's id, which its partition is, and
/// the one field it writes.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    /// The token's id.
    pub target_id: String,
    /// `conditions`, and nothing else.
    pub payload: Row,
}

/// Why a badge set is refused before it is queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The token, or the instance, is not named by a uuid.
    NotAnId(&'static str),
    /// The condition is absent, not text, or blanks alone.
    NoState,
    /// The condition holds the character U+0000, which `text` cannot.
    Nul,
    /// `on` is absent, or neither true nor false.
    NoSwitch,
    /// The engine holds no such token, or holds it on another instance.
    UnknownToken,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::NoState => f.write_str("un état a un nom"),
            Self::Nul => f.write_str("state contient un caractère nul"),
            Self::NoSwitch => f.write_str("on vaut true ou false"),
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
/// instance `target` — or `target` itself when `wire` names no token, or a
/// null one.
///
/// # Errors
///
/// [`Refusal::NotAnId`] when the token or the instance is not a uuid,
/// [`Refusal::NoState`] when the condition is not text with something
/// besides blanks, [`Refusal::Nul`] when it holds U+0000,
/// [`Refusal::NoSwitch`] when `on` is not a boolean.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    let target = target.map(str::trim).filter(|target| !target.is_empty());
    // A token named null is none named, as the mock-up reads it.
    let (token, instance) = match wire.get("token").filter(|token| !token.is_null()) {
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
    let state = wire
        .get("state")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|state| !state.is_empty())
        .ok_or(Refusal::NoState)?;
    if state.contains('\0') {
        return Err(Refusal::Nul);
    }
    let on = wire
        .get("on")
        .and_then(Value::as_bool)
        .ok_or(Refusal::NoSwitch)?;
    Ok(Request {
        // The token named as its own target stands on no instance named.
        instance: instance.filter(|instance| *instance != token),
        token,
        state: state.to_owned(),
        on,
    })
}

/// `request` made the command written on the token's partition, against
/// `base`, its row as the commands queued there will leave it: the
/// conditions it holds, with the badge put on or taken off.
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
    let mut conditions: Vec<&str> = base
        .get("conditions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !request.on {
        conditions.retain(|state| *state != request.state);
    } else if !conditions.contains(&request.state.as_str()) {
        conditions.push(&request.state);
    }
    Ok(Prepared {
        target_id: request.token.clone(),
        payload: Row::from([("conditions".to_owned(), Value::from(conditions))]),
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
