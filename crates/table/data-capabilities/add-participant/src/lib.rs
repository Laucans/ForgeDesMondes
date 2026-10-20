//! DataCapability `table.addParticipant` (`data-capability.json`):
//! reinforcements arrive mid-fight, and the DM adds them to the running
//! combat — they fall into their place of the order without the combat
//! starting again.
//!
//! The engine writes one partition per command, and a participant's place
//! in a combat is its own, `BattlemapInstance/<its entry's id>`: joining is
//! one entry inserted there, ranked per Initiative@1 among the combat's
//! entries ([`prepare`]):
//!
//! - an NPC rolls a d20 — drawn from the [`Dice`] the caller injects — and
//!   its initiative is the roll plus its DEX modifier, `floor((DEX - 10) /
//!   2)` (`npc-initiative-bounds`); a PC rolls nothing, its initiative is the
//!   figure the DM entered, and an NPC is given none;
//! - it is placed after every participant that ranks before it or ties with
//!   it — initiative descending, then the DEX score descending (none last),
//!   then the draw descending, a number drawn from the same dice and kept as
//!   its `tiebreak` (`initiative-tiebreak`) — whatever order the DM gave the
//!   others since;
//! - the ranks are whole numbers the order is read by, so the participants
//!   it lands ahead of each move down until a gap in the ranks takes it in:
//!   each such move is a command of its own on that participant's entry, a
//!   relative step of its rank, which composes with every other step queued
//!   there and needs no confirmation. An entry the engine holds no row of
//!   (one only the tables hold) is written whole, its own fields as they are;
//! - the current turn, the round and every other field stay as they are: an
//!   entry is named by its id, not its rank.
//!
//! What the engine cannot see from one row — the token's instance, the
//! instance's open combat, its entries, the character's kind and DEX — is
//! read before the command is queued ([`Seating`]); what the tables could
//! never hold, or the combat forbids, is refused before anything is
//! ([`Refusal`]).

use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde_json::{Value, json};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.addParticipant";

/// The faces of the die a participant's draw is rolled on: the number kept
/// as its `tiebreak`, which settles a tie on initiative and DEX.
pub const DRAW_SIDES: u32 = 100;

/// The aggregate whose partitions a token and an entry are.
const AGGREGATE: &str = "BattlemapInstance";

/// The kind of character that is a PC, as `entity.type` writes it.
const PC: &str = "pj";

/// The kind of character that is an NPC.
const NPC: &str = "pnj";

/// A row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// Whether `id` names this DataCapability, with or without its version.
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The partition of the token, or of the entry, `id`.
pub fn partition(id: &str) -> String {
    format!("{AGGREGATE}/{id}")
}

/// The source of the dice: injected, so a test rolls what it means to.
pub trait Dice {
    /// A whole number from 1 to `sides`, each as likely; `None` when there
    /// is no randomness to be had.
    fn roll(&mut self, sides: u32) -> Option<i64>;
}

/// What the DM asked: the token that joins, the instance it should stand on
/// when the target named one, and a PC's initiative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The token's id, lowercase.
    pub token: String,
    /// The instance the command targets, lowercase; `None` when the token is
    /// the target.
    pub instance: Option<String>,
    /// The initiative the DM entered, for a PC.
    pub initiative: Option<i64>,
}

/// The token a request names, as the engine holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Its id, lowercase.
    pub id: String,
    /// The instance it stands on, lowercase.
    pub instance: String,
    /// The character it is a token of, lowercase.
    pub character: String,
}

/// A character's sheet, as the tables hold it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Character {
    /// `pj` or `pnj`.
    pub kind: String,
    /// Its DEX score, when the sheet gives one.
    pub dex: Option<i64>,
}

/// A participant already in the combat: its entry as the queue will leave
/// it.
#[derive(Debug, Clone, PartialEq)]
pub struct Seated {
    /// The entry's id, lowercase.
    pub id: String,
    /// Its token's id.
    pub token: String,
    /// Its roll; `None` for a PC.
    pub roll: Option<i64>,
    /// Its initiative.
    pub initiative: i64,
    /// Its DEX score, when known.
    pub dex: Option<i64>,
    /// The draw that settles its ties.
    pub tiebreak: f64,
    /// Its rank.
    pub rank: i64,
    /// Whether the engine holds its entry — a row, or a command queued on
    /// it: a step of its rank composes with what it holds. One only the
    /// tables hold is written whole.
    pub held: bool,
}

/// What the tables and the queue hold of the token's instance and character.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seating {
    /// The instance's open combat — the one with no end; `None` when there
    /// is none.
    pub combat: Option<String>,
    /// Its entries.
    pub entries: Vec<Seated>,
    /// The token's character; `None` when it is no PC or NPC.
    pub character: Option<Character>,
}

/// The commands a joining is: the newcomer's entry, and each move down of a
/// participant it lands ahead of — every one on its entry's partition.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    /// The newcomer's entry id and its row, inserted whole.
    pub entry: (String, Row),
    /// Each entry moved down, by id, the last of the order first so that no
    /// two ever hold one rank between two moves: the step of its rank, or —
    /// one only the tables hold — its row whole.
    pub moves: Vec<(String, Row)>,
}

/// Why a joining is refused before it is queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The token, or the instance, is not named by a uuid.
    NotAnId(&'static str),
    /// The initiative sent is not a whole number the tables hold.
    NotAnInitiative,
    /// The engine holds no such token, or holds it on another instance.
    UnknownToken,
    /// The instance has no combat running.
    NoOpenCombat,
    /// The token is in the combat already.
    AlreadyInCombat,
    /// The token's character is no PC or NPC the tables hold.
    UnknownCharacter,
    /// A PC's initiative was not sent: it is the DM's to enter.
    PcInitiative,
    /// An initiative was sent for an NPC, which rolls its own.
    NpcRolls,
    /// The NPC's sheet gives no DEX for its modifier.
    NoDex,
    /// The dice could not be rolled.
    NoRandomness,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::NotAnInitiative => f.write_str("l'initiative est un nombre entier"),
            Self::UnknownToken => f.write_str("ce token n'est pas sur cette instance"),
            Self::NoOpenCombat => f.write_str("aucun combat n'est en cours sur cette instance"),
            Self::AlreadyInCombat => f.write_str("ce token est déjà dans le combat"),
            Self::UnknownCharacter => f.write_str("ce token n'est ni un PJ ni un PNJ"),
            Self::PcInitiative => f.write_str("l'initiative d'un PJ est celle que saisit le DM"),
            Self::NpcRolls => f.write_str("un PNJ tire son initiative, le DM n'en saisit pas"),
            Self::NoDex => f.write_str("la fiche de ce PNJ ne donne pas sa DEX"),
            Self::NoRandomness => f.write_str("le dé n'a pas pu être lancé"),
        }
    }
}

impl std::error::Error for Refusal {}

/// What `wire`, sent to `target`, asks: the token is `wire.token`, on the
/// instance `target` — or `target` itself when `wire` names no token — and a
/// PC's initiative is `wire.initiative`.
///
/// # Errors
///
/// [`Refusal::NotAnId`] when the token or the instance is not a uuid;
/// [`Refusal::NotAnInitiative`] when the initiative is no whole number a
/// `smallint` holds.
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
    let initiative = match wire.get("initiative") {
        None | Some(Value::Null) => None,
        Some(initiative) => Some(
            initiative
                .as_i64()
                .filter(|n| i16::try_from(*n).is_ok())
                .ok_or(Refusal::NotAnInitiative)?,
        ),
    };
    Ok(Request {
        // The token named as its own target stands on no instance named.
        instance: instance.filter(|instance| *instance != token),
        token,
        initiative,
    })
}

/// The token `request` names, from `base`, its row as the commands queued
/// there will leave it.
///
/// # Errors
///
/// [`Refusal::UnknownToken`] when `base` holds no such token, or holds it on
/// another instance than the one the request names.
pub fn token(request: &Request, base: &Row) -> Result<Token, Refusal> {
    let held = |field: &str| {
        base.get(field)
            .and_then(Value::as_str)
            .map(str::to_lowercase)
    };
    let (Some(instance), Some(character)) = (held("tokenInstanceId"), held("characterId")) else {
        return Err(Refusal::UnknownToken);
    };
    if held("tokenId").as_deref() != Some(request.token.as_str())
        || request
            .instance
            .as_ref()
            .is_some_and(|named| *named != instance)
    {
        return Err(Refusal::UnknownToken);
    }
    Ok(Token {
        id: request.token.clone(),
        instance,
        character,
    })
}

/// `request` made the commands that seat `token` in the combat `seating`
/// holds, as the entry `entry`: its roll and its draw from `dice`.
///
/// # Errors
///
/// [`Refusal::NoOpenCombat`], [`Refusal::AlreadyInCombat`],
/// [`Refusal::UnknownCharacter`]; for a PC, [`Refusal::PcInitiative`]; for an
/// NPC, [`Refusal::NpcRolls`] and [`Refusal::NoDex`];
/// [`Refusal::NoRandomness`] when `dice` rolls nothing.
pub fn prepare(
    request: &Request,
    token: &Token,
    seating: &Seating,
    dice: &mut impl Dice,
    entry: &str,
) -> Result<Prepared, Refusal> {
    let combat = seating.combat.as_deref().ok_or(Refusal::NoOpenCombat)?;
    if seating
        .entries
        .iter()
        .any(|seated| seated.token.eq_ignore_ascii_case(&token.id))
    {
        return Err(Refusal::AlreadyInCombat);
    }
    let character = seating
        .character
        .as_ref()
        .filter(|character| [PC, NPC].contains(&character.kind.as_str()))
        .ok_or(Refusal::UnknownCharacter)?;
    let (roll, initiative) = if character.kind == PC {
        (None, request.initiative.ok_or(Refusal::PcInitiative)?)
    } else {
        if request.initiative.is_some() {
            return Err(Refusal::NpcRolls);
        }
        let dex = character.dex.ok_or(Refusal::NoDex)?;
        let roll = dice.roll(20).ok_or(Refusal::NoRandomness)?;
        (Some(roll), roll + modifier(dex))
    };
    let draw = dice.roll(DRAW_SIDES).ok_or(Refusal::NoRandomness)?;
    let newcomer = Standing {
        initiative,
        dex: character.dex,
        draw: draw as f64,
    };
    // The order as the carousel reads it: by rank, then by id.
    let mut entries: Vec<&Seated> = seating.entries.iter().collect();
    entries.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.id.cmp(&b.id)));
    // After the last participant that ranks before it, or ties with it.
    let at = entries
        .iter()
        .rposition(|seated| order(&seated.standing(), &newcomer) != Ordering::Greater)
        .map_or(0, |before| before + 1);
    let rank = at
        .checked_sub(1)
        .and_then(|before| entries.get(before))
        .map_or(1, |before| before.rank.max(0) + 1);
    let mut last = rank;
    let mut moves = Vec::new();
    for seated in &entries[at..] {
        if seated.rank > last {
            break;
        }
        last += 1;
        moves.push((seated.id.clone(), seated.moved_to(combat, last)));
    }
    // Last entry first: each lands on a rank the one after it just left, or
    // on a free one — queued top-down, the first would land on a rank the
    // next still holds, and a failure midway would leave two at one rank.
    moves.reverse();
    let row = Row::from([
        ("initiativeEntryId".to_string(), json!(entry)),
        ("initiativeEntryCombatId".to_string(), json!(combat)),
        ("initiativeEntryTokenId".to_string(), json!(token.id)),
        ("roll".to_string(), json!(roll)),
        ("initiative".to_string(), json!(initiative)),
        ("dexScore".to_string(), json!(character.dex)),
        ("tiebreak".to_string(), json!(newcomer.draw)),
        ("rank".to_string(), json!(rank)),
    ]);
    Ok(Prepared {
        entry: (entry.to_string(), row),
        moves,
    })
}

/// What ranks a participant (Initiative@1, `order`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Standing {
    /// Its initiative.
    pub initiative: i64,
    /// Its DEX score, when known.
    pub dex: Option<i64>,
    /// Its draw.
    pub draw: f64,
}

/// Whether `a` goes before `b` (`Less`), after it (`Greater`), or ties with
/// it on all three: initiative descending, then the DEX score descending — a
/// participant with none after every known one — then the draw descending.
pub fn order(a: &Standing, b: &Standing) -> Ordering {
    b.initiative
        .cmp(&a.initiative)
        .then_with(|| match (a.dex, b.dex) {
            (Some(a), Some(b)) => b.cmp(&a),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        })
        .then_with(|| b.draw.total_cmp(&a.draw))
}

/// A DEX score's modifier: half its distance to 10, rounded down.
pub fn modifier(dex: i64) -> i64 {
    (dex - 10).div_euclid(2)
}

impl Seated {
    fn standing(&self) -> Standing {
        Standing {
            initiative: self.initiative,
            dex: self.dex,
            draw: self.tiebreak,
        }
    }

    /// The command that moves this entry of `combat` down to `rank`: the
    /// step from its rank, on the entry the engine holds; its row whole, at
    /// that rank, on one only the tables hold — where nothing is to step
    /// from.
    fn moved_to(&self, combat: &str, rank: i64) -> Row {
        let mut row = Row::from([
            ("initiativeEntryId".to_string(), json!(self.id)),
            ("initiativeEntryCombatId".to_string(), json!(combat)),
            ("initiativeEntryTokenId".to_string(), json!(self.token)),
        ]);
        if self.held {
            row.insert("rank".to_string(), json!(rank - self.rank));
        } else {
            row.extend([
                ("roll".to_string(), json!(self.roll)),
                ("initiative".to_string(), json!(self.initiative)),
                ("dexScore".to_string(), json!(self.dex)),
                ("tiebreak".to_string(), json!(self.tiebreak)),
                ("rank".to_string(), json!(rank)),
            ]);
        }
        row
    }
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
