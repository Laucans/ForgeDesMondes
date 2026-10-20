//! DataCapability `table.renameToken` (`data-capability.json`): the DM
//! renames one token of a battlemap instance — « Bandit 2 » becomes
//! « Bandit blessé » — and nothing else changes.
//!
//! The engine writes one partition per command, and a token is its own,
//! `BattlemapInstance/<its id>`: a rename is one command there, carrying the
//! token's new name alone. Its number, its character, and the other tokens
//! of the same character — partitions of their own — are never written
//! (`multi-drop-numbering`). What the DM sends names the token, under
//! `token`, and the instance it stands on as the command's target — or the
//! token itself as the target; [`request`] reads it, and [`prepare`] makes
//! it the command, against the token's row as the queue will leave it:
//!
//! - a rename the tables could never hold is refused before it is queued
//!   ([`Refusal`]): an id is a uuid, the name is text with something besides
//!   blanks, and holds no character U+0000;
//! - the name is kept without the blanks around it;
//! - a token the engine does not hold, or one of another instance than the
//!   target, is none to rename (`instance-isolated`).

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.renameToken";

/// The aggregate whose partition a token is.
const AGGREGATE: &str = "BattlemapInstance";

/// A row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What the DM asked: the token, the instance it should stand on when the
/// target named one, and its new name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The token's id, lowercase.
    pub token: String,
    /// The instance the command targets, lowercase; `None` when the token is
    /// the target.
    pub instance: Option<String>,
    /// The new name, without the blanks around it.
    pub name: String,
}

/// The command a rename is: the token's id, which its partition is, and the
/// one field it writes.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    /// The token's id.
    pub target_id: String,
    /// `name`, and nothing else.
    pub payload: Row,
}

/// Why a rename is refused before it is queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The token, or the instance, is not named by a uuid.
    NotAnId(&'static str),
    /// The name is absent, not text, or blanks alone.
    NoName,
    /// The name holds the character U+0000, which `text` cannot.
    Nul,
    /// The engine holds no such token, or holds it on another instance.
    UnknownToken,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::NoName => f.write_str("un token a un nom"),
            Self::Nul => f.write_str("name contient un caractère nul"),
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
/// [`Refusal::NoName`] when the name is not text with something besides
/// blanks, [`Refusal::Nul`] when it holds U+0000.
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
    let name = wire
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or(Refusal::NoName)?;
    if name.contains('\0') {
        return Err(Refusal::Nul);
    }
    Ok(Request {
        // The token named as its own target stands on no instance named.
        instance: instance.filter(|instance| *instance != token),
        token,
        name: name.to_owned(),
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
        payload: Row::from([("name".to_owned(), Value::from(request.name.as_str()))]),
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
