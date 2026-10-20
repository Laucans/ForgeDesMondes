//! DataCapability `table.removeToken` (`data-capability.json`): the DM
//! clears a token off the map — a dead monster, in one click — and the
//! combat carries on without it.
//!
//! The engine writes one partition per command, and a token is its own,
//! `BattlemapInstance/<its id>`: removing it is one deletion there, which the
//! engine keeps as a tombstone (a soft delete, reversible through the
//! command journal), and nothing else. Its initiative entry is a row of its
//! own, which the command never writes: the tables follow the aggregate's
//! relations once the token's row goes (`initiativeEntryTokenId`, cascade),
//! and the order the carousel shows closes up by itself, each place counted
//! from the ranks that remain (`orderIndex`, derived). A token that holds the
//! turn is passed by `table.nextTurn` first, sent before this command: the
//! next living participant then has it. What the DM sends names the token,
//! under `token`, and the instance it stands on as the command's target — or
//! the token itself as the target; [`request`] reads it, and [`prepare`]
//! makes it the command, against the token's row as the queue will leave it:
//!
//! - a command naming no uuid is refused before it is queued ([`Refusal`]);
//! - a token the engine does not hold, or one of another instance than the
//!   target, is none to remove (`instance-isolated`).

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.removeToken";

/// The aggregate whose partition a token is.
const AGGREGATE: &str = "BattlemapInstance";

/// A row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What the DM asked: the token, and the instance it should stand on when
/// the target named one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The token's id, lowercase.
    pub token: String,
    /// The instance the command targets, lowercase; `None` when the token is
    /// the target.
    pub instance: Option<String>,
}

/// The command a removal is: the token's id, which its partition is, and no
/// field — a deletion writes none, its partition is tombstoned whole.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    /// The token's id.
    pub target_id: String,
    /// Empty.
    pub payload: Row,
}

/// Why a removal is refused before it is queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The token, or the instance, is not named by a uuid.
    NotAnId(&'static str),
    /// The engine holds no such token, or holds it on another instance.
    UnknownToken,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
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
/// [`Refusal::NotAnId`] when the token or the instance is not a uuid.
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
    Ok(Request {
        // The token named as its own target stands on no instance named.
        instance: instance.filter(|instance| *instance != token),
        token,
    })
}

/// `request` made the deletion of the token's partition, against `base`, its
/// row as the commands queued there will leave it.
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
    Ok(Prepared {
        target_id: request.token.clone(),
        payload: Row::new(),
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
