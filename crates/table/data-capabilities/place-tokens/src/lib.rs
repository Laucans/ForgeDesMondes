//! DataCapability `table.placeTokens` (`data-capability.json`): the DM drops
//! a character on a battlemap instance at a hex — a monster « × 3 » at once —
//! and gets that many tokens, each with its own number and hit points.
//!
//! The engine writes one partition per command and judges a row alone: a
//! token is its own row, `BattlemapInstance/<its id>`, and a drop of n tokens
//! is n commands — the first is the one submitted, the others its follow-ups
//! ([`plugins::Prepared::then`]), each a drop of one that numbers itself
//! after the ones queued ahead of it. A token's id is derived from its
//! instance, its character and its number ([`token_id`]): a token queued and
//! not yet applied is found on its partition, so the next drop numbers after
//! it and a PC queued once is not queued twice. Two drops prepared side by
//! side both choose the same numbers: the aggregate refuses the later token
//! (`multi-drop-numbering`), which never lands on one applied or queued.
//!
//! What one row cannot tell — the instance's session, the character's sheet,
//! whether a PC is present, the numbers already taken — is read before the
//! drop is queued ([`Roster`]); [`prepare`] turns the request and that roster
//! into the token rows:
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

use async_trait::async_trait;
use harness_dataguard::api::Submission;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::{Value, json};
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.placeTokens";

/// The most tokens one drop makes: the drop dialog's « × 12 ».
pub const MAX_COUNT: u16 = 12;

/// The aggregate a token is a row of.
const AGGREGATE: &str = "BattlemapInstance";

/// The kind of character that is a PC, as `entity.type` writes it.
const PC: &str = "pj";

/// The kind of character that is an NPC.
const NPC: &str = "pnj";

/// The namespace every token's id is derived in. It never changes: a token
/// dropped before keeps its id.
const NAMESPACE: Uuid = Uuid::from_u128(0x2d84_7a1e_93c6_5f0b_8e27_41ad_6c09_b3f5);

/// What the tables hold of the instance `$1` and the character `$2`, the
/// instance's session being `$3` when its engine row names it: the session's
/// campaign; the character's campaign, kind, name and HP max — a PC or an NPC
/// with its sheet alone, which a token's foreign key names; whether it is
/// present at the session, in the table or in the Session's engine rows; and
/// the tokens it has on the instance, in the table or in the engine's rows,
/// with the highest number they hold. What the engine tombstoned is not
/// there — but the ids of the character's tokens it removed from the
/// instance are listed, as a tombstone keeps its row's fields: a new token
/// never takes one ([`fresh_token_id`]).
pub const ROSTER: &str = "WITH gone AS (SELECT partition FROM dataguard_rows WHERE tombstoned),
    instance AS (
        SELECT coalesce($3::text,
            (SELECT session_id::text FROM battlemap_instance WHERE id = $1::text::uuid
                AND 'BattlemapInstance/' || id::text NOT IN (SELECT partition FROM gone)))
            AS session
    ),
    placed AS (
        SELECT t.id::text AS id, t.number::bigint AS number FROM token t
        WHERE t.instance_id = $1::text::uuid AND t.character_id = $2::text::uuid
            AND 'BattlemapInstance/' || t.id::text NOT IN (SELECT partition FROM gone)
        UNION
        SELECT lower(r.fields ->> 'tokenId'), (r.fields ->> 'number')::numeric::bigint
        FROM dataguard_rows r
        WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
            AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'tokenId')
            AND lower(r.fields ->> 'tokenInstanceId') = $1::text
            AND lower(r.fields ->> 'characterId') = $2::text
    )
    SELECT
        (SELECT s.campaign_id::text FROM session s JOIN instance i ON s.id::text = i.session
            WHERE 'Session/' || s.id::text NOT IN (SELECT partition FROM gone)) AS campaign,
        e.campaign_id::text AS character_campaign, e.type AS kind, e.name AS name,
        c.hp_max::bigint AS hp_max,
        (EXISTS (SELECT 1 FROM session_present_character p JOIN instance i
                ON p.session_id::text = i.session WHERE p.character_id = $2::text::uuid)
            OR EXISTS (SELECT 1 FROM dataguard_rows r JOIN instance i
                ON lower(r.fields ->> 'sessionPresentCharacterSessionId') = i.session
                WHERE r.partition LIKE 'Session/%' AND NOT r.tombstoned
                    AND lower(r.fields ->> 'sessionPresentCharacterCharacterId') = $2::text))
            AS present,
        (SELECT count(DISTINCT id) FROM placed) AS placed,
        (SELECT max(number) FROM placed) AS highest,
        (SELECT coalesce(jsonb_agg(substr(r.partition, length('BattlemapInstance/') + 1)), '[]')
            FROM dataguard_rows r
            WHERE r.partition LIKE 'BattlemapInstance/%' AND r.tombstoned
                AND lower(r.fields ->> 'tokenInstanceId') = $1::text
                AND lower(r.fields ->> 'characterId') = $2::text) AS removed
    FROM (SELECT 1) AS one
    LEFT JOIN (entity e JOIN character c ON c.entity_id = e.id)
        ON e.id = $2::text::uuid AND e.type IN ('pj', 'pnj')
            AND 'Entity/' || e.id::text NOT IN (SELECT partition FROM gone)";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a drop asks for: the instance and the character by their ids,
/// lowercase and hyphenated, the hex, and how many tokens.
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
    /// Whether the character has a token on the instance, applied or queued.
    pub placed: bool,
    /// The highest number its tokens hold on the instance, applied or
    /// queued; `None` when none holds one.
    pub highest: Option<i64>,
    /// The ids of its tokens the engine removed from the instance, whose
    /// tombstones a new token never lands on.
    pub removed: Vec<String>,
}

/// Why a drop is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A field that must be a uuid is not one: `instance` or `character`.
    NotAnId(&'static str),
    /// A coordinate is not a whole number the tables hold: `q` or `r`.
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

impl Refusal {
    /// Whether the refusal is of something the request names that does not
    /// exist (404) rather than of a request that cannot be done (422).
    #[must_use]
    pub fn not_found(&self) -> bool {
        matches!(self, Self::UnknownInstance | Self::UnknownCharacter)
    }
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
            Self::Absent => f.write_str("token-roster: ce PJ n'est pas présent à la séance"),
            Self::AlreadyPlaced => f.write_str("token-roster: ce PJ a déjà son token sur la carte"),
            Self::OnePcToken => f.write_str("token-roster: un PJ n'a qu'un token par carte"),
            Self::NoNumberLeft => {
                f.write_str("multi-drop-numbering: plus aucun numéro de token n'est libre")
            }
        }
    }
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Places)
}

/// The id of the token of `character` on `instance` numbered `number` — a
/// PC's has none — lowercase and hyphenated, as [`request`] gives ids: the
/// first one its number can take.
#[must_use]
pub fn token_id(instance: &str, character: &str, number: Option<i64>) -> String {
    nth_token_id(instance, character, number, 0)
}

/// The id a new token of `character` on `instance` numbered `number` takes:
/// the first one its number can take that is none of the ids in `removed`,
/// so a token removed keeps its tombstone when its number is dropped again.
#[must_use]
pub fn fresh_token_id(
    instance: &str,
    character: &str,
    number: Option<i64>,
    removed: &[String],
) -> String {
    (0..)
        .map(|generation| nth_token_id(instance, character, number, generation))
        .find(|id| !removed.iter().any(|gone| gone.eq_ignore_ascii_case(id)))
        .unwrap_or_default()
}

/// The `generation`-th id the token of `character` on `instance` numbered
/// `number` can take.
fn nth_token_id(instance: &str, character: &str, number: Option<i64>, generation: u32) -> String {
    let mut name = format!("token:{instance}:{character}");
    if let Some(number) = number {
        name.push_str(&format!(":{number}"));
    }
    if generation > 0 {
        name.push_str(&format!("#{generation}"));
    }
    Uuid::new_v5(&NAMESPACE, name.as_bytes()).to_string()
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

/// The token rows `request` drops, against what `roster` holds, in the
/// order they are numbered.
///
/// # Errors
///
/// [`Refusal::UnknownInstance`], [`Refusal::UnknownCharacter`],
/// [`Refusal::CharacterOfAnotherCampaign`]; for a PC, [`Refusal::OnePcToken`],
/// [`Refusal::Absent`] and [`Refusal::AlreadyPlaced`] (`token-roster`); for
/// an NPC, [`Refusal::NoNumberLeft`].
pub fn prepare(request: &Request, roster: &Roster) -> Result<Vec<Row>, Refusal> {
    let campaign = roster.campaign.as_deref().ok_or(Refusal::UnknownInstance)?;
    let character = roster
        .character
        .as_ref()
        .filter(|character| [PC, NPC].contains(&character.kind.as_str()))
        .ok_or(Refusal::UnknownCharacter)?;
    if !character.campaign.eq_ignore_ascii_case(campaign) {
        return Err(Refusal::CharacterOfAnotherCampaign);
    }
    let token = |number: Option<i64>, name: String, hp: Value| {
        Row::from([
            (
                "tokenId".to_owned(),
                json!(fresh_token_id(
                    &request.instance,
                    &request.character,
                    number,
                    &roster.removed
                )),
            ),
            ("tokenInstanceId".to_owned(), json!(request.instance)),
            ("characterId".to_owned(), json!(request.character)),
            ("number".to_owned(), json!(number)),
            ("name".to_owned(), json!(name)),
            ("q".to_owned(), json!(request.q)),
            ("r".to_owned(), json!(request.r)),
            ("visible".to_owned(), json!(true)),
            ("hpCurrent".to_owned(), hp.clone()),
            ("hpMax".to_owned(), hp),
            ("conditions".to_owned(), json!([])),
        ])
    };
    let name = character.name.trim();
    if character.kind == PC {
        if request.count != 1 {
            return Err(Refusal::OnePcToken);
        }
        if !roster.present {
            return Err(Refusal::Absent);
        }
        if roster.placed {
            return Err(Refusal::AlreadyPlaced);
        }
        return Ok(vec![token(None, name.to_owned(), Value::Null)]);
    }
    let first = roster.highest.unwrap_or(0).max(0) + 1;
    let last = first + i64::from(request.count) - 1;
    if i16::try_from(last).is_err() {
        return Err(Refusal::NoNumberLeft);
    }
    // An HP max the tables could not hold, or below 0, is none to copy.
    let hp = character
        .hp_max
        .filter(|hp| *hp >= 0 && i16::try_from(*hp).is_ok())
        .map_or(Value::Null, |hp| json!(hp));
    Ok((first..=last)
        .map(|number| token(Some(number), format!("{name} {number}"), hp.clone()))
        .collect())
}

/// The follow-ups of a drop of `request` whose first token is the command
/// itself: one drop of one token for each other token asked for, on the
/// same instance and hex, each numbering itself after the ones queued ahead.
#[must_use]
pub fn follow_ups(request: &Request) -> Vec<Submission> {
    (1..request.count)
        .map(|_| Submission {
            data_capability: DATA_CAPABILITY.to_owned(),
            payload: Row::from([
                ("character".to_owned(), json!(request.character)),
                ("q".to_owned(), json!(request.q)),
                ("r".to_owned(), json!(request.r)),
            ]),
            target_id: Some(request.instance.clone()),
            ..Submission::default()
        })
        .collect()
}

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// A drop made the token rows it writes ([`prepare`]), judged against the
/// tables ([`ROSTER`]) and the partitions as the queue leaves them: the
/// first token is the command's, on its own partition, and the others its
/// follow-ups ([`follow_ups`]). One naming no id, no hex or no count it can
/// make is refused, 422; one naming an instance or a character that is not
/// there, 404; one the roster forbids, 422 — nothing queued.
struct Places;

#[async_trait]
impl Prepare for Places {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let request = request(payload, target_id).map_err(refused)?;
        // Over memory the tables hold nothing: no instance is found.
        let Some(rows) = context.rows else {
            return Err(refused(Refusal::UnknownInstance));
        };
        // The instance the engine wrote names its session on its own row,
        // which `base` is; one the tables hold, in its table.
        let session = context
            .base
            .get("sessionId")
            .and_then(Value::as_str)
            .and_then(uuid);
        let held = rows
            .rows(
                ROSTER,
                &[
                    json!(request.instance),
                    json!(request.character),
                    json!(session),
                ],
            )
            .await
            .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
        let held = &held[0];
        let text = |column: &str| held[column].as_str().map(str::to_owned);
        let mut roster = Roster {
            campaign: text("campaign"),
            character: text("kind").map(|kind| Character {
                campaign: text("character_campaign").unwrap_or_default(),
                kind,
                name: text("name").unwrap_or_default(),
                hp_max: held["hp_max"].as_i64(),
            }),
            present: held["present"].as_bool().unwrap_or(false),
            placed: held["placed"].as_i64().unwrap_or(0) > 0,
            highest: held["highest"].as_i64(),
            removed: held["removed"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        };
        // The tokens queued and not yet applied, on the partitions their
        // ids name: a PC's one, an NPC's numbers after the highest applied.
        let queued = |number: Option<i64>| {
            let partition = format!(
                "{AGGREGATE}/{}",
                fresh_token_id(
                    &request.instance,
                    &request.character,
                    number,
                    &roster.removed
                )
            );
            async move { context.row(&partition).await }
        };
        roster.placed = roster.placed || !queued(None).await?.is_empty();
        let mut next = roster.highest.unwrap_or(0).max(0) + 1;
        while i16::try_from(next).is_ok() && !queued(Some(next)).await?.is_empty() {
            roster.highest = Some(next);
            next += 1;
        }
        let mut tokens = prepare(&request, &roster).map_err(refused)?.into_iter();
        let first = tokens.next().unwrap_or_default();
        Ok(plugins::Prepared {
            target_id: first
                .get("tokenId")
                .and_then(Value::as_str)
                .map(str::to_owned),
            payload: first,
            then: follow_ups(&request),
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
