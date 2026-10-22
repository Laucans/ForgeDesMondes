//! DataCapability `table.addParticipant` (`data-capability.json`):
//! reinforcements arrive mid-fight, and the DM adds them to the running
//! combat — they fall into their place of the order without the combat
//! starting again (Initiative@1).
//!
//! The engine writes one partition per command and judges a row alone: a
//! participant's place in a combat is its own row, `BattlemapInstance/<its
//! entry's id>`, as `table.startCombat` seats it, its id derived from its
//! combat and its token ([`entry_id`]) — so a token seated, applied or still
//! queued, is found on its partition, and a token seated twice lands on one.
//! Joining is the command that inserts the newcomer's entry, then its
//! follow-ups ([`plugins::Prepared::then`]), one per participant it lands
//! ahead of, each moving that entry one rank down ([`follow_ups`]). The
//! command is relative: a move is a step the engine adds to the rank as the
//! commands ahead leave it, so two joinings sent one after the other both
//! count and neither asks for a confirmation.
//!
//! What one row cannot tell — the instance's open combat, its participants,
//! the token's character, its kind and DEX — is read before the command is
//! queued ([`COMBAT`], [`PARTICIPANTS`], [`SHEET`]), each row as the queue
//! leaves it; [`prepare`] turns the request and that [`Seating`] into the
//! rows:
//!
//! - the token is the payload's `token`, on the instance the command
//!   targets — or the payload's `instance`; a token the engine does not hold
//!   on that instance is not found;
//! - an NPC rolls a d20 the injected [`Dice`] throw, and its initiative is
//!   the roll plus its DEX modifier, its sheet's DEX or [`DEFAULT_DEX`]; a PC
//!   rolls nothing and its initiative is the figure the DM entered, which an
//!   NPC is never given (`npc-initiative-bounds`);
//! - it is placed after every participant that ranks before it or ties with
//!   it, initiative descending, then DEX descending — none last —, then the
//!   draw descending, a number the dice draw and kept as its `tiebreak`
//!   (`initiative-tiebreak`), whatever order the DM gave the others since
//!   ([`placed`]);
//! - the participants from its rank on move down one rank each, until a gap
//!   in the ranks takes them in; the turn, the round and every other field
//!   stay as they are: an entry is named by its id, not its rank;
//! - a token already in the combat, an instance with no open combat, or
//!   what the tables could never hold is refused before anything is queued
//!   ([`Refusal`]).
//!
//! A move is taken only while it makes room: another participant of its
//! combat holds its rank in the queue, and the newcomer it names is seated
//! in that combat at or above it ([`shiftable`]); and only once, carrying the
//! seal its joining issued for it ([`Seals`]) — one sent alone, off a
//! joining, is refused. A joining is not atomic with its moves: one refused
//! — a participant removed in between — stops none of the others, and the
//! order stays readable, two participants at one rank ordered by id.
//!
//! The command is relative, so an entry inserted twice would add its numbers
//! onto themselves: a token is claimed while its joining is in flight
//! ([`Claims`]), and a second joining of it prepared before the first is
//! queued is refused.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use harness_dataguard::api::Submission;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use harness_dataguard::reads::Rows;
use serde_json::{Value, json};
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.addParticipant";

/// The DEX score an NPC whose sheet gives none rolls with: an average one,
/// no modifier — `table.startCombat`'s, so one combat follows one rule.
pub const DEFAULT_DEX: i64 = 10;

/// The aggregate a token, a combat and an entry are rows of.
const AGGREGATE: &str = "BattlemapInstance";

/// How long a joining claims its token while it is not yet queued, in
/// milliseconds: the time between its `Prepare` and its entering the queue.
pub const CLAIM_MILLIS: u64 = 10_000;

/// The kind of character that is a PC, as `entity.type` writes it.
const PC: &str = "pj";

/// The kind of character that is an NPC.
const NPC: &str = "pnj";

/// The namespace every entry's id is derived in — the one
/// `table.startCombat` seats its participants in, so a token's entry in a
/// combat has one id whoever seats it. It never changes.
const NAMESPACE: Uuid = Uuid::from_u128(0x5a1c_e0b4_27d9_4f63_9b1e_c4f0_83a6_2d17);

/// The open combat of the instance `$1` among the engine's rows: the latest
/// started with no end. What the engine tombstoned is not there, nor a
/// combat queued and not yet applied: a joining follows a start.
pub const COMBAT: &str = "SELECT lower(r.fields ->> 'combatId') AS combat
    FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'combatId')
        AND lower(r.fields ->> 'combatInstanceId') = $1::text
        AND r.fields ->> 'endedAt' IS NULL
    ORDER BY r.fields ->> 'startedAt' DESC NULLS LAST, r.partition
    LIMIT 1";

/// The entries the engine holds of the combat `$1` whose token it holds,
/// and the tokens it holds on the instance `$2`, by id: each token's entry
/// in the combat, queued and not yet applied, is found on the partition its
/// id names. What the engine tombstoned is not there — nor the entry of a
/// token taken off the map, which the tables' cascade removed and the
/// carousel no longer shows.
pub const PARTICIPANTS: &str = "SELECT
        coalesce((SELECT jsonb_agg(lower(e.fields ->> 'initiativeEntryId'))
            FROM dataguard_rows e
            WHERE e.partition LIKE 'BattlemapInstance/%' AND NOT e.tombstoned
                AND e.partition = 'BattlemapInstance/' || lower(e.fields ->> 'initiativeEntryId')
                AND lower(e.fields ->> 'initiativeEntryCombatId') = $1::text
                AND EXISTS (SELECT 1 FROM dataguard_rows t
                    WHERE t.partition = 'BattlemapInstance/' || lower(e.fields ->> 'initiativeEntryTokenId')
                        AND NOT t.tombstoned)),
            '[]') AS entries,
        coalesce((SELECT jsonb_agg(lower(t.fields ->> 'tokenId'))
            FROM dataguard_rows t
            WHERE t.partition LIKE 'BattlemapInstance/%' AND NOT t.tombstoned
                AND t.partition = 'BattlemapInstance/' || lower(t.fields ->> 'tokenId')
                AND lower(t.fields ->> 'tokenInstanceId') = $2::text),
            '[]') AS tokens";

/// The character `$1`'s kind and DEX score, when it is a PC or an NPC with
/// its sheet that the engine has not tombstoned.
pub const SHEET: &str = "SELECT e.type AS kind, c.dexterity AS dex
    FROM entity e JOIN character c ON c.entity_id = e.id
    WHERE e.id::text = $1::text AND e.type IN ('pj', 'pnj')
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Entity/' || e.id::text AND r.tombstoned)";

/// One partition's row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// The partition of the aggregate a token, a combat or an entry of id `id`
/// is written on.
#[must_use]
pub fn partition(id: &str) -> String {
    format!("{AGGREGATE}/{id}")
}

/// The id of the entry of `token` in `combat`, lowercase and hyphenated.
#[must_use]
pub fn entry_id(combat: &str, token: &str) -> String {
    Uuid::new_v5(&NAMESPACE, format!("entry:{combat}:{token}").as_bytes()).to_string()
}

/// What a joining asks for: the token, on the instance, and a PC's
/// initiative, ids lowercase and hyphenated.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    /// The instance whose combat the token joins.
    pub instance: String,
    /// The token that joins.
    pub token: String,
    /// The initiative the DM entered, for a PC.
    pub initiative: Option<i64>,
}

/// A character's sheet, as the tables hold it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Character {
    /// `pj` or `pnj`.
    pub kind: String,
    /// Its DEX score, when the sheet gives one.
    pub dex: Option<i64>,
}

/// A participant already in the combat: its entry as the queue leaves it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seated {
    /// The entry's id.
    pub id: String,
    /// Its token's id.
    pub token: String,
    /// Its initiative.
    pub initiative: i64,
    /// Its DEX score, when known.
    pub dex: Option<i64>,
    /// The draw its ties are settled by.
    pub tiebreak: f64,
    /// Its rank.
    pub rank: i64,
}

impl Seated {
    /// The participant `row` holds, an entry as the queue leaves it; `None`
    /// when it is no entry the tables could hold.
    #[must_use]
    pub fn of(row: &Row) -> Option<Self> {
        let id = |field: &str| row.get(field).and_then(Value::as_str).and_then(uuid);
        Some(Self {
            id: id("initiativeEntryId")?,
            token: id("initiativeEntryTokenId")?,
            initiative: row.get("initiative").and_then(Value::as_i64)?,
            dex: row.get("dexScore").and_then(Value::as_i64),
            tiebreak: row.get("tiebreak").and_then(Value::as_f64)?,
            rank: row.get("rank").and_then(Value::as_i64)?,
        })
    }

    fn standing(&self) -> Standing {
        Standing {
            initiative: self.initiative,
            dex: self.dex,
            draw: self.tiebreak,
        }
    }
}

/// What the tables and the queue hold of the token's instance and character.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seating {
    /// The instance's open combat; `None` when it has none.
    pub combat: Option<String>,
    /// Its participants.
    pub entries: Vec<Seated>,
    /// The token's character; `None` when it is no PC nor NPC with a sheet.
    pub character: Option<Character>,
}

/// The randomness a joining draws from, injected: the server's throws dice
/// ([`SystemDice`]), a test's are scripted.
pub trait Dice: Send {
    /// A d20: a whole number from 1 to 20.
    fn d20(&mut self) -> i64;
    /// The draw a participant's ties are settled by, kept from then on.
    fn draw(&mut self) -> i64;
}

/// The server's dice: each throw drawn from a fresh random id.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemDice;

impl SystemDice {
    /// 32 random bits.
    fn bits() -> u32 {
        // The low 32 bits of a v4 id are all random: its version and
        // variant sit in the high half.
        Uuid::new_v4().as_u128() as u32
    }
}

impl Dice for SystemDice {
    fn d20(&mut self) -> i64 {
        i64::from(Self::bits() % 20) + 1
    }

    fn draw(&mut self) -> i64 {
        i64::from(Self::bits())
    }
}

/// What a joining writes: the newcomer's entry, inserted whole, and the
/// entries it lands ahead of, each moved one rank down.
#[derive(Clone, Debug, PartialEq)]
pub struct Joined {
    /// The newcomer's entry.
    pub entry: Row,
    /// The ids of the entries moved down, in rank order.
    pub moves: Vec<String>,
}

/// Why a joining, or a move, is refused before it is queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The token, or the instance, is not named by a uuid.
    NotAnId(&'static str),
    /// The initiative sent is not a whole number a `smallint` holds.
    NotAnInitiative,
    /// The engine holds no such token on the instance.
    UnknownToken,
    /// The instance has no open combat.
    NoOpenCombat,
    /// The token is in the combat already.
    AlreadyInCombat,
    /// The token's character is no PC nor NPC with a sheet.
    UnknownCharacter,
    /// A PC's initiative was not sent: it is the DM's to enter.
    PcInitiative,
    /// An initiative was sent for an NPC, which rolls its own.
    NpcRolls,
    /// A move that makes no room for a newcomer of its combat.
    NotAShift,
    /// A move no joining issued: no seal, another's, or one already taken.
    Unsealed,
    /// The token's joining is in flight: prepared, not yet queued.
    Joining,
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
            Self::NotAnInitiative => f.write_str("l'initiative est un nombre entier"),
            Self::UnknownToken => f.write_str("ce token n'est pas sur cette instance"),
            Self::NoOpenCombat => f.write_str("aucun combat n'est en cours sur cette instance"),
            Self::AlreadyInCombat => f.write_str("ce token est déjà dans le combat"),
            Self::UnknownCharacter => f.write_str("ce token n'est ni un PJ ni un PNJ"),
            Self::PcInitiative => f.write_str(
                "npc-initiative-bounds: l'initiative d'un PJ est celle que saisit le DM",
            ),
            Self::NpcRolls => f.write_str(
                "npc-initiative-bounds: un PNJ tire son initiative, le DM n'en saisit pas",
            ),
            Self::NotAShift => f.write_str(
                "initiative-tiebreak: ce décalage ne fait de place à aucun nouveau participant",
            ),
            Self::Unsealed => f.write_str(
                "initiative-tiebreak: ce décalage ne suit l'arrivée d'aucun participant",
            ),
            Self::Joining => f.write_str("ce token est en train de rejoindre le combat"),
        }
    }
}

/// What the DataCapability plugs into the engine, its `Prepare` throwing the
/// server's dice.
#[must_use]
pub fn plugin() -> Plugin {
    plugin_with(SystemDice)
}

/// What the DataCapability plugs into the engine, its `Prepare` throwing
/// `dice`.
#[must_use]
pub fn plugin_with(dice: impl Dice + 'static) -> Plugin {
    Plugin::new().prepare(
        DATA_CAPABILITY,
        Joins {
            dice: Mutex::new(Box::new(dice)),
            seals: Seals::default(),
            claims: Claims::default(),
        },
    )
}

/// The joining `wire` asks for on `target`, the instance the command names
/// — else the payload's `instance`.
///
/// # Errors
///
/// [`Refusal::NotAnId`] for an instance or a token that is no uuid,
/// [`Refusal::NotAnInitiative`] for an initiative that is no whole number a
/// `smallint` holds.
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
        instance,
        token,
        initiative,
    })
}

/// The character of the token `request` names, from `token`, its row as the
/// queue leaves it.
///
/// # Errors
///
/// [`Refusal::UnknownToken`] when `token` is no token of the instance the
/// request names, [`Refusal::UnknownCharacter`] when it names no character.
pub fn character_of(request: &Request, token: &Row) -> Result<String, Refusal> {
    let held = |field: &str| token.get(field).and_then(Value::as_str).and_then(uuid);
    if held("tokenId").as_deref() != Some(request.token.as_str())
        || held("tokenInstanceId").as_deref() != Some(request.instance.as_str())
    {
        return Err(Refusal::UnknownToken);
    }
    held("characterId").ok_or(Refusal::UnknownCharacter)
}

/// The rows `request` writes against what `seating` holds, with the
/// randomness of `dice`: an NPC throws a d20, then the newcomer draws.
///
/// # Errors
///
/// [`Refusal::NoOpenCombat`], [`Refusal::AlreadyInCombat`],
/// [`Refusal::UnknownCharacter`]; for a PC, [`Refusal::PcInitiative`]; for an
/// NPC, [`Refusal::NpcRolls`].
pub fn prepare(
    request: &Request,
    seating: &Seating,
    dice: &mut dyn Dice,
) -> Result<Joined, Refusal> {
    let combat = seating.combat.as_deref().ok_or(Refusal::NoOpenCombat)?;
    if seating
        .entries
        .iter()
        .any(|seated| seated.token == request.token)
    {
        return Err(Refusal::AlreadyInCombat);
    }
    let character = seating
        .character
        .as_ref()
        .filter(|character| [PC, NPC].contains(&character.kind.as_str()))
        .ok_or(Refusal::UnknownCharacter)?;
    let (roll, initiative, dex) = if character.kind == PC {
        let entered = request.initiative.ok_or(Refusal::PcInitiative)?;
        (None, entered, character.dex)
    } else {
        if request.initiative.is_some() {
            return Err(Refusal::NpcRolls);
        }
        let dex = character.dex.unwrap_or(DEFAULT_DEX);
        let roll = dice.d20();
        (Some(roll), self::initiative(roll, dex), Some(dex))
    };
    let draw = dice.draw();
    let (rank, moves) = placed(
        &seating.entries,
        &Standing {
            initiative,
            dex,
            draw: draw as f64,
        },
    );
    let entry = Row::from([
        (
            "initiativeEntryId".to_owned(),
            json!(entry_id(combat, &request.token)),
        ),
        ("initiativeEntryCombatId".to_owned(), json!(combat)),
        ("initiativeEntryTokenId".to_owned(), json!(request.token)),
        ("roll".to_owned(), json!(roll)),
        ("initiative".to_owned(), json!(initiative)),
        ("dexScore".to_owned(), json!(dex)),
        ("tiebreak".to_owned(), json!(draw)),
        ("rank".to_owned(), json!(rank)),
    ]);
    Ok(Joined { entry, moves })
}

/// What a participant's rank reads (Initiative@1, `order`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Standing {
    /// Its initiative.
    pub initiative: i64,
    /// Its DEX score, when known.
    pub dex: Option<i64>,
    /// The draw its ties are settled by.
    pub draw: f64,
}

/// Whether `a` goes before `b` (`Less`), after it (`Greater`), or ties with
/// it on all three (Initiative@1, `order`): initiative descending, then DEX
/// score descending — an unknown one after every known one —, then draw
/// descending.
#[must_use]
pub fn order(a: &Standing, b: &Standing) -> Ordering {
    b.initiative
        .cmp(&a.initiative)
        .then_with(|| b.dex.cmp(&a.dex))
        .then_with(|| b.draw.total_cmp(&a.draw))
}

/// The rank a newcomer of standing `newcomer` takes among `entries`, and the
/// ids of the entries it lands ahead of that move one rank down, in rank
/// order. The order is the carousel's — by rank, then by id —, whatever the
/// DM made of it: the newcomer goes after the last participant that ranks
/// before it or ties with it, first when there is none; the ones from its
/// rank on move down while their ranks run on, a gap taking them in.
#[must_use]
pub fn placed(entries: &[Seated], newcomer: &Standing) -> (i64, Vec<String>) {
    let mut sorted: Vec<&Seated> = entries.iter().collect();
    sorted.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.id.cmp(&b.id)));
    let at = sorted
        .iter()
        .rposition(|seated| order(&seated.standing(), newcomer) != Ordering::Greater)
        .map_or(0, |before| before + 1);
    let rank = at
        .checked_sub(1)
        .map_or(1, |before| sorted[before].rank.max(0) + 1);
    let mut frontier = rank;
    let mut moves = Vec::new();
    for seated in &sorted[at..] {
        if seated.rank < rank {
            continue;
        }
        if seated.rank > frontier {
            break;
        }
        frontier = frontier.max(seated.rank + 1);
        moves.push(seated.id.clone());
    }
    (rank, moves)
}

/// An NPC's initiative (Initiative@1, `initiative`): its d20 plus the
/// modifier of its DEX score.
#[must_use]
pub fn initiative(roll: i64, dex: i64) -> i64 {
    roll + modifier(dex)
}

/// A score's modifier: half its distance to 10, rounded down
/// (AbilityModifier@1).
#[must_use]
pub fn modifier(score: i64) -> i64 {
    (score - 10).div_euclid(2)
}

/// The follow-ups of the joining that seats `newcomer`: one move for each
/// of `moves`, in rank order, each carrying the joining's `seal`.
#[must_use]
pub fn follow_ups(newcomer: &str, moves: &[String], seal: &str) -> Vec<Submission> {
    moves
        .iter()
        .map(|entry| Submission {
            data_capability: DATA_CAPABILITY.to_owned(),
            payload: Row::from([
                ("shift".to_owned(), json!(entry)),
                ("newcomer".to_owned(), json!(newcomer)),
                ("seal".to_owned(), json!(seal)),
            ]),
            target_id: Some(entry.clone()),
            ..Submission::default()
        })
        .collect()
}

/// Whether the entry `shifted` may move one rank down to make room for the
/// entry `newcomer`, against their combat's row `combat` and its other
/// participants `others`, each as the queue leaves it: both are entries of
/// that combat, which is open, the newcomer is not the one moved and ranks
/// at or above it, and another participant holds its rank.
///
/// # Errors
///
/// [`Refusal::NotAShift`].
pub fn shiftable(
    shifted: &Row,
    newcomer: &Row,
    combat: &Row,
    others: &[Seated],
) -> Result<(), Refusal> {
    let text = |row: &Row, field: &str| row.get(field).and_then(Value::as_str).and_then(uuid);
    let (Some(moved), Some(seated)) = (Seated::of(shifted), Seated::of(newcomer)) else {
        return Err(Refusal::NotAShift);
    };
    let combat_id = text(shifted, "initiativeEntryCombatId");
    let holds_its_rank = others
        .iter()
        .any(|other| other.id != moved.id && other.rank == moved.rank);
    if combat_id.is_none()
        || text(newcomer, "initiativeEntryCombatId") != combat_id
        || text(combat, "combatId") != combat_id
        || combat.get("endedAt").is_some_and(|ended| !ended.is_null())
        || seated.id == moved.id
        || seated.rank > moved.rank
        || !holds_its_rank
    {
        return Err(Refusal::NotAShift);
    }
    Ok(())
}

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// What the joinings issued and their moves have not taken yet: per
/// newcomer's entry, the seal of its latest joining and the entries whose
/// move is still to come. Held in the process, so a restart refuses the moves
/// in flight; a newer joining of the same entry voids the older one's.
#[derive(Debug, Default)]
pub struct Seals(Mutex<BTreeMap<String, Sealed>>);

/// The seal of a newcomer's joining, and the entries still to move.
#[derive(Debug)]
struct Sealed {
    seal: String,
    pending: BTreeSet<String>,
}

impl Seals {
    fn held(&self) -> MutexGuard<'_, BTreeMap<String, Sealed>> {
        // The map holds plain data: a panic elsewhere left it whole.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records the joining of `newcomer` sealed by `seal`, whose moves are
    /// `entries`, and voids the earlier one's.
    pub fn issue(&self, newcomer: &str, seal: &str, entries: &[String]) {
        self.held().insert(
            newcomer.to_owned(),
            Sealed {
                seal: seal.to_owned(),
                pending: entries.iter().cloned().collect(),
            },
        );
    }

    /// Whether `seal` is the one the latest joining of `newcomer` left for
    /// the move of `entry`.
    #[must_use]
    pub fn holds(&self, newcomer: &str, seal: &str, entry: &str) -> bool {
        self.held()
            .get(newcomer)
            .is_some_and(|sealed| sealed.seal == seal && sealed.pending.contains(entry))
    }

    /// Takes the move of `entry` off the pending ones: `false` when
    /// [`Seals::holds`] would not have said so, or when it was taken.
    pub fn take(&self, newcomer: &str, seal: &str, entry: &str) -> bool {
        let mut held = self.held();
        let Some(sealed) = held.get_mut(newcomer) else {
            return false;
        };
        let taken = sealed.seal == seal && sealed.pending.remove(entry);
        if sealed.pending.is_empty() {
            held.remove(newcomer);
        }
        taken
    }
}

/// The entries whose joining is in flight: claimed when prepared, so a
/// second joining of the same token prepared before the first is queued —
/// both would pass the queue's check — is refused, and released by time, the
/// queue holding the entry from then on. Held in the process.
#[derive(Debug, Default)]
pub struct Claims(Mutex<BTreeMap<String, u64>>);

impl Claims {
    fn held(&self) -> MutexGuard<'_, BTreeMap<String, u64>> {
        // The map holds plain data: a panic elsewhere left it whole.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Claims `entry` at `now` (milliseconds): `false` when a claim made
    /// less than [`CLAIM_MILLIS`] ago holds it.
    pub fn claim(&self, entry: &str, now: u64) -> bool {
        let live = |at: &u64| now.saturating_sub(*at) < CLAIM_MILLIS;
        let mut held = self.held();
        if held.get(entry).is_some_and(live) {
            return false;
        }
        // The claims that ran out are of no use any more.
        held.retain(|_, at| live(at));
        held.insert(entry.to_owned(), now);
        true
    }
}

/// A joining made the entry it inserts ([`prepare`]), judged against the
/// engine's rows and the tables as the queue leaves them; its moves are its
/// follow-ups ([`follow_ups`]), each judged again ([`shiftable`]). One
/// naming no id, or an initiative the tables could not hold, is refused,
/// 422; one naming a token not on the instance, 404; one the combat
/// forbids, 422 — nothing queued.
struct Joins {
    /// The dice its joinings throw.
    dice: Mutex<Box<dyn Dice>>,
    /// The seals its joinings issued for their moves.
    seals: Seals,
    /// The tokens whose joining is in flight.
    claims: Claims,
}

#[async_trait]
impl Prepare for Joins {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        // A `shift` of null names no move, as an absent one.
        if let Some(shift) = payload.get("shift").filter(|shift| !shift.is_null()) {
            return self.shift_of(context, payload, shift).await;
        }
        let request = request(payload, target_id).map_err(refused)?;
        let token = context.row(&partition(&request.token)).await?;
        let character = character_of(&request, &token).map_err(refused)?;
        // Over memory the tables hold nothing: no combat is found.
        let Some(rows) = context.rows else {
            return Err(refused(Refusal::NoOpenCombat));
        };
        let Some(combat) = open_combat(context, rows, &request.instance).await? else {
            return Err(refused(Refusal::NoOpenCombat));
        };
        let entries = participants(context, rows, &combat, &request.instance).await?;
        let sheet = rows
            .rows(SHEET, &[json!(character)])
            .await
            .map_err(failed)?;
        let character = sheet.get(0).map(|sheet| Character {
            kind: sheet["kind"].as_str().unwrap_or_default().to_owned(),
            dex: sheet["dex"].as_i64(),
        });
        let seating = Seating {
            combat: Some(combat),
            entries,
            character,
        };
        let joined = {
            let mut dice = self
                .dice
                .lock()
                .map_err(|_| plugins::Refusal::unprocessable("le dé n'a pas pu être lancé"))?;
            prepare(&request, &seating, dice.as_mut()).map_err(refused)?
        };
        let id = joined.entry["initiativeEntryId"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        // No await from here to the return: the claim and the seal are made
        // together, and a joining the claim refuses leaves no seal.
        if !self.claims.claim(&id, context.now_millis) {
            return Err(refused(Refusal::Joining));
        }
        let seal = context.ids.fresh();
        self.seals.issue(&id, &seal, &joined.moves);
        Ok(plugins::Prepared {
            then: follow_ups(&id, &joined.moves, &seal),
            target_id: Some(id),
            payload: joined.entry,
            ..plugins::Prepared::default()
        })
    }
}

impl Joins {
    /// The move `shift` asks for, making room for the newcomer the payload
    /// names: one rank down, the step the engine adds to the rank as the
    /// queue leaves it, once, under the seal its joining issued.
    async fn shift_of(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        shift: &Value,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let id = |value: Option<&Value>| {
            value
                .and_then(Value::as_str)
                .and_then(uuid)
                .ok_or_else(|| refused(Refusal::NotAShift))
        };
        let (shift, newcomer) = (id(Some(shift))?, id(payload.get("newcomer"))?);
        let seal = payload
            .get("seal")
            .and_then(Value::as_str)
            .filter(|seal| self.seals.holds(&newcomer, seal, &shift))
            .ok_or_else(|| refused(Refusal::Unsealed))?;
        let shifted = context.row(&partition(&shift)).await?;
        let seated = context.row(&partition(&newcomer)).await?;
        let combat_id = shifted
            .get("initiativeEntryCombatId")
            .and_then(Value::as_str)
            .and_then(uuid)
            .ok_or_else(|| refused(Refusal::NotAShift))?;
        let combat = context.row(&partition(&combat_id)).await?;
        let instance = combat
            .get("combatInstanceId")
            .and_then(Value::as_str)
            .and_then(uuid)
            .ok_or_else(|| refused(Refusal::NotAShift))?;
        let Some(rows) = context.rows else {
            return Err(refused(Refusal::NotAShift));
        };
        let others = participants(context, rows, &combat_id, &instance).await?;
        shiftable(&shifted, &seated, &combat, &others).map_err(refused)?;
        // Taken only now: a move refused above can be sent again.
        if !self.seals.take(&newcomer, seal, &shift) {
            return Err(refused(Refusal::Unsealed));
        }
        Ok(plugins::Prepared {
            target_id: Some(shift),
            payload: Row::from([("rank".to_owned(), json!(1))]),
            ..plugins::Prepared::default()
        })
    }
}

/// The open combat of `instance`, as the queue leaves it: an end queued
/// ahead closes it.
async fn open_combat(
    context: &Context<'_>,
    rows: &dyn Rows,
    instance: &str,
) -> Result<Option<String>, plugins::Refusal> {
    let found = rows
        .rows(COMBAT, &[json!(instance)])
        .await
        .map_err(failed)?;
    let Some(combat) = found
        .get(0)
        .and_then(|row| row["combat"].as_str())
        .and_then(uuid)
    else {
        return Ok(None);
    };
    let row = context.row(&partition(&combat)).await?;
    Ok(row
        .get("endedAt")
        .is_none_or(Value::is_null)
        .then_some(combat))
}

/// The participants of `combat` on `instance`, each entry as the queue
/// leaves it: the ones the engine holds whose token is on the map, and each
/// token's entry still queued on the partition its id names.
async fn participants(
    context: &Context<'_>,
    rows: &dyn Rows,
    combat: &str,
    instance: &str,
) -> Result<Vec<Seated>, plugins::Refusal> {
    let held = rows
        .rows(PARTICIPANTS, &[json!(combat), json!(instance)])
        .await
        .map_err(failed)?;
    let ids = |field: &str| -> Vec<String> {
        held.get(0)
            .and_then(|row| row[field].as_array())
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(uuid)
            .collect()
    };
    let candidates: BTreeSet<String> = ids("entries")
        .into_iter()
        .chain(ids("tokens").iter().map(|token| entry_id(combat, token)))
        .collect();
    let mut seated = Vec::new();
    for id in candidates {
        let row = context.row(&partition(&id)).await?;
        let of_combat = row
            .get("initiativeEntryCombatId")
            .and_then(Value::as_str)
            .and_then(uuid)
            .is_some_and(|of| of == combat);
        // Every entry is written on the partition its own id names.
        if let Some(entry) = Seated::of(&row).filter(|_| of_combat) {
            seated.push(entry);
        }
    }
    Ok(seated)
}

/// The engine's refusal of a read that failed.
fn failed(e: harness_dataguard::Error) -> plugins::Refusal {
    plugins::Refusal::unprocessable(e.to_string())
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
