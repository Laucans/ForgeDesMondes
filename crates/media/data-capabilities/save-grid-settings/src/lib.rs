//! DataCapability `media.saveGridSettings` (`data-capability.json`): the DM
//! adjusts a battlemap's hex grid, and every later view draws it as saved.
//!
//! A save is one command on `Media/<the battlemap's id>` carrying the grid's
//! six settings whole. What the screen sends is `HexGrid@1`'s words — `size`,
//! `ox`, `oy`, `orient`, `color`, `opacity` — and [`prepare`] makes them the
//! aggregate's fields, which the DataGuard's invariants judge and its store
//! projects into `battlemap_grid`:
//!
//! - `orient` `pointy` and `flat` are stored « pointe » and « plat », the
//!   words `battlemap_grid.orientation` holds;
//! - the opacity is a fraction of 1, as `HexGrid@1` states it;
//! - each number is kept to the hundredth, as the table keeps it;
//! - the battlemap is the command's target or, with none, the payload's
//!   `id`, lowercase as the table writes it;
//! - the feet a hex is worth are never written: one hex is always 5 feet
//!   (`hex-is-five-feet`), and a payload carrying them — or any field but
//!   the six and the `id` — is refused ([`Refusal::Unknown`]).
//!
//! A hex size of 0 or an opacity outside 0 to 1 is still submitted: the
//! invariant `grid-values-range` rejects it, naming itself, and nothing is
//! applied. So is a save on an illustration, or on no media at all:
//! `grid-only-for-battlemap` rejects it. What no invariant judges and the
//! table could never hold — no battlemap, a field missing, an orientation
//! that is neither, a value that is no number or no colour, a number past
//! the table's precision — is refused before it is queued ([`Refusal`]).

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "media.saveGridSettings";

/// The grid's settings: each word of `HexGrid@1` the payload carries, and
/// the aggregate's field it is saved under.
const SETTINGS: [(&str, &str); 6] = [
    ("size", "hexSizePx"),
    ("ox", "offsetX"),
    ("oy", "offsetY"),
    ("orient", "orientation"),
    ("color", "color"),
    ("opacity", "opacity"),
];

/// The largest size or offset `battlemap_grid` holds (`numeric(7,2)`).
const LARGEST: f64 = 99_999.99;

/// The payload's field naming the battlemap when the command names no
/// target (`target.id` of `data-capability.json`); never written.
const ID: &str = "id";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a save becomes before it is submitted.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The battlemap's id, lowercase as the table writes it: one battlemap
    /// is one partition whatever case the request used.
    pub target_id: String,
    /// The six fields written on its partition, each number to the
    /// hundredth the table keeps.
    pub payload: Row,
}

/// Why a save is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Neither the command nor its payload names the battlemap.
    NoBattlemap,
    /// The payload carries a field that is none of the grid's six settings.
    Unknown(String),
    /// One of the six settings is missing.
    Missing(&'static str),
    /// A setting no grid can hold: a size or an offset that is no number or
    /// one past the table's precision, an orientation neither pointy nor
    /// flat, a colour that is no word, an opacity that is no number.
    Invalid(&'static str),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoBattlemap => write!(f, "aucune battlemap désignée"),
            Self::Unknown(field) => write!(
                f,
                "{field} : seuls size, ox, oy, orient, color et opacity se règlent"
            ),
            Self::Missing(field) => write!(f, "{field} : réglage manquant"),
            Self::Invalid(field) => write!(f, "{field} : valeur impossible pour une grille"),
        }
    }
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// A save of `wire` on the battlemap `target` names — or, with no target,
/// the payload's `id` — as it is submitted.
///
/// # Errors
///
/// [`Refusal`] when no battlemap is named, or `wire` carries another field,
/// lacks one of the six, or holds a value no grid can.
pub fn prepare(wire: &Row, target: Option<&str>) -> Result<Prepared, Refusal> {
    if let Some(field) = wire
        .keys()
        .find(|field| *field != ID && SETTINGS.iter().all(|(word, _)| word != field))
    {
        return Err(Refusal::Unknown(field.clone()));
    }
    let target_id = target
        .or_else(|| wire.get(ID).and_then(Value::as_str))
        .filter(|id| !id.trim().is_empty())
        .ok_or(Refusal::NoBattlemap)?
        .to_ascii_lowercase();
    let mut payload = Row::new();
    for (word, field) in SETTINGS {
        let given = wire
            .get(word)
            .filter(|value| !value.is_null())
            .ok_or(Refusal::Missing(word))?;
        let value = match word {
            "orient" => orientation(given),
            "color" => given
                .as_str()
                .filter(|color| !color.trim().is_empty())
                .map(Value::from),
            // Out of 0 to 1, it is `grid-values-range`'s to reject.
            "opacity" => hundredth(given),
            // Out of what the table holds, it never reaches it; 0 and below
            // are `grid-values-range`'s to reject.
            _ => hundredth(given).filter(|n| n.as_f64().is_some_and(|n| n.abs() <= LARGEST)),
        }
        .ok_or(Refusal::Invalid(word))?;
        payload.insert(field.to_string(), value);
    }
    Ok(Prepared { target_id, payload })
}

/// `HexGrid@1`'s orientation as `battlemap_grid.orientation` stores it; the
/// stored words are taken as they are.
fn orientation(given: &Value) -> Option<Value> {
    match given.as_str()? {
        "pointy" | "pointe" => Some("pointe".into()),
        "flat" | "plat" => Some("plat".into()),
        _ => None,
    }
}

/// A number, as JSON writes it — never a text, which the table would read —
/// to the hundredth `battlemap_grid` keeps: the engine judges what the
/// table will hold, so a size of 0.004 is the 0 `grid-values-range` rejects.
/// A number already there is kept as it was given.
fn hundredth(given: &Value) -> Option<Value> {
    let n = given.as_f64()?;
    let rounded = (n * 100.0).round() / 100.0;
    Some(if rounded == n {
        given.clone()
    } else {
        rounded.into()
    })
}
