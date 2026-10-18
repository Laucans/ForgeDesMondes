//! DataCapability `campaign.restoreEntityVersion` (`data-capability.json`):
//! the DM takes an element back to one of its versions in one action — the
//! element, its sheet and its aliases rewritten exactly as that version's
//! snapshot holds them (`restore-exact`) — or undoes only some changes of the
//! version after it, each field back to what that version held. Either way
//! the restoration is a version of its own, of source `restauration`, naming
//! the version it restores: nothing is lost, and it can be undone in turn.
//!
//! The engine writes one partition per command: a restoration is one command
//! on `Entity/<the element's id>`, carrying every field of the element and of
//! its sheet a snapshot holds, its aliases and the version it adds, which the
//! DataGuard's invariants judge and its store projects into `entity`,
//! `character`, `entity_alias` and `entity_version`. [`prepare`] turns the
//! DM's request — a version, by its number or its id, and the changes to undo
//! — into that state, against the element's row as the queue will leave it
//! and the versions it has ([`Kept`]):
//!
//! - a field the snapshot does not hold is written `null`: an absent field
//!   and a `null` are the same (EntityVersion@1), and what the element gained
//!   since is gone with the rest;
//! - an element restored as a PC or an NPC has its sheet, one restored as a
//!   place or an event has none;
//! - whether an alias restored now names another element of the campaign is
//!   the DataGuard's to judge (`alias-unique-in-campaign`): it refuses the
//!   command rather than write it;
//! - a version the element does not have — another element's among them, or
//!   any of an element the engine does not hold — is refused before it is
//!   queued ([`Refusal`]);
//! - the version is the next one (EntityVersion@1): its number, the snapshot
//!   of the element as the restoration leaves it, and the diff from the
//!   previous snapshot. The store settles it again when it is applied.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "campaign.restoreEntityVersion";

/// Where a restoration comes from (EntityVersion@1).
pub const SOURCE: &str = "restauration";

/// The row's field holding the element's aliases, an array of strings; a
/// snapshot holds them under `aliases`.
const ALIAS: &str = "alias";

/// A snapshot's field holding the aliases.
const ALIASES: &str = "aliases";

/// The types of element that have a character sheet.
const CHARACTERS: [&str; 2] = ["pj", "pnj"];

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

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// One version of the element, as it was written: never changed since.
#[derive(Clone, Debug, PartialEq)]
pub struct Kept {
    /// Its id.
    pub id: String,
    /// Its number in the element's history, from 1.
    pub number: u64,
    /// The element, its sheet and its aliases as it left them.
    pub snapshot: Map<String, Value>,
    /// What it changed from the version before, `{field, old, new}`.
    pub diff: Vec<Value>,
}

impl Kept {
    /// The version an element's row carries — the last one written — when it
    /// carries one.
    #[must_use]
    pub fn of(row: &Row) -> Option<Self> {
        Some(Self {
            id: row.get("entityVersionId")?.as_str()?.to_owned(),
            number: row.get("versionNumber")?.as_u64()?,
            snapshot: row.get("snapshot")?.as_object()?.clone(),
            diff: row
                .get("diff")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        })
    }
}

/// What a restoration is given besides the DM's request: the new version's
/// id, and the time.
#[derive(Clone, Copy, Debug)]
pub struct Fresh<'a> {
    /// The id of the version the restoration adds.
    pub version: &'a str,
    /// Now, RFC 3339.
    pub now: &'a str,
}

/// Why a restoration is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The request names no element.
    NoElement,
    /// The request names no version: neither a number nor an id.
    NoVersion,
    /// The element has no such version: it is another element's, or none —
    /// an element the engine does not hold has none.
    UnknownVersion,
    /// The changes to undo are not a non-empty list of positions.
    NotChanges,
    /// The version after the one named has no change at this position — or
    /// there is no version after it.
    UnknownChange(u64),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoElement => f.write_str("une restauration nomme sa fiche"),
            Self::NoVersion => f.write_str("une restauration nomme la version restaurée"),
            Self::UnknownVersion => f.write_str("cette version n'existe pas pour cette fiche"),
            Self::NotChanges => {
                f.write_str("les changements annulés sont une liste non vide de positions")
            }
            Self::UnknownChange(at) => {
                write!(f, "la version suivante n'a pas de changement {at}")
            }
        }
    }
}

/// What a restoration becomes before it is submitted: what the command
/// writes on `Entity/<the element's id>`, its version included.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The fields written.
    pub payload: Row,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The element a request names in its payload, when it names one there —
/// `entity`, or `id` — rather than as the command's target.
#[must_use]
pub fn element_id(wire: &Row) -> Option<String> {
    ["entity", "id"]
        .iter()
        .find_map(|field| wire.get(*field).and_then(Value::as_str))
        .filter(|id| !id.trim().is_empty())
        .map(str::to_owned)
}

/// The version `wire` names among `history`: by its number, or by its id in
/// whatever case.
fn named<'a>(wire: &Row, history: &'a [Kept]) -> Result<&'a Kept, Refusal> {
    let found = match wire.get("version") {
        Some(Value::Number(number)) => {
            let number = number.as_u64().ok_or(Refusal::UnknownVersion)?;
            history.iter().find(|kept| kept.number == number)
        }
        Some(Value::String(id)) if !id.trim().is_empty() => history
            .iter()
            .find(|kept| kept.id.eq_ignore_ascii_case(id.trim())),
        _ => return Err(Refusal::NoVersion),
    };
    found.ok_or(Refusal::UnknownVersion)
}

/// The positions `wire` undoes, when it undoes some: `None` restores the
/// version whole.
fn positions(wire: &Row) -> Result<Option<Vec<u64>>, Refusal> {
    match wire.get("changes") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(changes)) if !changes.is_empty() => changes
            .iter()
            .map(|at| at.as_u64().ok_or(Refusal::NotChanges))
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => Err(Refusal::NotChanges),
    }
}

/// The snapshot the restoration leaves: `target`'s whole, or — undoing the
/// changes at `undone` of the version after it — the element as it stands,
/// each field those changes name back to `target`'s value.
fn restored(
    base: &Row,
    history: &[Kept],
    target: &Kept,
    undone: Option<&[u64]>,
) -> Result<Map<String, Value>, Refusal> {
    let Some(undone) = undone else {
        return Ok(target.snapshot.clone());
    };
    let next = history.iter().find(|kept| kept.number == target.number + 1);
    let mut snapshot = snapshot_of(base);
    for &at in undone {
        let field = next
            .and_then(|next| usize::try_from(at).ok().and_then(|i| next.diff.get(i)))
            .and_then(|change| change.get("field"))
            .and_then(Value::as_str)
            .ok_or(Refusal::UnknownChange(at))?;
        let value = target.snapshot.get(field).cloned().unwrap_or(Value::Null);
        snapshot.insert(field.to_owned(), value);
    }
    Ok(snapshot)
}

/// The command restoring the element `id` as `wire` asks — `base` being its
/// row as the commands queued ahead of it will leave it, `history` the
/// versions it has.
///
/// # Errors
///
/// [`Refusal`] for a request naming no element or no version, a version the
/// element does not have, or a change its next version does not hold.
pub fn prepare(
    wire: &Row,
    base: &Row,
    history: &[Kept],
    id: &str,
    fresh: Fresh<'_>,
) -> Result<Prepared, Refusal> {
    if id.trim().is_empty() {
        return Err(Refusal::NoElement);
    }
    let target = named(wire, history)?;
    // The row is the element's own, live: under the id the engine holds it by.
    if base.get("entityId").and_then(Value::as_str) != Some(id) {
        return Err(Refusal::UnknownVersion);
    }
    let undone = positions(wire)?;
    let snapshot = restored(base, history, target, undone.as_deref())?;
    let mut payload: Row = SNAPSHOT
        .iter()
        .map(|field| {
            let value = snapshot.get(*field).cloned().unwrap_or(Value::Null);
            ((*field).to_owned(), value)
        })
        .collect();
    let aliases = snapshot
        .get(ALIASES)
        .filter(|aliases| aliases.is_array())
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    payload.insert(ALIAS.into(), aliases);
    let character = snapshot
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| CHARACTERS.contains(&kind));
    let sheet = if character {
        Value::from(id)
    } else {
        Value::Null
    };
    for (field, value) in [
        ("entityId", Value::from(id)),
        ("characterEntityId", sheet),
        ("updatedAt", fresh.now.into()),
        ("entityAliasEntityId", id.into()),
        ("entityVersionEntityId", id.into()),
        ("entityVersionId", fresh.version.into()),
        ("entityVersionCreatedAt", fresh.now.into()),
        ("source", SOURCE.into()),
        ("restoredFromVersionId", target.id.clone().into()),
    ] {
        payload.insert(field.into(), value);
    }
    let mut after = base.clone();
    after.extend(payload.iter().map(|(k, v)| (k.clone(), v.clone())));
    let previous = base
        .get("snapshot")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let number = base
        .get("versionNumber")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + 1;
    let snapshot = snapshot_of(&after);
    payload.insert("diff".into(), Value::Array(diff(&previous, &snapshot)));
    payload.insert("versionNumber".into(), number.into());
    payload.insert("snapshot".into(), Value::Object(snapshot));
    Ok(Prepared { payload })
}

/// The snapshot of an element's row: every field of the element and of its
/// sheet it holds, and its `aliases`.
fn snapshot_of(row: &Row) -> Map<String, Value> {
    let mut snapshot: Map<String, Value> = SNAPSHOT
        .iter()
        .filter_map(|field| row.get(*field).map(|v| ((*field).to_owned(), v.clone())))
        .collect();
    if let Some(aliases) = row.get(ALIAS) {
        snapshot.insert(ALIASES.into(), aliases.clone());
    }
    snapshot
}

/// The field-by-field difference from `previous` to `snapshot`
/// (EntityVersion@1): a field whose value differs, an absent field and a
/// `null` being the same, sorted by field in code point order.
#[must_use]
pub fn diff(previous: &Map<String, Value>, snapshot: &Map<String, Value>) -> Vec<Value> {
    let fields: BTreeSet<&String> = previous.keys().chain(snapshot.keys()).collect();
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
