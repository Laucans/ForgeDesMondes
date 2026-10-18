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
//! - a PC or an NPC has its sheet, and a token colour to fall back on;
//! - the version is the next one (EntityVersion@1, source `dm`): its number,
//!   the complete snapshot of the element as the save leaves it, and the diff
//!   from the previous snapshot. [`versioned`] computes it from the row before
//!   and the row after: here against the row the queue will have left, and
//!   again by the store against the real one when the save is applied.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use unicode_normalization::UnicodeNormalization;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "campaign.saveEntity";

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
const FIELDS: [(&str, &str); 19] = [
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

/// The row's field holding the element's aliases, an array of strings.
pub const ALIAS: &str = "alias";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a save is given besides its wire payload: the ids it may need, made
/// by the server, and the time.
#[derive(Clone, Copy, Debug)]
pub struct Fresh<'a> {
    /// The id a new element takes; unused when the element exists.
    pub entity: &'a str,
    /// The id of the version the save adds.
    pub version: &'a str,
    /// Now, RFC 3339.
    pub now: &'a str,
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

/// The fields the editor sends: under `entity`, or the payload itself.
fn entity_of(wire: &Row) -> Map<String, Value> {
    match wire.get("entity") {
        Some(Value::Object(entity)) => entity.clone(),
        _ => wire.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
    }
}

/// The element a save names, when it names one: `entity.id`.
#[must_use]
pub fn element_id(wire: &Row) -> Option<String> {
    entity_of(wire)
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(str::to_owned)
}

/// The command a save of `wire` submits, `base` being the element's row as
/// the commands queued ahead of it will leave it (empty for a new element).
#[must_use]
pub fn prepare(wire: &Row, base: &Row, fresh: Fresh<'_>) -> Prepared {
    let entity = entity_of(wire);
    let id = element_id(wire).unwrap_or_else(|| {
        base.get("entityId")
            .and_then(Value::as_str)
            .unwrap_or(fresh.entity)
            .to_owned()
    });
    let mut payload = Row::new();
    payload.insert("entityId".into(), id.clone().into());
    if let Some(campaign) = wire.get("campaign").filter(|c| c.is_string()) {
        payload.insert("entityCampaignId".into(), campaign.clone());
    }
    let kind = entity
        .get("kind")
        .or_else(|| entity.get("type"))
        .filter(|kind| kind.as_str().is_some_and(|kind| TYPES.contains(&kind)));
    if let Some(kind) = kind {
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
    if let Some(aliases) = entity.get("aliases") {
        payload.insert(ALIAS.into(), Value::Array(unique(aliases)));
    }
    let merged = |payload: &Row| {
        let mut after = base.clone();
        after.extend(payload.iter().map(|(k, v)| (k.clone(), v.clone())));
        after
    };
    let after = merged(&payload);
    if after
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| CHARACTERS.contains(&kind))
    {
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
    let version = versioned(base, &merged(&payload));
    stamp(&mut payload, version);
    Prepared {
        target_id: id,
        payload,
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
