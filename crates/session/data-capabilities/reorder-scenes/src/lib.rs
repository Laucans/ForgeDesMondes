//! DataCapability `session.reorderScenes` (`data-capability.json`): the DM
//! drags a session's scenes into the order they will play them, and the new
//! order is saved in one go. One command on `Session/<its session id>`
//! writes the whole order, so every rank is rewritten at once
//! (`scene-order-contiguous`). [`prepare`] turns the wire's payload —
//! `{session, order: [sceneId]}` — into that command:
//!
//! - the session is the command's target, lowercase as the tables write a
//!   uuid;
//! - the order is written under [`ORDER`], the scenes' ids lowercase and
//!   kept as given: the first ranked 1, the last n. A scene the order omits,
//!   repeats or takes from another session is the DataGuard's to reject,
//!   which judges the order against the session's scenes when it is
//!   applied;
//! - a payload naming no session, or no order of scene ids, is refused
//!   before anything is queued ([`Refusal`]).

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "session.reorderScenes";

/// The field of the session's row the order is written under: the scenes'
/// ids, each at its rank less one.
pub const ORDER: &str = "scenePosition";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Why a reorder is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload names no session, or only blanks.
    NoSession,
    /// The payload's `order` is not a list of scene ids, or is empty.
    NoOrder,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoSession => "un réordonnancement nomme sa séance",
            Self::NoOrder => "un réordonnancement donne la liste des scènes de la séance",
        })
    }
}

/// What a reorder becomes before it is submitted: what the command writes
/// on `Session/<target_id>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The order, under [`ORDER`].
    pub payload: Row,
    /// The session's id, lowercase: the command's target.
    pub target_id: String,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// An id as the wire gives it: trimmed, lowercase; `None` when it is not a
/// text, is blank, or holds a character no table holds (U+0000).
fn id(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|id| !id.is_empty() && !id.contains('\0'))
        .map(str::to_ascii_lowercase)
}

/// The reorder the wire's payload asks for.
///
/// # Errors
///
/// [`Refusal`] when the payload names no session, or its `order` is not a
/// non-empty list of ids.
pub fn prepare(wire: &Row) -> Result<Prepared, Refusal> {
    let session = wire.get("session").and_then(id).ok_or(Refusal::NoSession)?;
    let order = wire
        .get("order")
        .and_then(Value::as_array)
        .filter(|order| !order.is_empty())
        .ok_or(Refusal::NoOrder)?
        .iter()
        .map(|scene| id(scene).map(Value::String))
        .collect::<Option<Vec<_>>>()
        .ok_or(Refusal::NoOrder)?;
    Ok(Prepared {
        payload: Row::from([(ORDER.into(), Value::Array(order))]),
        target_id: session,
    })
}
