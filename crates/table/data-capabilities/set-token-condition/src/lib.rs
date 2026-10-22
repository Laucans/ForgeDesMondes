//! DataCapability `table.setTokenCondition` (`data-capability.json`): the DM
//! puts a condition badge on one token of a battlemap instance — « Bandit 2 »
//! is « Empoisonné », « À terre » — or takes it off, and nothing else
//! changes.
//!
//! The engine writes one partition per command and judges a row alone: a
//! token is its own row, `BattlemapInstance/<its id>`, as `table.placeTokens`
//! drops it. A badge set names the instance — the command's target, or the
//! payload's `instance` — the token, the condition under `state`, and
//! whether it is `on`; it lands on the token's own row and writes its
//! `conditions` alone. Its hit points and its death are
//! `table.changeHitPoints`'; the other tokens of the same character, and
//! another instance's, are partitions of their own, never written
//! (`instance-isolated`).
//!
//! The conditions written are the token's as the queue leaves it, with the
//! badge put on or taken off, so that two badges set in a row both stay
//! (`state-persisted-reset`):
//!
//! - the condition is kept without the blanks around it, and is refused when
//!   it is blanks alone or holds U+0000, which `text` cannot;
//! - a badge put on is added after the others, once; one taken off is gone,
//!   the others keep their order.
//!
//! [`request`] reads the DM's request, [`prepare`] turns it and the token's
//! row into the row written, or into the [`Refusal`] that says why it is
//! not.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.setTokenCondition";

/// The aggregate a token is a row of.
const AGGREGATE: &str = "BattlemapInstance";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a badge set asks for: the instance and the token by their ids,
/// lowercase and hyphenated, the condition and whether it is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The battlemap instance the token stands on.
    pub instance: String,
    /// The token whose badge is set.
    pub token: String,
    /// The condition, without the blanks around it.
    pub state: String,
    /// `true` puts the badge on, `false` takes it off.
    pub on: bool,
}

/// Why a badge set is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A field that must be a uuid is not one: `instance` or `token`.
    NotAnId(&'static str),
    /// `state` is absent, not text, or blanks alone.
    NoState,
    /// `state` holds the character U+0000, which `text` cannot.
    Nul,
    /// `on` is absent, or not `true` or `false`.
    NoSwitch,
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
            Self::NoState => f.write_str("state est le nom d'un état"),
            Self::Nul => f.write_str("state contient un caractère nul"),
            Self::NoSwitch => f.write_str("on vaut true ou false"),
            Self::UnknownToken => f.write_str("ce token n'existe pas"),
            Self::TokenOfAnotherInstance => {
                f.write_str("instance-isolated: ce token est sur une autre instance de battlemap")
            }
        }
    }
}

/// What a badge set becomes before it is submitted: the partition it writes
/// and the row it writes there.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The token's id: the partition the change lands on.
    pub target_id: String,
    /// The fields written: `conditions`.
    pub payload: Row,
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Condition)
}

/// The badge set `wire` asks for on `target`, the instance the command
/// names — else the payload's `instance`.
///
/// # Errors
///
/// [`Refusal::NotAnId`] for an instance or a token that is no uuid,
/// [`Refusal::NoState`] for a condition that is no text besides blanks,
/// [`Refusal::Nul`] for one that holds U+0000, [`Refusal::NoSwitch`] for an
/// `on` that is no boolean.
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
        instance,
        token,
        state: state.to_owned(),
        on,
    })
}

/// The row setting the badge `request` names, `token` being that token's
/// row as the queue leaves it: its `conditions` on its own partition, with
/// the badge put on or taken off, and nothing else.
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
    let mut conditions: Vec<&str> = token
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

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// A badge set made the row it writes on the token's partition
/// ([`prepare`]), judged against that partition as the queue leaves it: one
/// naming no id, no condition or no switch is refused, 422; one naming a
/// token that is not there, 404; a token of another instance, 422 — nothing
/// queued.
struct Condition;

#[async_trait]
impl Prepare for Condition {
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
