//! DataCapability `table.placeTokens` (`data-capability.json`): the DM drops
//! a character on a battlemap instance at a hex — a monster « × 3 » at once —
//! and gets that many tokens, each with its own number and hit points.
//!
//! The engine writes one partition per command and judges a row alone: a
//! token is its own row, `BattlemapInstance/<its id>`, and a drop of n tokens
//! is n commands, one per token, which the DataGuard's invariants judge and
//! its store projects into `token`. What the engine cannot see from one row —
//! the instance's session, the character's sheet, whether a PC is present,
//! the numbers already taken — is read before the drop is queued
//! ([`Roster`]); [`prepare`] turns the request and that roster into the token
//! rows:
//!
//! - the instance is the command's target, or the payload's `instance`; the
//!   character is a PC or an NPC of the instance's campaign, by its id;
//! - an NPC's tokens are numbered after the highest number its tokens hold on
//!   the instance (1 to n the first time), and named after the character
//!   with their number, « Bandit 2 » (`multi-drop-numbering`); each copies
//!   the HP max of the sheet as its HP max and its current HP, so a fight
//!   never changes the sheet (`pj-hp-not-tracked`);
//! - a PC is dropped once, only when present at the instance's session, and
//!   only when it has no token on the instance yet (`token-roster`); its
//!   token has no number, its name is the character's, and it tracks no HP;
//! - every token stands on the hex dropped on, visible, with no condition;
//! - what the tables could never hold, or what the roster forbids, is refused
//!   before anything is queued ([`Refusal`]).

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.placeTokens";

/// The most tokens one drop makes: the drop dialog's « × 12 ».
pub const MAX_COUNT: u16 = 12;

/// The kind of character that is a PC, as `entity.type` writes it.
const PC: &str = "pj";

/// The kind of character that is an NPC.
const NPC: &str = "pnj";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Whether `id` names this DataCapability, at any version.
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// What a drop asks for: the instance and the character by their ids,
/// lowercase, the hex, and how many tokens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The battlemap instance dropped on.
    pub instance: String,
    /// The character dropped.
    pub character: String,
    /// The hex's axial column.
    pub q: i64,
    /// The hex's axial row.
    pub r: i64,
    /// How many tokens, from 1 to [`MAX_COUNT`].
    pub count: u16,
}

/// A character's sheet, as the tables hold it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Character {
    /// Its campaign.
    pub campaign: String,
    /// `pj` or `pnj`.
    pub kind: String,
    /// Its name.
    pub name: String,
    /// Its HP max, when the sheet gives one.
    pub hp_max: Option<i64>,
}

/// What the tables and the queue hold of the instance and the character a
/// drop names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Roster {
    /// The campaign of the instance's session; `None` when there is no such
    /// instance.
    pub campaign: Option<String>,
    /// The character's sheet; `None` when it is no PC or NPC.
    pub character: Option<Character>,
    /// Whether the character is present at the instance's session.
    pub present: bool,
    /// How many tokens the character has on the instance, applied or queued.
    pub placed: usize,
    /// The numbers those tokens hold.
    pub taken: Vec<i64>,
}

/// Why a drop is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A field that must be a uuid is not one.
    NotAnId(&'static str),
    /// A coordinate is not a whole number the tables hold.
    NotAHex(&'static str),
    /// The count is not a whole number from 1 to [`MAX_COUNT`].
    Count,
    /// The tables hold no such instance.
    UnknownInstance,
    /// The tables hold no such PC or NPC.
    UnknownCharacter,
    /// The character is of another campaign than the instance's session.
    CharacterOfAnotherCampaign,
    /// A PC not present at the session (`token-roster`).
    Absent,
    /// A PC that has its token on the instance already (`token-roster`).
    AlreadyPlaced,
    /// A PC dropped more than once (`token-roster`).
    OnePcToken,
    /// The numbers a `smallint` holds are all taken.
    NoNumberLeft,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::NotAHex(field) => write!(f, "{field} n'est pas une coordonnée de case"),
            Self::Count => write!(f, "le nombre de tokens va de 1 à {MAX_COUNT}"),
            Self::UnknownInstance => f.write_str("cette instance de battlemap n'existe pas"),
            Self::UnknownCharacter => f.write_str("ce personnage n'existe pas"),
            Self::CharacterOfAnotherCampaign => {
                f.write_str("ce personnage n'est pas de la campagne de la séance")
            }
            Self::Absent => f.write_str("ce PJ n'est pas présent à la séance"),
            Self::AlreadyPlaced => f.write_str("ce PJ a déjà son token sur la carte"),
            Self::OnePcToken => f.write_str("un PJ n'a qu'un token par carte"),
            Self::NoNumberLeft => f.write_str("plus aucun numéro de token n'est libre"),
        }
    }
}

/// The drop `wire` asks for on `target`, the instance the command names —
/// else the payload's `instance`.
///
/// # Errors
///
/// [`Refusal::NotAnId`] for an instance or a character that is no uuid,
/// [`Refusal::NotAHex`] for a coordinate that is no whole number an
/// `integer` holds, [`Refusal::Count`] for a count that is no whole number
/// from 1 to [`MAX_COUNT`].
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    let instance = target
        .or_else(|| wire.get("instance").and_then(Value::as_str))
        .and_then(uuid)
        .ok_or(Refusal::NotAnId("instance"))?;
    let character = wire
        .get("character")
        .and_then(Value::as_str)
        .and_then(uuid)
        .ok_or(Refusal::NotAnId("character"))?;
    let axis = |field: &'static str| {
        wire.get(field)
            .and_then(Value::as_i64)
            .filter(|n| i32::try_from(*n).is_ok())
            .ok_or(Refusal::NotAHex(field))
    };
    let (q, r) = (axis("q")?, axis("r")?);
    let count = match wire.get("count") {
        None | Some(Value::Null) => 1,
        Some(count) => count
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .filter(|n| (1..=MAX_COUNT).contains(n))
            .ok_or(Refusal::Count)?,
    };
    Ok(Request {
        instance,
        character,
        q,
        r,
        count,
    })
}

/// The token rows `request` drops, against what `roster` holds: one per id
/// of `ids`, which holds one fresh id per token asked for.
///
/// # Errors
///
/// [`Refusal::UnknownInstance`], [`Refusal::UnknownCharacter`],
/// [`Refusal::CharacterOfAnotherCampaign`]; for a PC, [`Refusal::OnePcToken`],
/// [`Refusal::Absent`] and [`Refusal::AlreadyPlaced`] (`token-roster`); for
/// an NPC, [`Refusal::NoNumberLeft`].
pub fn prepare(request: &Request, roster: &Roster, ids: &[String]) -> Result<Vec<Row>, Refusal> {
    let campaign = roster.campaign.as_deref().ok_or(Refusal::UnknownInstance)?;
    let character = roster
        .character
        .as_ref()
        .filter(|character| [PC, NPC].contains(&character.kind.as_str()))
        .ok_or(Refusal::UnknownCharacter)?;
    if !character.campaign.eq_ignore_ascii_case(campaign) {
        return Err(Refusal::CharacterOfAnotherCampaign);
    }
    let count = usize::from(request.count);
    let token = |id: &str, number: Value, name: String, hp: Value| {
        Row::from([
            ("tokenId".to_string(), json!(id)),
            ("tokenInstanceId".to_string(), json!(request.instance)),
            ("characterId".to_string(), json!(request.character)),
            ("number".to_string(), number),
            ("name".to_string(), json!(name)),
            ("q".to_string(), json!(request.q)),
            ("r".to_string(), json!(request.r)),
            ("visible".to_string(), json!(true)),
            ("hpCurrent".to_string(), hp.clone()),
            ("hpMax".to_string(), hp),
            ("conditions".to_string(), json!([])),
        ])
    };
    let name = character.name.trim();
    if character.kind == PC {
        if count != 1 {
            return Err(Refusal::OnePcToken);
        }
        if !roster.present {
            return Err(Refusal::Absent);
        }
        if roster.placed > 0 {
            return Err(Refusal::AlreadyPlaced);
        }
        return Ok(ids
            .iter()
            .take(1)
            .map(|id| token(id, Value::Null, name.to_string(), Value::Null))
            .collect());
    }
    let first = roster.taken.iter().copied().max().unwrap_or(0).max(0) + 1;
    let last = first + i64::from(request.count) - 1;
    if i16::try_from(last).is_err() {
        return Err(Refusal::NoNumberLeft);
    }
    // An HP max the tables could not hold, or below 0, is none to copy.
    let hp = character
        .hp_max
        .filter(|hp| *hp >= 0 && i16::try_from(*hp).is_ok())
        .map_or(Value::Null, |hp| json!(hp));
    Ok(ids
        .iter()
        .take(count)
        .zip(first..)
        .map(|(id, number)| token(id, json!(number), format!("{name} {number}"), hp.clone()))
        .collect())
}

/// `text` as a uuid, lowercase as the tables write it; `None` when it is no
/// uuid.
fn uuid(text: &str) -> Option<String> {
    let hyphens = [8, 13, 18, 23];
    let shaped = text.len() == 36
        && text.char_indices().all(|(i, c)| {
            if hyphens.contains(&i) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        });
    shaped.then(|| text.to_ascii_lowercase())
}
