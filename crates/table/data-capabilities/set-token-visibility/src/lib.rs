//! DataCapability `table.setTokenVisibility` (`data-capability.json`): the
//! DM hides a token from the players — an ambush waiting its moment — or
//! reveals it, and nothing else changes.
//!
//! The engine writes one partition per command, and a token is its own,
//! `BattlemapInstance/<its id>`: hiding or revealing it is one command there,
//! carrying its visibility alone. Its place in the combat is its initiative
//! entry's rank, a row of its own, which the command never writes: a hidden
//! participant keeps its rank, and revealing it moves neither its rank nor
//! anyone else's (`hidden-keeps-rank`). What the DM sends names the token,
//! under `token`, and the instance it stands on as the command's target — or
//! the token itself as the target; [`request`] reads it, and [`prepare`]
//! makes it the command, against the token's row as the queue will leave it:
//!
//! - a command the tables could never hold is refused before it is queued
//!   ([`Refusal`]): an id is a uuid, and the visibility is `true` or `false`;
//! - a token the engine does not hold, or one of another instance than the
//!   target, is none to hide (`instance-isolated`).
//!
//! What the player view shows of a hidden token is the read's
//! (`table.getBattlemapInstance`), not this command's.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.setTokenVisibility";

/// The aggregate whose partition a token is.
const AGGREGATE: &str = "BattlemapInstance";

/// A row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What the DM asked: the token, the instance it should stand on when the
/// target named one, and whether the players see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The token's id, lowercase.
    pub token: String,
    /// The instance the command targets, lowercase; `None` when the token is
    /// the target.
    pub instance: Option<String>,
    /// `false` hides the token, `true` reveals it.
    pub visible: bool,
}

/// The command a change of visibility is: the token's id, which its
/// partition is, and the one field it writes.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    /// The token's id.
    pub target_id: String,
    /// `visible`, and nothing else.
    pub payload: Row,
}

/// Why a change of visibility is refused before it is queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The token, or the instance, is not named by a uuid.
    NotAnId(&'static str),
    /// The visibility is absent, or not `true` or `false`.
    NotAVisibility,
    /// The engine holds no such token, or holds it on another instance.
    UnknownToken,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::NotAVisibility => f.write_str("visible vaut true ou false"),
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
/// [`Refusal::NotAVisibility`] when `visible` is not a boolean.
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
    let visible = wire
        .get("visible")
        .and_then(Value::as_bool)
        .ok_or(Refusal::NotAVisibility)?;
    Ok(Request {
        // The token named as its own target stands on no instance named.
        instance: instance.filter(|instance| *instance != token),
        token,
        visible,
    })
}

/// `request` made the command written on the token's partition, against
/// `base`, its row as the commands queued there will leave it.
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
        payload: Row::from([("visible".to_owned(), Value::Bool(request.visible))]),
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
