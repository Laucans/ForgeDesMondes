//! DataCapability `table.startCombat` (`data-capability.json`): the DM starts
//! the fight on a battlemap instance and the order is ready at once —
//! monsters rolled, PCs entered, ties settled the D&D way (Initiative@1).
//!
//! The engine writes one partition per command and judges a row alone: the
//! combat is its own row, `BattlemapInstance/<its id>`, and so is each
//! participant's place in it, `BattlemapInstance/<its entry id>`. A start is
//! the command that writes the combat, then its follow-ups
//! ([`plugins::Prepared::then`]), one per participant, each seating one entry
//! ([`seat`]). Their ids are derived ([`combat_id`], [`entry_id`]): the
//! combat names its first participant's entry before it is written, and a
//! combat queued and not yet applied is found on its partition, so a second
//! start sent once the first is queued finds it open. Two sent at the very
//! same moment both land on the same partition: the engine parks the second
//! until its author confirms the overwrite (`confirm_on_stale`), and its
//! seats are refused, the entries being the first one's.
//!
//! A start is not atomic with its seats ([`plugins::Prepared::then`]): a
//! seat refused — its token taken off the map in between — stops the ones
//! after it, and the combat stays open with the participants seated.
//!
//! What one row cannot tell — the instance's tokens, each character's kind
//! and DEX, whether a combat of the instance is still open — is read before
//! the start is queued ([`LINEUP`], [`Lineup`]); [`prepare`] turns the
//! request and that lineup into the rows:
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
use std::sync::Mutex;

use async_trait::async_trait;
use harness_dataguard::api::Submission;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::{Value, json};
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.startCombat";

/// The DEX score an NPC whose sheet gives none rolls with: an average one,
/// no modifier.
pub const DEFAULT_DEX: i64 = 10;

/// The aggregate a combat and an entry are rows of.
const AGGREGATE: &str = "BattlemapInstance";

/// The kind of character that is a PC, as `entity.type` writes it.
const PC: &str = "pj";

/// The kind of character that is an NPC.
const NPC: &str = "pnj";

/// A d20.
const D20: std::ops::RangeInclusive<i64> = 1..=20;

/// The namespace every combat's and entry's id is derived in. It never
/// changes: a combat started before keeps its id.
const NAMESPACE: Uuid = Uuid::from_u128(0x5a1c_e0b4_27d9_4f63_9b1e_c4f0_83a6_2d17);

/// What the tables and the engine's rows hold of the instance `$1`, now
/// being `$2` milliseconds since the epoch: whether the table holds the
/// instance; its tokens the engine holds — a token is the engine's row,
/// which no projection writes into `token` yet — with their character, its
/// kind and its DEX score — a PC or an NPC with its sheet alone —, by id:
/// the order the ties keep; whether a combat of the instance has no end, in
/// the table or in the engine's rows; and now. What the engine tombstoned
/// is not there.
pub const LINEUP: &str = "WITH gone AS (SELECT partition FROM dataguard_rows WHERE tombstoned),
    placed AS (
        SELECT lower(r.fields ->> 'tokenId') AS token,
            lower(r.fields ->> 'characterId') AS character
        FROM dataguard_rows r
        WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
            AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'tokenId')
            AND lower(r.fields ->> 'tokenInstanceId') = $1::text
    )
    SELECT
        EXISTS (SELECT 1 FROM battlemap_instance i WHERE i.id::text = $1::text
            AND 'BattlemapInstance/' || i.id::text NOT IN (SELECT partition FROM gone))
            AS instance,
        (SELECT coalesce(jsonb_agg(jsonb_build_object('token', p.token,
                'character', p.character, 'kind', e.type, 'dex', c.dexterity)
                ORDER BY p.token), '[]')
            FROM placed p
            LEFT JOIN (entity e JOIN character c ON c.entity_id = e.id)
                ON e.id::text = p.character AND e.type IN ('pj', 'pnj')
                    AND 'Entity/' || e.id::text NOT IN (SELECT partition FROM gone))
            AS tokens,
        (EXISTS (SELECT 1 FROM combat c
                WHERE c.instance_id::text = $1::text AND c.ended_at IS NULL
                    AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
                        WHERE r.partition = 'BattlemapInstance/' || c.id::text
                            AND (r.tombstoned OR r.fields ->> 'endedAt' IS NOT NULL)))
            OR EXISTS (SELECT 1 FROM dataguard_rows r
                WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
                    AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'combatId')
                    AND lower(r.fields ->> 'combatInstanceId') = $1::text
                    AND r.fields ->> 'endedAt' IS NULL))
            AS open,
        to_timestamp($2::int8 / 1000.0) AS now";

/// Whether the engine ever wrote the combat partition `$1`, or an entry of
/// the combat `$2`: a tombstone (a reset) is a row too, and an entry seated
/// for a combat whose own row never landed takes its id all the same — a
/// new combat lands on neither.
pub const WRITTEN: &str = "SELECT EXISTS (SELECT 1 FROM dataguard_rows WHERE partition = $1::text)
        OR EXISTS (SELECT 1 FROM dataguard_rows WHERE partition LIKE 'BattlemapInstance/%'
            AND lower(fields ->> 'initiativeEntryCombatId') = $2::text) AS written";

/// One partition's row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// The partition of the aggregate a combat or an entry of id `id` is
/// written on.
#[must_use]
pub fn partition(id: &str) -> String {
    format!("{AGGREGATE}/{id}")
}

/// The id of the `generation`-th combat of `instance`, lowercase and
/// hyphenated: the first one is generation 0, and each start takes the
/// first one no combat took.
#[must_use]
pub fn combat_id(instance: &str, generation: u32) -> String {
    Uuid::new_v5(
        &NAMESPACE,
        format!("combat:{instance}:{generation}").as_bytes(),
    )
    .to_string()
}

/// The id of the entry of `token` in `combat`, lowercase and hyphenated.
#[must_use]
pub fn entry_id(combat: &str, token: &str) -> String {
    Uuid::new_v5(&NAMESPACE, format!("entry:{combat}:{token}").as_bytes()).to_string()
}

/// What a start asks for: the instance by its id, the PCs' initiatives by
/// character and the NPCs' rolls by token, ids lowercase and hyphenated.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    /// The battlemap instance whose combat starts.
    pub instance: String,
    /// The initiative the DM entered, by PC character.
    pub initiative: BTreeMap<String, i64>,
    /// The d20 rolled at the table, by NPC token.
    pub rolls: BTreeMap<String, i64>,
}

/// A token of the instance, as the engine holds it, and its character's
/// sheet.
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
    /// Whether the tables or the engine hold the instance.
    pub instance: bool,
    /// Its tokens, in the order the ties keep.
    pub tokens: Vec<Participant>,
    /// Whether a combat of the instance has no end yet, applied or queued.
    pub open: bool,
}

/// The randomness a start draws from, injected: the server's throws dice
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

/// The rows a start writes: the combat, then its entries in rank order.
#[derive(Clone, Debug, PartialEq)]
pub struct Started {
    /// The combat's row.
    pub combat: Row,
    /// Each entry's row, rank 1 first.
    pub entries: Vec<Row>,
}

/// Why a start, or a seat, is refused before it is queued.
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
    /// A seat's entry is not one the tables could hold.
    NotAnEntry,
    /// A seat's entry is not the one its combat names for its token, or its
    /// combat is not open, or its token not on the combat's instance.
    NotOfItsCombat,
    /// A seat's entry is seated already.
    Seated,
}

impl Refusal {
    /// Whether the refusal is of something the request names that does not
    /// exist (404) rather than of a request that cannot be done (422).
    #[must_use]
    pub fn not_found(&self) -> bool {
        matches!(self, Self::UnknownInstance)
    }
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
            Self::CombatOpen => {
                f.write_str("one-active-combat: un combat est déjà en cours sur cette carte")
            }
            Self::NoParticipant => f.write_str("aucun token sur la carte : pas de combat"),
            Self::UnknownCharacter => f.write_str("un token n'est plus ni un PJ ni un PNJ"),
            Self::MissingInitiative(character) => {
                write!(f, "l'initiative du PJ {character} n'est pas saisie")
            }
            Self::RollOfNoNpc => f.write_str("un jet ne nomme aucun token de PNJ de la carte"),
            Self::NotAnEntry => f.write_str("cette entrée d'initiative n'est pas valide"),
            Self::NotOfItsCombat => {
                f.write_str("instance-isolated: cette entrée n'est pas celle de son combat")
            }
            Self::Seated => f.write_str("cette entrée d'initiative est déjà placée"),
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
        Starts {
            dice: Mutex::new(Box::new(dice)),
        },
    )
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

/// The rows `request` starts, against what `lineup` holds, as the combat
/// `combat` at `now` (RFC 3339), with the randomness of `dice`: per token,
/// in the lineup's order, an NPC with no roll sent throws a d20, then every
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
    combat: &str,
    now: &str,
    dice: &mut dyn Dice,
) -> Result<Started, Refusal> {
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
                (Some(roll), self::initiative(roll, dex), Some(dex))
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
    let entries: Vec<Row> = ranked(&standings)
        .into_iter()
        .zip(1_i64..)
        .map(|(at, rank)| {
            let (participant, roll, standing) = &seated[at];
            Row::from([
                (
                    "initiativeEntryId".to_owned(),
                    json!(entry_id(combat, &participant.token)),
                ),
                ("initiativeEntryCombatId".to_owned(), json!(combat)),
                (
                    "initiativeEntryTokenId".to_owned(),
                    json!(participant.token),
                ),
                ("roll".to_owned(), json!(roll)),
                ("initiative".to_owned(), json!(standing.initiative)),
                ("dexScore".to_owned(), json!(standing.dex)),
                ("tiebreak".to_owned(), json!(standing.draw)),
                ("rank".to_owned(), json!(rank)),
            ])
        })
        .collect();
    let current = entries
        .first()
        .and_then(|entry| entry.get("initiativeEntryId"))
        .cloned()
        .unwrap_or(Value::Null);
    let combat = Row::from([
        ("combatId".to_owned(), json!(combat)),
        ("combatInstanceId".to_owned(), json!(request.instance)),
        ("round".to_owned(), json!(1)),
        ("currentEntryId".to_owned(), current),
        ("startedAt".to_owned(), json!(now)),
    ]);
    Ok(Started { combat, entries })
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
#[must_use]
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

/// The follow-ups of a start whose combat is the command itself: one seat
/// for each of `entries`, in rank order.
#[must_use]
pub fn follow_ups(entries: &[Row]) -> Vec<Submission> {
    entries
        .iter()
        .map(|entry| Submission {
            data_capability: DATA_CAPABILITY.to_owned(),
            payload: Row::from([("entry".to_owned(), json!(entry))]),
            target_id: entry
                .get("initiativeEntryId")
                .and_then(Value::as_str)
                .map(str::to_owned),
            ..Submission::default()
        })
        .collect()
}

/// The entry `wire` — a seat's `entry` — writes, its ids lowercase and
/// hyphenated, its fields the eight of `initiative_entry` alone.
///
/// # Errors
///
/// [`Refusal::NotAnEntry`] for an entry the tables could not hold: an id
/// that is no uuid, a roll out of 1 to 20, an initiative or a DEX score no
/// `smallint` holds, a draw that is no number, a rank below 1.
pub fn seat(wire: &Value) -> Result<Row, Refusal> {
    let field = |name: &str| wire.get(name).unwrap_or(&Value::Null);
    let id = |name: &str| {
        field(name)
            .as_str()
            .and_then(uuid)
            .ok_or(Refusal::NotAnEntry)
    };
    let small = |value: &Value| value.as_i64().filter(|n| i16::try_from(*n).is_ok());
    let nullable = |name: &str, holds: &dyn Fn(i64) -> bool| match field(name) {
        Value::Null => Ok(Value::Null),
        value => small(value)
            .filter(|n| holds(*n))
            .map(Value::from)
            .ok_or(Refusal::NotAnEntry),
    };
    let row = Row::from([
        (
            "initiativeEntryId".to_owned(),
            json!(id("initiativeEntryId")?),
        ),
        (
            "initiativeEntryCombatId".to_owned(),
            json!(id("initiativeEntryCombatId")?),
        ),
        (
            "initiativeEntryTokenId".to_owned(),
            json!(id("initiativeEntryTokenId")?),
        ),
        ("roll".to_owned(), nullable("roll", &|n| D20.contains(&n))?),
        (
            "initiative".to_owned(),
            json!(small(field("initiative")).ok_or(Refusal::NotAnEntry)?),
        ),
        ("dexScore".to_owned(), nullable("dexScore", &|_| true)?),
        (
            "tiebreak".to_owned(),
            field("tiebreak")
                .as_f64()
                .filter(|draw| draw.is_finite())
                .map(|_| field("tiebreak").clone())
                .ok_or(Refusal::NotAnEntry)?,
        ),
        (
            "rank".to_owned(),
            json!(
                field("rank")
                    .as_i64()
                    .filter(|rank| *rank >= 1 && i32::try_from(*rank).is_ok())
                    .ok_or(Refusal::NotAnEntry)?
            ),
        ),
    ]);
    Ok(row)
}

/// Whether the entry `entry` may be seated, against its combat's row
/// `combat`, its token's `token` and its own partition's `seated`, each as
/// the queue leaves it: the entry is the one its combat names for its token
/// ([`entry_id`]), the combat is open, the token is on the combat's
/// instance, and the entry is not seated yet.
///
/// # Errors
///
/// [`Refusal::NotOfItsCombat`] and [`Refusal::Seated`].
pub fn seatable(entry: &Row, combat: &Row, token: &Row, seated: &Row) -> Result<(), Refusal> {
    let text = |row: &Row, field: &str| {
        row.get(field)
            .and_then(Value::as_str)
            .and_then(uuid)
            .unwrap_or_default()
    };
    let combat_id = text(entry, "initiativeEntryCombatId");
    let token_id = text(entry, "initiativeEntryTokenId");
    let instance = text(combat, "combatInstanceId");
    if text(entry, "initiativeEntryId") != entry_id(&combat_id, &token_id)
        || text(combat, "combatId") != combat_id
        || instance.is_empty()
        || combat.get("endedAt").is_some_and(|ended| !ended.is_null())
        || text(token, "tokenId") != token_id
        || text(token, "tokenInstanceId") != instance
    {
        return Err(Refusal::NotOfItsCombat);
    }
    if !seated.is_empty() {
        return Err(Refusal::Seated);
    }
    Ok(())
}

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// A start made the combat it writes ([`prepare`]), judged against the
/// tables ([`LINEUP`]) and the combats of the instance as the queue leaves
/// them; its entries are its follow-ups ([`follow_ups`]), each a seat
/// ([`seat`], [`seatable`]). One naming no id, or figures the tables could
/// not hold, is refused, 422; one naming an instance that is not there,
/// 404; one the lineup forbids, 422 — nothing queued.
struct Starts {
    /// The dice its starts throw.
    dice: Mutex<Box<dyn Dice>>,
}

#[async_trait]
impl Prepare for Starts {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        // An `entry` of null names no seat, as an absent one.
        if let Some(entry) = payload.get("entry").filter(|entry| !entry.is_null()) {
            let entry = seat(entry).map_err(refused)?;
            let id = |field: &str| {
                entry
                    .get(field)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            let combat = context
                .row(&partition(&id("initiativeEntryCombatId")))
                .await?;
            let token = context
                .row(&partition(&id("initiativeEntryTokenId")))
                .await?;
            let seated = context.row(&partition(&id("initiativeEntryId"))).await?;
            seatable(&entry, &combat, &token, &seated).map_err(refused)?;
            return Ok(plugins::Prepared {
                target_id: Some(id("initiativeEntryId")),
                payload: entry,
                ..plugins::Prepared::default()
            });
        }
        let request = request(payload, target_id).map_err(refused)?;
        // Over memory the tables hold nothing: no instance is found.
        let Some(rows) = context.rows else {
            return Err(refused(Refusal::UnknownInstance));
        };
        let failed = |e: harness_dataguard::Error| plugins::Refusal::unprocessable(e.to_string());
        let held = rows
            .rows(
                LINEUP,
                &[json!(request.instance), json!(context.now_millis)],
            )
            .await
            .map_err(failed)?;
        let held = &held[0];
        let instance = context.row(&partition(&request.instance)).await?;
        let mut lineup = Lineup {
            instance: held["instance"].as_bool().unwrap_or(false)
                || instance
                    .get("battlemapInstanceId")
                    .is_some_and(|id| !id.is_null()),
            tokens: held["tokens"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|token| Participant {
                    token: token["token"].as_str().unwrap_or_default().to_owned(),
                    character: token["character"].as_str().unwrap_or_default().to_owned(),
                    kind: token["kind"].as_str().map(str::to_owned),
                    dex: token["dex"].as_i64(),
                })
                .collect(),
            open: held["open"].as_bool().unwrap_or(false),
        };
        // The instance's combats, applied or queued, on the partitions their
        // ids name: the first never written is the new one's.
        let mut generation = 0;
        let combat = loop {
            let id = combat_id(&request.instance, generation);
            let row = context.row(&partition(&id)).await?;
            if row.is_empty() {
                let written = rows
                    .rows(WRITTEN, &[json!(partition(&id)), json!(id)])
                    .await
                    .map_err(failed)?;
                if !written[0]["written"].as_bool().unwrap_or(false) {
                    break id;
                }
            } else if row.get("endedAt").is_none_or(Value::is_null) {
                lineup.open = true;
            }
            generation += 1;
        };
        let now = held["now"]
            .as_str()
            .ok_or_else(|| plugins::Refusal::unprocessable("l'heure n'a pas pu être lue"))?;
        let started = {
            let mut dice = self
                .dice
                .lock()
                .map_err(|_| plugins::Refusal::unprocessable("les dés n'ont pas pu être lancés"))?;
            prepare(&request, &lineup, &combat, now, dice.as_mut()).map_err(refused)?
        };
        Ok(plugins::Prepared {
            target_id: Some(combat),
            payload: started.combat,
            then: follow_ups(&started.entries),
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
