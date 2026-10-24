//! DataCapability `table.removeToken` (`data-capability.json`): the DM
//! clears a token off the map — a dead monster, in one click — and the
//! combat carries on without it.
//!
//! The engine writes one partition per command, and a token is its own,
//! `BattlemapInstance/<its id>`: removing it is one deletion there, which the
//! engine keeps as a tombstone (a soft delete, reversible through the
//! command journal), and nothing else. Its initiative entry is a row of its
//! own, which the command never writes: the tables follow the aggregate's
//! relations once the token's row goes (`initiativeEntryTokenId`, cascade;
//! `currentEntryId`, nullify), and the order the carousel shows closes up by
//! itself, each place counted from the ranks that remain (`orderIndex`,
//! derived). A token that holds the turn is passed by `table.nextTurn` first,
//! sent before this command: the next living participant then has it.
//!
//! What the DM sends names the token, under `token`, and the instance it
//! stands on as the command's target — or the token itself as the target;
//! [`request`] reads it, and [`prepare`] judges it against the token's row
//! as the queue will leave it:
//!
//! - a removal naming no uuid is refused before it is queued ([`Refusal`]);
//! - a token the engine does not hold, one it removed, or one of another
//!   instance than the target, is none to remove (`instance-isolated`).

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.removeToken";

/// The aggregate a token is a row of.
const AGGREGATE: &str = "BattlemapInstance";

/// One partition's row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What the DM asked: the token, and the instance it should stand on when
/// the target named one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The token's id, lowercase and hyphenated.
    pub token: String,
    /// The instance the command targets, lowercase and hyphenated; `None`
    /// when the token is the target.
    pub instance: Option<String>,
}

/// Why a removal is refused before it is queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The token, or the instance, is not named by a uuid.
    NotAnId(&'static str),
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
            Self::UnknownToken => f.write_str("ce token n'est pas sur cette instance"),
        }
    }
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Removes)
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
/// [`Refusal::NotAnId`] when the token or the instance is not a uuid.
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
    Ok(Request {
        // The token named as its own target stands on no instance named.
        instance: instance.filter(|instance| *instance != token),
        token,
    })
}

/// Whether the removal `request` asks may be queued against `token`, the
/// token's row as the commands queued there will leave it. A deletion
/// writes no field: its partition is tombstoned whole.
///
/// # Errors
///
/// [`Refusal::UnknownToken`] when `token` holds no such token, or holds it
/// on another instance than the one the request names.
pub fn prepare(request: &Request, token: &Row) -> Result<(), Refusal> {
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
    Ok(())
}

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// A removal made the deletion of the token's partition ([`prepare`]),
/// against that partition as the queue leaves it — the command's target may
/// be the instance. One naming no id is refused, 422; one naming a token
/// that is not there, or not on the instance named, 404 — nothing queued.
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
        let token = context.row(&partition(&request.token)).await?;
        prepare(&request, &token).map_err(refused)?;
        Ok(plugins::Prepared {
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
