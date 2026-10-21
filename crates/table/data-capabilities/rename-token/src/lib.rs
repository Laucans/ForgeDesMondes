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
//! - a token the engine does not hold, one it removed, or one of another
//!   instance than the target, is none to rename (`instance-isolated`).

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::{Value, json};
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.renameToken";

/// The aggregate a token is a row of.
const AGGREGATE: &str = "BattlemapInstance";

/// One partition's row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What the DM asked: the token, the instance it should stand on when the
/// target named one, and its new name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The token's id, lowercase and hyphenated.
    pub token: String,
    /// The instance the command targets, lowercase and hyphenated; `None`
    /// when the token is the target.
    pub instance: Option<String>,
    /// The new name, without the blanks around it.
    pub name: String,
}

/// Why a rename is refused before it is queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The token, or the instance, is not named by a uuid.
    NotAnId(&'static str),
    /// The name is absent, not text, or blanks alone.
    NoName,
    /// The name holds the character U+0000, which `text` cannot.
    Nul,
    /// The engine holds no such token, removed it, or holds it on another
    /// instance.
    UnknownToken,
}

impl Refusal {
    /// Whether the refusal is of a token that is not there (404) rather
    /// than of a request that cannot be done (422).
    #[must_use]
    pub fn not_found(&self) -> bool {
        matches!(self, Self::UnknownToken)
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::NoName => f.write_str("multi-drop-numbering: un token a un nom"),
            Self::Nul => f.write_str("name contient un caractère nul"),
            Self::UnknownToken => f.write_str("ce token n'est pas sur cette instance"),
        }
    }
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Renames)
}

/// The partition of the token `token`.
#[must_use]
pub fn partition(token: &str) -> String {
    format!("{AGGREGATE}/{token}")
}

/// What `wire`, sent to `target`, asks: the token is `wire.token`, on the
/// instance `target` — or `target` itself when `wire.token` is absent or
/// null.
///
/// # Errors
///
/// [`Refusal::NotAnId`] when the token or the instance is not a uuid,
/// [`Refusal::NoName`] when the name is not text with something besides
/// blanks, [`Refusal::Nul`] when it holds U+0000.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    // A `token` of null names no token, as an absent one.
    let (token, instance) = match wire.get("token").filter(|token| !token.is_null()) {
        Some(token) => (
            token
                .as_str()
                .and_then(uuid)
                .ok_or(Refusal::NotAnId("token"))?,
            target
                .map(|target| uuid(target).ok_or(Refusal::NotAnId("instance")))
                .transpose()?,
        ),
        None => (
            target.and_then(uuid).ok_or(Refusal::NotAnId("token"))?,
            None,
        ),
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

/// The payload `request` writes on the token's partition — its name, and
/// nothing else — against `token`, its row as the commands queued there
/// will leave it.
///
/// # Errors
///
/// [`Refusal::UnknownToken`] when `token` holds no such token, or holds it
/// on another instance than the one the request names.
pub fn prepare(request: &Request, token: &Row) -> Result<Row, Refusal> {
    let held = |field: &str| token.get(field).and_then(Value::as_str).and_then(uuid);
    let on = held("tokenInstanceId").ok_or(Refusal::UnknownToken)?;
    if held("tokenId").as_deref() != Some(request.token.as_str())
        || request
            .instance
            .as_ref()
            .is_some_and(|instance| *instance != on)
    {
        return Err(Refusal::UnknownToken);
    }
    Ok(Row::from([("name".to_owned(), json!(request.name))]))
}

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// A rename made the command written on the token's partition ([`prepare`]),
/// against that partition as the queue leaves it — the command's target may
/// be the instance, whose row `base` is. One naming no id or no name it can
/// write is refused, 422; one naming a token that is not there, or not on
/// the instance named, 404 — nothing queued.
struct Renames;

#[async_trait]
impl Prepare for Renames {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let request = request(payload, target_id).map_err(refused)?;
        // A partition the engine removed is empty, as one it never wrote.
        let token = context.row(&partition(&request.token)).await?;
        let payload = prepare(&request, &token).map_err(refused)?;
        Ok(plugins::Prepared {
            payload,
            target_id: Some(request.token),
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
