//! DataCapability `table.moveToken` (`data-capability.json`): the DM drops a
//! token on another hex of a battlemap instance, and that hex is saved at
//! once — the one position the DM view and the player view both read
//! (`single-position-source`), found where it was left when the map is
//! reopened (`state-persisted-reset`).
//!
//! The engine writes one partition per command and judges a row alone: a
//! token is its own row, `BattlemapInstance/<its id>`, as `table.placeTokens`
//! drops it. A move names the instance — the command's target, or the
//! payload's `instance` — and the token; it lands on the token's own row and
//! writes its axial `q` and `r` (HexGrid@1) alone: never another token, never
//! the instance, never the original battlemap nor its grid
//! (`instance-isolated`).
//!
//! What a move names is judged before it is queued: the token's row, as the
//! queue leaves it, is a token, and a token of the instance named — a token
//! of another instance is refused whatever hex it is sent to. [`request`]
//! reads the DM's request, [`prepare`] turns it and the token's row into the
//! row written, or into the [`Refusal`] that says why it is not.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.moveToken";

/// The aggregate a token is a row of.
const AGGREGATE: &str = "BattlemapInstance";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a move asks for: the instance and the token by their ids, lowercase
/// and hyphenated, and the hex it is dropped on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The battlemap instance the token stands on.
    pub instance: String,
    /// The token moved.
    pub token: String,
    /// The hex's axial column.
    pub q: i64,
    /// The hex's axial row.
    pub r: i64,
}

/// Why a move is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A field that must be a uuid is not one: `instance` or `token`.
    NotAnId(&'static str),
    /// A coordinate is not a whole number the tables hold: `q` or `r`.
    NotAHex(&'static str),
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
            Self::NotAHex(field) => write!(f, "{field} n'est pas une coordonnée de case"),
            Self::UnknownToken => f.write_str("ce token n'existe pas"),
            Self::TokenOfAnotherInstance => {
                f.write_str("instance-isolated: ce token est sur une autre instance de battlemap")
            }
        }
    }
}

/// What a move becomes before it is submitted: the partition it writes and
/// the row it writes there.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The token's id: the partition the move lands on.
    pub target_id: String,
    /// The fields written: `q` and `r`.
    pub payload: Row,
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Moves)
}

/// The move `wire` asks for on `target`, the instance the command names —
/// else the payload's `instance`.
///
/// # Errors
///
/// [`Refusal::NotAnId`] for an instance or a token that is no uuid,
/// [`Refusal::NotAHex`] for a coordinate that is no whole number an
/// `integer` holds.
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
    let axis = |field: &'static str| {
        wire.get(field)
            .and_then(Value::as_i64)
            .filter(|n| i32::try_from(*n).is_ok())
            .ok_or(Refusal::NotAHex(field))
    };
    Ok(Request {
        instance,
        token,
        q: axis("q")?,
        r: axis("r")?,
    })
}

/// The row moving the token `request` names, `token` being that token's row
/// as the queue leaves it: its `q` and `r` on its own partition, and nothing
/// else.
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
    let payload = [("q", request.q), ("r", request.r)]
        .into_iter()
        .map(|(field, value)| (field.to_owned(), Value::from(value)))
        .collect();
    Ok(Prepared {
        target_id: request.token.clone(),
        payload,
    })
}

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// A move made the row it writes on the token's partition ([`prepare`]),
/// judged against that partition as the queue leaves it: one naming no id or
/// no hex is refused, 422; one naming a token that is not there, 404; a
/// token of another instance, 422 — nothing queued.
struct Moves;

#[async_trait]
impl Prepare for Moves {
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
