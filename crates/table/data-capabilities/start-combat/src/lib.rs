//! DataCapability `table.startCombat` (`data-capability.json`): the DM starts
//! the fight on a battlemap instance and the order is ready at once —
//! monsters rolled, PCs entered, ties settled the D&D way (Initiative@1).
//!
//! The engine writes one partition per command and judges a row alone: the
//! combat is its own row, `BattlemapInstance/<its id>`, and so is each
//! participant's place in it, `BattlemapInstance/<its entry id>`. A start is
//! one command for the combat, then one per participant, which the
//! DataGuard's invariants judge and its store projects into `combat` and
//! `initiative_entry`. What the engine cannot see from one row — the
//! instance's tokens, each character's kind and DEX, whether a combat of the
//! instance is still open — is read before the start is queued ([`Lineup`]);
//! [`prepare`] turns the request and that lineup into the rows:
//!
//! - the instance is the command's target, or the payload's `instance`;
//! - every token of the instance joins, a hidden one like any other
//!   (`hidden-keeps-rank`);
//! - an NPC's roll is the one the DM sent for its token, or a d20 the
//!   injected [`Dice`] throw; its initiative is the roll plus its DEX
//!   modifier, and its DEX the sheet's (`npc-initiative-bounds`);
//! - a PC rolls nothing: its initiative is the figure the DM entered for its
//!   character, and its DEX the sheet's, which the ties read;
//! - ranks follow initiative, then DEX, then a draw the dice make, kept
//!   from then on (`initiative-tiebreak`, [`ranked`]);
//! - the combat starts at round 1 on the participant ranked 1 (`next-turn`),
//!   and an instance whose combat is still open starts none
//!   (`one-active-combat`);
//! - what the tables could never hold, or what the lineup forbids, is
//!   refused before anything is queued ([`Refusal`]).

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.startCombat";

/// The DEX score an NPC whose sheet gives none rolls with: an average one,
/// no modifier.
pub const DEFAULT_DEX: i64 = 10;

/// The kind of character that is a PC, as `entity.type` writes it.
const PC: &str = "pj";

/// The kind of character that is an NPC.
const NPC: &str = "pnj";

/// A d20.
const D20: std::ops::RangeInclusive<i64> = 1..=20;

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Whether `id` names this DataCapability, at any version.
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The partition of the aggregate a combat or an entry of id `id` is
/// written on.
pub fn partition(id: &str) -> String {
    format!("BattlemapInstance/{id}")
}

/// What a start asks for: the instance by its id, lowercase, the PCs'
/// initiatives by character and the NPCs' rolls by token, ids lowercase.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    /// The battlemap instance whose combat starts.
    pub instance: String,
    /// The initiative the DM entered, by PC character.
    pub initiative: BTreeMap<String, i64>,
    /// The d20 rolled at the table, by NPC token.
    pub rolls: BTreeMap<String, i64>,
}

/// A token of the instance, as the tables hold it and its character's sheet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Participant {
    /// The token's id.
    pub token: String,
    /// Its character's id.
    pub character: String,
    /// `pj` or `pnj`; `None` when its character is neither any more.
    pub kind: Option<String>,
    /// The sheet's DEX score, when it gives one.
    pub dex: Option<i64>,
}

/// What the tables and the queue hold of the instance a start names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lineup {
    /// Whether the tables hold the instance.
    pub instance: bool,
    /// Its tokens, applied or queued, in the order the ties keep.
    pub tokens: Vec<Participant>,
    /// Whether a combat of the instance has no end yet, applied or queued.
    pub open: bool,
}

/// The randomness a start draws from, injected: the server's throws dice,
/// a test's are scripted.
pub trait Dice {
    /// A d20: a whole number from 1 to 20.
    fn d20(&mut self) -> i64;
    /// The draw a participant's ties are settled by, kept from then on.
    fn draw(&mut self) -> i64;
}

/// The ids and the time a start is given.
#[derive(Clone, Copy, Debug)]
pub struct Fresh<'a> {
    /// The combat's id.
    pub combat: &'a str,
    /// One id per entry, as many as the lineup has tokens: the first goes
    /// to rank 1.
    pub entries: &'a [String],
    /// When it starts, RFC 3339.
    pub now: &'a str,
}

/// The rows a start writes, each with the id its partition is named by: the
/// combat, then its entries in rank order.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The combat's id and row.
    pub combat: (String, Row),
    /// Each entry's id and row, rank 1 first.
    pub entries: Vec<(String, Row)>,
}

/// Why a start is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A field, or a key, that must be a uuid is not one.
    NotAnId(&'static str),
    /// The initiatives are not an object of whole numbers a `smallint`
    /// holds.
    NotAnInitiative,
    /// The rolls are not an object of whole numbers from 1 to 20.
    NotARoll,
    /// The tables hold no such instance.
    UnknownInstance,
    /// A combat of the instance has not ended (`one-active-combat`).
    CombatOpen,
    /// The instance has no token to fight.
    NoParticipant,
    /// A token's character is no PC nor NPC any more.
    UnknownCharacter,
    /// A PC with a token on the instance has no initiative entered: the id
    /// of its character.
    MissingInitiative(String),
    /// A roll names no NPC token of the instance: a PC rolls nothing.
    RollOfNoNpc,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::NotAnInitiative => {
                f.write_str("l'initiative d'un PJ est un nombre entier, par personnage")
            }
            Self::NotARoll => f.write_str("un jet d'initiative va de 1 à 20, par token"),
            Self::UnknownInstance => f.write_str("cette instance de battlemap n'existe pas"),
            Self::CombatOpen => f.write_str("un combat est déjà en cours sur cette carte"),
            Self::NoParticipant => f.write_str("aucun token sur la carte : pas de combat"),
            Self::UnknownCharacter => f.write_str("un token n'est plus ni un PJ ni un PNJ"),
            Self::MissingInitiative(character) => {
                write!(f, "l'initiative du PJ {character} n'est pas saisie")
            }
            Self::RollOfNoNpc => f.write_str("un jet ne nomme aucun token de PNJ de la carte"),
        }
    }
}

/// The start `wire` asks for on `target`, the instance the command names —
/// else the payload's `instance`.
///
/// # Errors
///
/// [`Refusal::NotAnId`] for an instance, a character or a token that is no
/// uuid, [`Refusal::NotAnInitiative`] for initiatives that are not whole
/// numbers a `smallint` holds, [`Refusal::NotARoll`] for rolls that are not
/// whole numbers from 1 to 20.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    let instance = target
        .or_else(|| wire.get("instance").and_then(Value::as_str))
        .and_then(uuid)
        .ok_or(Refusal::NotAnId("instance"))?;
    let initiative = figures(
        wire.get("initiative"),
        "initiative",
        |n| i16::try_from(n).is_ok(),
        &Refusal::NotAnInitiative,
    )?;
    let rolls = figures(
        wire.get("rolls"),
        "rolls",
        |n| D20.contains(&n),
        &Refusal::NotARoll,
    )?;
    Ok(Request {
        instance,
        initiative,
        rolls,
    })
}

/// `value`, an object of whole numbers by uuid, keyed lowercase: empty when
/// absent.
fn figures(
    value: Option<&Value>,
    field: &'static str,
    holds: fn(i64) -> bool,
    refusal: &Refusal,
) -> Result<BTreeMap<String, i64>, Refusal> {
    match value {
        None | Some(Value::Null) => Ok(BTreeMap::new()),
        Some(Value::Object(figures)) => figures
            .iter()
            .map(|(id, figure)| {
                let id = uuid(id).ok_or(Refusal::NotAnId(field))?;
                let figure = figure
                    .as_i64()
                    .filter(|n| holds(*n))
                    .ok_or_else(|| refusal.clone())?;
                Ok((id, figure))
            })
            .collect(),
        Some(_) => Err(refusal.clone()),
    }
}

/// The rows `request` starts, against what `lineup` holds, with the ids and
/// the time of `fresh` and the randomness of `dice`: per token, in the
/// lineup's order, an NPC with no roll sent throws a d20, then every
/// participant draws.
///
/// # Errors
///
/// [`Refusal::UnknownInstance`], [`Refusal::CombatOpen`]
/// (`one-active-combat`), [`Refusal::NoParticipant`],
/// [`Refusal::RollOfNoNpc`], [`Refusal::UnknownCharacter`] and
/// [`Refusal::MissingInitiative`].
pub fn prepare(
    request: &Request,
    lineup: &Lineup,
    fresh: Fresh<'_>,
    dice: &mut impl Dice,
) -> Result<Prepared, Refusal> {
    if !lineup.instance {
        return Err(Refusal::UnknownInstance);
    }
    if lineup.open {
        return Err(Refusal::CombatOpen);
    }
    if lineup.tokens.is_empty() {
        return Err(Refusal::NoParticipant);
    }
    let npc = |token: &str| {
        lineup
            .tokens
            .iter()
            .any(|p| p.token == token && p.kind.as_deref() == Some(NPC))
    };
    if !request.rolls.keys().all(|token| npc(token)) {
        return Err(Refusal::RollOfNoNpc);
    }
    let mut seated = Vec::with_capacity(lineup.tokens.len());
    for participant in &lineup.tokens {
        let (roll, initiative, dex) = match participant.kind.as_deref() {
            Some(PC) => {
                let entered = request
                    .initiative
                    .get(&participant.character)
                    .copied()
                    .ok_or_else(|| Refusal::MissingInitiative(participant.character.clone()))?;
                (None, entered, participant.dex)
            }
            Some(NPC) => {
                let dex = participant.dex.unwrap_or(DEFAULT_DEX);
                let roll = match request.rolls.get(&participant.token) {
                    Some(roll) => *roll,
                    None => dice.d20(),
                };
                (Some(roll), initiative(roll, dex), Some(dex))
            }
            _ => return Err(Refusal::UnknownCharacter),
        };
        let standing = Standing {
            initiative,
            dex,
            draw: dice.draw(),
        };
        seated.push((participant, roll, standing));
    }
    let standings: Vec<Standing> = seated.iter().map(|(.., standing)| *standing).collect();
    let entries: Vec<(String, Row)> = ranked(&standings)
        .into_iter()
        .zip(fresh.entries)
        .zip(1_i64..)
        .map(|((at, id), rank)| {
            let (participant, roll, standing) = &seated[at];
            let row = Row::from([
                ("initiativeEntryId".to_string(), json!(id)),
                ("initiativeEntryCombatId".to_string(), json!(fresh.combat)),
                (
                    "initiativeEntryTokenId".to_string(),
                    json!(participant.token),
                ),
                ("roll".to_string(), json!(roll)),
                ("initiative".to_string(), json!(standing.initiative)),
                ("dexScore".to_string(), json!(standing.dex)),
                ("tiebreak".to_string(), json!(standing.draw)),
                ("rank".to_string(), json!(rank)),
            ]);
            (id.clone(), row)
        })
        .collect();
    let current = entries.first().map(|(id, _)| id.clone());
    let combat = Row::from([
        ("combatId".to_string(), json!(fresh.combat)),
        ("combatInstanceId".to_string(), json!(request.instance)),
        ("round".to_string(), json!(1)),
        ("currentEntryId".to_string(), json!(current)),
        ("startedAt".to_string(), json!(fresh.now)),
    ]);
    Ok(Prepared {
        combat: (fresh.combat.to_string(), combat),
        entries,
    })
}

/// What a participant's rank reads (Initiative@1, `order`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Standing {
    /// Its initiative.
    pub initiative: i64,
    /// Its DEX score, when known.
    pub dex: Option<i64>,
    /// The draw its ties are settled by.
    pub draw: i64,
}

/// The positions of `standings` in rank order (Initiative@1, `order`):
/// initiative descending, then DEX score descending — an unknown one after
/// every known one —, then draw descending, then the order given.
pub fn ranked(standings: &[Standing]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..standings.len()).collect();
    // A stable sort: a tie on all three keeps the order given.
    order.sort_by(|&a, &b| {
        let (a, b) = (&standings[a], &standings[b]);
        b.initiative
            .cmp(&a.initiative)
            .then_with(|| b.dex.cmp(&a.dex))
            .then_with(|| b.draw.cmp(&a.draw))
    });
    order
}

/// An NPC's initiative (Initiative@1, `initiative`): its d20 plus the
/// modifier of its DEX score.
pub fn initiative(roll: i64, dex: i64) -> i64 {
    roll + modifier(dex)
}

/// A score's modifier: half its distance to 10, rounded down
/// (AbilityModifier@1).
pub fn modifier(score: i64) -> i64 {
    (score - 10).div_euclid(2)
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
