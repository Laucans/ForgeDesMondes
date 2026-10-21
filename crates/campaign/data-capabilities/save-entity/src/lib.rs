//! DataCapability `campaign.saveEntity` (`data-capability.json`): the DM
//! saves an element — a PC, an NPC, a place or an event — with its sheet and
//! its aliases, in one explicit action, and the save is kept as a version.
//!
//! The engine writes one partition per command: a save is one command on
//! `Entity/<the element's id>`, carrying the fields it writes and the version
//! it adds, which the DataGuard's invariants judge and its store projects
//! into `entity`, `character`, `entity_alias` and `entity_version`. What the
//! DM sends is the wire payload of the editor; [`prepare`] turns it into that
//! state:
//!
//! - the editor's names are the aggregate's (`kind` is `type`, `pvMax` is
//!   `hpMax`, `stats.DEX` is `dexterity`…); a field it does not name is
//!   dropped, a DEX modifier first of all: it is computed from the score
//!   (AbilityModifier@1), never written;
//! - the aliases are kept as typed, a blank one or one that normalises
//!   (AliasMatch@1) as an earlier one dropped. That no other element of the
//!   campaign answers to one of them is the DataGuard's to judge;
//! - a PC or an NPC has its sheet, and a token colour to fall back on; an
//!   element made a place or an event loses its sheet, which its versions
//!   keep;
//! - a save the tables could never hold is refused before it is queued
//!   ([`Refusal`]): a new element names its campaign, its type and its name,
//!   an element stays in its campaign, an id is a uuid, a score, the AC and
//!   the HP max are integers a `smallint` holds, and no value holds the
//!   character U+0000 — every cast of the projection then succeeds;
//! - the version is the next one (EntityVersion@1, source `dm`): its number,
//!   the complete snapshot of the element as the save leaves it, and the diff
//!   from the previous snapshot. [`versioned`] computes it from the row before
//!   and the row after, here against the row the queue will have left; the
//!   Entity aggregate settles it again when the save is applied, after the
//!   element's last version.
//!
//! The engine runs [`plugin`]'s `Prepare`: on the element's row as the queue
//! leaves it, with fresh ids and now.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self as engine, Context, Plugin};
use serde_json::{Map, Value};
use unicode_normalization::UnicodeNormalization;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "campaign.saveEntity";

/// This DataCapability's manifest (contract F), as the engine parses it.
pub const MANIFEST: &str = include_str!("../data-capability.json");

/// Where a save by the DM comes from (EntityVersion@1).
pub const SOURCE: &str = "dm";

/// The colour a new PC's or NPC's token falls back on when the DM gave none
/// (`default-token`: a sheet has one).
pub const DEFAULT_TOKEN_COLOR: &str = "accent";

/// The types of element that have a character sheet.
const CHARACTERS: [&str; 2] = ["pj", "pnj"];

/// The types an element may have.
const TYPES: [&str; 4] = ["pj", "pnj", "lieu", "evenement"];

/// The editor's name of a field, and the aggregate's.
const FIELDS: [(&str, &str); 20] = [
    ("name", "name"),
    ("description", "description"),
    ("notes", "notes"),
    ("player", "playerId"),
    ("prompt", "initialPrompt"),
    ("background", "background"),
    ("behavior", "behavior"),
    ("ca", "armorClass"),
    ("pvMax", "hpMax"),
    ("spells", "spells"),
    ("features", "actions"),
    ("actions", "actions"),
    ("crop", "tokenCrop"),
    ("color", "tokenColor"),
    // The aggregate's own names, as a caller that knows them sends them.
    ("playerId", "playerId"),
    ("initialPrompt", "initialPrompt"),
    ("armorClass", "armorClass"),
    ("hpMax", "hpMax"),
    ("tokenColor", "tokenColor"),
    ("tokenCrop", "tokenCrop"),
];

/// The editor's names of the media, and the aggregate's: a media id or
/// `null`, nothing else.
const MEDIA: [(&str, &str); 4] = [
    ("portrait", "portraitMediaId"),
    ("portraitMediaId", "portraitMediaId"),
    ("token", "tokenMediaId"),
    ("tokenMediaId", "tokenMediaId"),
];

/// The six scores: the editor's abbreviation, and the aggregate's field.
const ABILITIES: [(&str, &str); 6] = [
    ("FOR", "strength"),
    ("DEX", "dexterity"),
    ("CON", "constitution"),
    ("INT", "intelligence"),
    ("SAG", "wisdom"),
    ("CHA", "charisma"),
];

/// The fields of the element and of its sheet a snapshot holds, beside its
/// `aliases`: what the DM sees and restores — never an id or a date.
const SNAPSHOT: [&str; 24] = [
    "type",
    "name",
    "description",
    "notes",
    "playerId",
    "initialPrompt",
    "background",
    "behavior",
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
    "armorClass",
    "hpMax",
    "spells",
    "actions",
    "portraitMediaId",
    "tokenMediaId",
    "tokenCrop",
    "tokenColor",
    "dndbeyondId",
    "dndbeyondSyncedAt",
];

/// The fields of the character sheet: a place or an event holds none.
const SHEET: [&str; 21] = [
    "characterEntityId",
    "playerId",
    "initialPrompt",
    "background",
    "behavior",
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
    "armorClass",
    "hpMax",
    "spells",
    "actions",
    "portraitMediaId",
    "tokenMediaId",
    "tokenCrop",
    "tokenColor",
    "dndbeyondId",
    "dndbeyondSyncedAt",
];

/// The fields that name a row of another table, which a uuid alone can.
const IDS: [&str; 5] = [
    "entityId",
    "entityCampaignId",
    "playerId",
    "portraitMediaId",
    "tokenMediaId",
];

/// The fields the tables hold as a `smallint`: the six scores, the AC and the
/// HP max.
const SMALLINTS: [&str; 8] = [
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
    "armorClass",
    "hpMax",
];

/// The row's field holding the element's aliases, an array of strings.
pub const ALIAS: &str = "alias";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a save is given besides its wire payload: the element's id — the one
/// `entity.id` names, else a new one the server made — the new version's,
/// and the time.
#[derive(Clone, Copy, Debug)]
pub struct Fresh<'a> {
    /// The element's id.
    pub entity: &'a str,
    /// The id of the version the save adds.
    pub version: &'a str,
    /// Now, RFC 3339.
    pub now: &'a str,
}

/// Why a save is refused before it is queued: what the tables could never
/// hold, which no invariant of the aggregate names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A new element names no campaign.
    NoCampaign,
    /// A new element has no type, or a type given is none of `pj`, `pnj`,
    /// `lieu`, `evenement`.
    NoType,
    /// A new element has no name, or one of blanks alone.
    NoName,
    /// An element saved under another campaign than its own.
    OtherCampaign,
    /// A field naming another row (the element, its campaign, its player, a
    /// media) is not a uuid.
    NotAnId(&'static str),
    /// A score, the AC or the HP max is not an integer a `smallint` holds.
    NotASmallint(&'static str),
    /// A field holds the character U+0000 somewhere, which neither `text`
    /// nor `jsonb` can hold.
    Nul(String),
    /// The aliases given are not a list: `null` would erase them all.
    NotAliases,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCampaign => f.write_str("un nouvel élément nomme sa campagne"),
            Self::NoType => f.write_str("un élément est un pj, un pnj, un lieu ou un evenement"),
            Self::NoName => f.write_str("un nouvel élément a un nom"),
            Self::OtherCampaign => f.write_str("un élément ne change pas de campagne"),
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::NotASmallint(field) => {
                write!(f, "{field} n'est pas un entier de -32768 à 32767")
            }
            Self::Nul(field) => write!(f, "{field} contient un caractère nul"),
            Self::NotAliases => f.write_str("les alias sont une liste"),
        }
    }
}

/// What a save becomes before it is submitted.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The element's id: the partition `Entity/<target_id>`.
    pub target_id: String,
    /// What the command writes on it, its version included.
    pub payload: Row,
}

/// The version a save adds (EntityVersion@1).
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    /// 1, then the previous number plus 1.
    pub number: u64,
    /// The element, its sheet and its aliases after the save, complete.
    pub snapshot: Map<String, Value>,
    /// What changed from the previous snapshot, `{field, old, new}` sorted
    /// by field.
    pub diff: Vec<Value>,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Save)
}

/// A save made what it writes ([`prepare`]): on the element `entity.id`
/// names — else the one the command targets, else a new one given a fresh
/// id — its row as the queue leaves it, a fresh version id and now; on the
/// partition its `entityId` names. A save the tables could never hold is
/// refused as unprocessable.
struct Save;

#[async_trait]
impl engine::Prepare for Save {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &Row,
        target_id: Option<&str>,
    ) -> Result<engine::Prepared, engine::Refusal> {
        let named = element_id(payload).or_else(|| target_id.map(str::to_owned));
        // The element's row as the queue leaves it — whether the command
        // targets it or `entity.id` alone names it — empty for a new one.
        let base = match &named {
            Some(id) => context.row(&format!("Entity/{id}")).await?,
            None => Row::new(),
        };
        let entity = named.unwrap_or_else(|| context.ids.fresh());
        let version = context.ids.fresh();
        let now = aggregate_entity::timestamp(context.now_millis);
        let fresh = Fresh {
            entity: &entity,
            version: &version,
            now: &now,
        };
        let prepared = prepare(payload, &base, fresh)
            .map_err(|refusal| engine::Refusal::unprocessable(refusal.to_string()))?;
        // The partition is the one the payload's `entityId` names.
        Ok(engine::Prepared {
            payload: prepared.payload,
            ..engine::Prepared::default()
        })
    }
}

/// The fields the editor sends: under `entity`, or the payload itself.
fn entity_of(wire: &Row) -> Map<String, Value> {
    match wire.get("entity") {
        Some(Value::Object(entity)) => entity.clone(),
        _ => wire.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
    }
}

/// `entity.id`, when it names one.
fn id_of(entity: &Map<String, Value>) -> Option<String> {
    entity
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(str::to_owned)
}

/// The element a save names, when it names one: `entity.id`.
#[must_use]
pub fn element_id(wire: &Row) -> Option<String> {
    match wire.get("entity") {
        Some(Value::Object(entity)) => id_of(entity),
        _ => wire
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .map(str::to_owned),
    }
}

/// Whether `text` is a uuid as PostgreSQL writes one: 8-4-4-4-12 hex digits.
fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Whether `a` and `b` name the same row: the same id, whatever its case.
fn same_id(a: &Value, b: &Value) -> bool {
    a.as_str()
        .zip(b.as_str())
        .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// The type an element's editor gives: `kind`, or a `type` that is one — the
/// editor's `type` is otherwise a PC's subtitle (« Elfe des bois… »).
fn kind_of(entity: &Map<String, Value>) -> Result<Option<&Value>, Refusal> {
    let valid = |kind: &Value| kind.as_str().is_some_and(|kind| TYPES.contains(&kind));
    match entity.get("kind") {
        Some(kind) if valid(kind) => Ok(Some(kind)),
        Some(_) => Err(Refusal::NoType),
        None => Ok(entity.get("type").filter(|kind| valid(kind))),
    }
}

/// The command a save of `wire` submits on the element `fresh.entity` — the
/// one `entity.id` names, else a new one — `base` being its row as the
/// commands queued ahead of it will leave it (empty for a new element).
///
/// # Errors
///
/// [`Refusal`] for a save the tables could never hold.
pub fn prepare(wire: &Row, base: &Row, fresh: Fresh<'_>) -> Result<Prepared, Refusal> {
    let entity = entity_of(wire);
    let id = fresh.entity.to_owned();
    let mut payload = Row::new();
    payload.insert("entityId".into(), id.clone().into());
    let campaign = wire.get("campaign").filter(|c| c.is_string());
    match (base.get("entityCampaignId"), campaign) {
        (None, None) => return Err(Refusal::NoCampaign),
        (None, Some(campaign)) => {
            payload.insert("entityCampaignId".into(), campaign.clone());
        }
        // A uuid in another case is the same campaign.
        (Some(own), Some(campaign)) if !same_id(own, campaign) => {
            return Err(Refusal::OtherCampaign);
        }
        (Some(_), _) => {}
    }
    if let Some(kind) = kind_of(&entity)? {
        payload.insert("type".into(), kind.clone());
    }
    for (given, field) in FIELDS {
        if let Some(value) = entity.get(given) {
            payload.insert(field.into(), value.clone());
        }
    }
    for (given, field) in MEDIA {
        if let Some(value) = entity.get(given).filter(|v| v.is_string() || v.is_null()) {
            payload.insert(field.into(), value.clone());
        }
    }
    let stats = entity.get("stats").and_then(Value::as_object);
    for (abbreviation, field) in ABILITIES {
        if let Some(score) = stats
            .and_then(|stats| stats.get(abbreviation))
            .or_else(|| entity.get(field))
        {
            payload.insert(field.into(), score.clone());
        }
    }
    match entity.get("aliases") {
        Some(aliases) if aliases.is_array() => {
            payload.insert(ALIAS.into(), Value::Array(unique(aliases)));
        }
        Some(_) => return Err(Refusal::NotAliases),
        None => {}
    }
    let merged = |payload: &Row| {
        let mut after = base.clone();
        after.extend(payload.iter().map(|(k, v)| (k.clone(), v.clone())));
        after
    };
    let after = merged(&payload);
    if !after.get("type").is_some_and(Value::is_string) {
        return Err(Refusal::NoType);
    }
    let named = after
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(|name| !name.trim().is_empty());
    if !named {
        return Err(Refusal::NoName);
    }
    for field in IDS {
        let id = after.get(field).unwrap_or(&Value::Null);
        if !id.is_null() && !id.as_str().is_some_and(is_uuid) {
            return Err(Refusal::NotAnId(field));
        }
    }
    let character = after
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| CHARACTERS.contains(&kind));
    if character {
        payload.insert("characterEntityId".into(), id.clone().into());
        let colored = after
            .get("tokenColor")
            .and_then(Value::as_str)
            .is_some_and(|color| !color.trim().is_empty());
        if !colored {
            payload.insert("tokenColor".into(), DEFAULT_TOKEN_COLOR.into());
        }
        for list in ["spells", "actions"] {
            if after.get(list).is_none_or(Value::is_null) {
                payload.insert(list.into(), Value::Array(Vec::new()));
            }
        }
    } else {
        for field in SHEET {
            if after.get(field).is_some_and(|value| !value.is_null()) {
                payload.insert(field.into(), Value::Null);
            }
        }
    }
    if !base.contains_key("entityCreatedAt") {
        payload.insert("entityCreatedAt".into(), fresh.now.into());
    }
    for (field, value) in [
        ("updatedAt", Value::from(fresh.now)),
        ("entityAliasEntityId", id.clone().into()),
        ("entityVersionEntityId", id.clone().into()),
        ("entityVersionId", fresh.version.into()),
        ("entityVersionCreatedAt", fresh.now.into()),
        ("source", SOURCE.into()),
        // A save restores nothing, whatever the last version did.
        ("restoredFromVersionId", Value::Null),
    ] {
        payload.insert(field.into(), value);
    }
    let row = merged(&payload);
    held(&row)?;
    let version = versioned(base, &row);
    stamp(&mut payload, version);
    Ok(Prepared {
        target_id: id,
        payload,
    })
}

/// Whether `row`'s values are ones the tables' casts hold: a `smallint` for
/// each score, the AC and the HP max, and nowhere the character U+0000.
fn held(row: &Row) -> Result<(), Refusal> {
    if let Some(field) = SMALLINTS
        .into_iter()
        .find(|field| !row.get(*field).is_none_or(smallint))
    {
        return Err(Refusal::NotASmallint(field));
    }
    match row.iter().find(|(_, value)| nul(value)) {
        Some((field, _)) => Err(Refusal::Nul(field.clone())),
        None => Ok(()),
    }
}

/// Whether `value` is `null`, or an integer — written with a fraction of
/// zero or not — from -32768 to 32767.
fn smallint(value: &Value) -> bool {
    value.is_null()
        || value.as_i64().map_or_else(
            || {
                value.as_f64().is_some_and(|number| {
                    number.fract() == 0.0 && (-32_768.0..=32_767.0).contains(&number)
                })
            },
            |number| i16::try_from(number).is_ok(),
        )
}

/// Whether `value` holds the character U+0000, in a string or a key.
fn nul(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains('\0'),
        Value::Array(values) => values.iter().any(nul),
        Value::Object(fields) => fields
            .iter()
            .any(|(key, value)| key.contains('\0') || nul(value)),
        _ => false,
    }
}

/// `row`'s version fields made `version`'s.
pub fn stamp(row: &mut Row, version: Version) {
    row.insert("versionNumber".into(), version.number.into());
    row.insert("snapshot".into(), Value::Object(version.snapshot));
    row.insert("diff".into(), Value::Array(version.diff));
}

/// The aliases given, as typed, without a blank one or one that names the
/// same as an earlier one (AliasMatch@1); anything but a string is dropped.
fn unique(aliases: &Value) -> Vec<Value> {
    let mut seen = Vec::new();
    let mut kept = Vec::new();
    for alias in aliases.as_array().into_iter().flatten() {
        let Some(text) = alias.as_str() else {
            continue;
        };
        let term = normalized(text);
        if !term.is_empty() && !seen.contains(&term) {
            seen.push(term);
            kept.push(alias.clone());
        }
    }
    kept
}

/// The version an Entity write adds, `before` being the element's row before
/// it and `after` the row it leaves: the number after `before`'s, the
/// snapshot of `after`, the diff from `before`'s snapshot.
#[must_use]
pub fn versioned(before: &Row, after: &Row) -> Version {
    let previous = before
        .get("snapshot")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let number = before
        .get("versionNumber")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + 1;
    let snapshot = snapshot_of(after);
    let diff = diff(&previous, &snapshot);
    Version {
        number,
        snapshot,
        diff,
    }
}

/// The snapshot of an element's row: every field of the element and of its
/// sheet it holds, and its `aliases`.
#[must_use]
pub fn snapshot_of(row: &Row) -> Map<String, Value> {
    let mut snapshot: Map<String, Value> = SNAPSHOT
        .iter()
        .filter_map(|field| row.get(*field).map(|v| ((*field).to_owned(), v.clone())))
        .collect();
    if let Some(aliases) = row.get(ALIAS) {
        snapshot.insert("aliases".into(), aliases.clone());
    }
    snapshot
}

/// The field-by-field difference from `previous` to `snapshot`
/// (EntityVersion@1): a field whose value differs, an absent field and a
/// `null` being the same, sorted by field in code point order.
#[must_use]
pub fn diff(previous: &Map<String, Value>, snapshot: &Map<String, Value>) -> Vec<Value> {
    let fields: std::collections::BTreeSet<&String> =
        previous.keys().chain(snapshot.keys()).collect();
    fields
        .into_iter()
        .filter_map(|field| {
            let old = previous.get(field).unwrap_or(&Value::Null);
            let new = snapshot.get(field).unwrap_or(&Value::Null);
            (!same(old, new)).then(|| {
                let mut change = Map::new();
                change.insert("field".into(), field.clone().into());
                change.insert("old".into(), old.clone());
                change.insert("new".into(), new.clone());
                Value::Object(change)
            })
        })
        .collect()
}

/// Two JSON values compared as JSON, numbers by value (15 and 15.0 are the
/// same number).
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            a == b || matches!((a.as_f64(), b.as_f64()), (Some(x), Some(y)) if x == y)
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| same(a, b)))
        }
        _ => a == b,
    }
}

/// A term normalised (AliasMatch@1): no white space around it, in lowercase,
/// with no accent — a combining mark U+0300 to U+036F of the decomposed
/// letter, the rest recomposed.
#[must_use]
pub fn normalized(term: &str) -> String {
    term.trim()
        .to_lowercase()
        .nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .nfc()
        .collect()
}
