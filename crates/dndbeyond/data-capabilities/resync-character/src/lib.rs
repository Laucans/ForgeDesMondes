//! DataCapability `dndbeyond.resyncCharacter` (`data-capability.json`): after
//! a level-up, the DM clicks « Resynchroniser » and a PC already linked to D&D
//! Beyond takes its sheet again — its statblock (StatBlock@1: the six scores,
//! the AC, the HP max, the actions and the spells) and the time it was
//! synchronised — on its `Entity` partition, kept as a version of source
//! `dndbeyond` (EntityVersion@1) the history shows, and undoes, like any
//! other. Its aliases, its notes, its token and its portrait stay the DM's
//! (`dndbeyond-resync-scope`): the resync never writes them.
//!
//! The PC is the one the command targets — else the one `entityId` names —,
//! read as the queue leaves it, the tables' own when the engine never wrote
//! it. Its link (`dndbeyondId`, the character's id an import wrote) is what
//! the sheet is fetched by, through the `character-sheets` port the server
//! shares ([`SharedCharacterSheets`]). A PC linked to nothing is refused with
//! a reason the DM reads; so is any failure of the port — a character no
//! longer public, unknown, a service whose answer changed shape or that did
//! not answer — and a sheet whose statblock is not complete: nothing is
//! queued, nothing written in part.
//!
//! The engine runs [`plugin`]'s `Prepare`: [`prepare`] makes the command and
//! the version it adds, numbered, snapshotted and diffed after the row's,
//! which the Entity aggregate judges, projects into its tables and settles
//! again after the element's last version.

use std::collections::BTreeMap;
use std::fmt;

use aggregate_entity::{Kept, settled};
use async_trait::async_trait;
use harness_dataguard::plugins::{self as engine, Context, Plugin};
use ports_character_sheets::{
    CharacterId, CharacterSheet, CharacterSheets, CharacterSheetsError, SharedCharacterSheets,
};
use serde_json::{Map, Value};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "dndbeyond.resyncCharacter";

/// This DataCapability's manifest (contract F), as the engine parses it.
pub const MANIFEST: &str = include_str!("../data-capability.json");

/// Where a resync comes from (EntityVersion@1).
pub const SOURCE: &str = "dndbeyond";

/// StatBlock@1's ten fields, in the order it names the unsound ones.
const STATBLOCK: [&str; 10] = [
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
    "armorClass",
    "hpMax",
    "actions",
    "spells",
];

/// The largest AC or HP max the tables hold (a `smallint`).
const SMALLINT: i64 = 32_767;

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Why a resync is refused: nothing is queued, nothing written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The `character-sheets` port's: the link is no character's, the
    /// character is no longer public or unknown, the service changed shape
    /// or did not answer.
    Sheets(CharacterSheetsError),
    /// The element the command targets does not exist.
    NoElement,
    /// The command names its element by something that is not a uuid.
    NotAnId,
    /// The element is linked to no D&D Beyond character.
    NotLinked,
    /// The statblock leaves these fields unsound (StatBlock@1), in the
    /// Concept's order.
    Incomplete(Vec<&'static str>),
    /// A value the tables could never hold: an AC or HP max beyond a
    /// `smallint`, or the character U+0000.
    Unwritable(&'static str),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sheets(e) => e.fmt(f),
            Self::NoElement => f.write_str("aucun élément de cet identifiant"),
            Self::NotAnId => f.write_str("entityId n'est pas un identifiant"),
            Self::NotLinked => f.write_str(
                "ce personnage n'est pas lié à D&D Beyond : importez-le d'abord par son adresse",
            ),
            Self::Incomplete(fields) => write!(
                f,
                "la fiche D&D Beyond n'a pas de statblock complet : {}",
                fields.join(", ")
            ),
            Self::Unwritable(field) => write!(f, "{field} ne tient pas dans la fiche"),
        }
    }
}

impl std::error::Error for Refusal {}

/// What a resync is given besides the row and the sheet: the new version's
/// id, and the time.
#[derive(Clone, Copy, Debug)]
pub struct Fresh<'a> {
    /// The id of the version the resync adds.
    pub version: &'a str,
    /// Now, RFC 3339.
    pub now: &'a str,
}

/// What the DataCapability plugs into the engine: its `Prepare`, which
/// reaches the `character-sheets` port the server shares.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .prepare(DATA_CAPABILITY, Resyncing)
        .needs::<SharedCharacterSheets>()
}

/// A resync made what it writes, before anything is queued: the PC's row as
/// the queue leaves it, its character's sheet fetched by its link, a fresh
/// version id and now. What names nothing — an element, a character — is
/// refused as not found, any other refusal as unprocessable.
struct Resyncing;

/// Why a resync is refused when the port is not shared: the engine refuses
/// to load first, so a server never answers it.
const NO_PORT: &str = "le port character-sheets n'est pas branché";

#[async_trait]
impl engine::Prepare for Resyncing {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &Row,
        target_id: Option<&str>,
    ) -> Result<engine::Prepared, engine::Refusal> {
        let Some(sheets) = context.shared.get::<SharedCharacterSheets>() else {
            return Err(engine::Refusal::unprocessable(NO_PORT));
        };
        // The engine names the target from the payload's `entityId` too.
        let id = target_id
            .or_else(|| payload.get("entityId").and_then(Value::as_str))
            .map(str::trim)
            .unwrap_or_default();
        if !is_uuid(id) {
            return Err(refused(Refusal::NotAnId));
        }
        let base = context.row(&format!("Entity/{id}")).await?;
        let character = linked(&base).map_err(refused)?;
        let sheet = sheets
            .fetch(character)
            .await
            .map_err(|e| refused(Refusal::Sheets(e)))?;
        let version = context.ids.fresh();
        let now = aggregate_entity::timestamp(context.now_millis);
        let fresh = Fresh {
            version: &version,
            now: &now,
        };
        Ok(engine::Prepared {
            payload: prepare(&base, &sheet, fresh).map_err(refused)?,
            // The payload it writes does not name the element: the
            // partition is the one read.
            target_id: Some(id.to_owned()),
            ..engine::Prepared::default()
        })
    }
}

/// The engine's refusal of `refusal`: what names nothing — an element, a
/// character — is not found, anything else unprocessable.
fn refused(refusal: Refusal) -> engine::Refusal {
    match refusal {
        Refusal::NoElement | Refusal::Sheets(CharacterSheetsError::NotFound) => {
            engine::Refusal::not_found(refusal.to_string())
        }
        _ => engine::Refusal::unprocessable(refusal.to_string()),
    }
}

/// The D&D Beyond character the element whose row is `base` is linked to:
/// the id an import wrote, or the URL a DM pasted.
///
/// # Errors
///
/// [`Refusal::NoElement`] for an empty row, [`Refusal::NotLinked`] for an
/// element linked to nothing, [`Refusal::Sheets`] for a link that names no
/// D&D Beyond character.
pub fn linked(base: &Row) -> Result<CharacterId, Refusal> {
    if base.is_empty() {
        return Err(Refusal::NoElement);
    }
    let link = base
        .get("dndbeyondId")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if link.is_empty() {
        return Err(Refusal::NotLinked);
    }
    CharacterId::parse(link).map_err(Refusal::Sheets)
}

/// Whether `text` is a uuid as PostgreSQL reads one: 8-4-4-4-12 hex digits.
fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// The statblock of `sheet` under the aggregate's names, once StatBlock@1
/// holds it complete and the tables can hold it.
///
/// # Errors
///
/// [`Refusal::Incomplete`] naming every unsound field, [`Refusal::Unwritable`]
/// for an AC or HP max beyond a `smallint`, or an action or a spell holding
/// the character U+0000.
pub fn statblock_of(sheet: &CharacterSheet) -> Result<Row, Refusal> {
    let Ok(Value::Object(fields)) = serde_json::to_value(&sheet.statblock) else {
        return Err(Refusal::Incomplete(STATBLOCK.to_vec()));
    };
    let incomplete = statblock(&fields);
    if !incomplete.is_empty() {
        return Err(Refusal::Incomplete(incomplete));
    }
    for field in ["armorClass", "hpMax"] {
        // StatBlock@1 holds it a positive integer already.
        if fields[field].as_i64().is_some_and(|n| n > SMALLINT) {
            return Err(Refusal::Unwritable(field));
        }
    }
    if ["actions", "spells"].iter().any(|list| nul(&fields[*list])) {
        return Err(Refusal::Unwritable("actions ou spells"));
    }
    Ok(fields.into_iter().collect())
}

/// StatBlock@1: the fields of `fields` a complete statblock lacks — absent,
/// null, of another type or out of range — once each, in the Concept's
/// order; empty when it is complete. A field other than the ten is ignored.
#[must_use]
pub fn statblock(fields: &Map<String, Value>) -> Vec<&'static str> {
    STATBLOCK
        .into_iter()
        .filter(|field| {
            let value = fields.get(*field).unwrap_or(&Value::Null);
            !match *field {
                "armorClass" | "hpMax" => value.as_i64().is_some_and(|n| n > 0),
                "actions" => entries(value).is_some_and(|n| n > 0),
                "spells" => entries(value).is_some(),
                _ => value.as_i64().is_some_and(|n| (1..=30).contains(&n)),
            }
        })
        .collect()
}

/// How many entries `value` holds when it is a list of sound
/// `{name, description}`: both strings, neither blank once trimmed.
fn entries(value: &Value) -> Option<usize> {
    let sound = |entry: &Value| {
        ["name", "description"].iter().all(|key| {
            entry
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty())
        })
    };
    let list = value.as_array()?;
    list.iter().all(sound).then_some(list.len())
}

/// Whether `value` holds the character U+0000, which neither `text` nor
/// `jsonb` can hold.
fn nul(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains('\0'),
        Value::Array(values) => values.iter().any(nul),
        Value::Object(fields) => fields.iter().any(|(k, v)| k.contains('\0') || nul(v)),
        _ => false,
    }
}

/// The command a resync of `sheet` submits on the PC whose row is `base`, as
/// the commands queued ahead of it will leave it: the statblock and the sync
/// date — nothing the DM set locally —, then the version it adds after the
/// row's, of source `dndbeyond`.
///
/// # Errors
///
/// [`Refusal`] for a row a resync cannot write — no element, one linked to
/// nothing —, a sheet of another character than the one the PC is linked to,
/// a statblock StatBlock@1 does not hold complete, a value the tables could
/// never hold.
pub fn prepare(base: &Row, sheet: &CharacterSheet, fresh: Fresh<'_>) -> Result<Row, Refusal> {
    // The port answers the character it was asked for: a sheet of another
    // would write a stranger's statblock on this PC.
    let character = linked(base)?;
    if sheet.id != character {
        let answered = format!("le personnage {} répondu pour {character}", sheet.id);
        return Err(Refusal::Sheets(CharacterSheetsError::ServiceChanged(
            answered,
        )));
    }
    let mut payload = statblock_of(sheet)?;
    for (field, value) in [
        ("dndbeyondSyncedAt", Value::from(fresh.now)),
        ("updatedAt", fresh.now.into()),
        (
            "entityVersionEntityId",
            base.get("entityId").cloned().unwrap_or(Value::Null),
        ),
        ("entityVersionId", fresh.version.into()),
        ("entityVersionCreatedAt", fresh.now.into()),
        ("source", SOURCE.into()),
        // A resync restores nothing, whatever the last version did.
        ("restoredFromVersionId", Value::Null),
    ] {
        payload.insert(field.into(), value);
    }
    let mut after = base.clone();
    after.extend(payload.iter().map(|(k, v)| (k.clone(), v.clone())));
    Ok(settled(last_version(base).as_ref(), &after)
        .expect("the row after carries the new version's id")
        .stamped(&payload))
}

/// The version `base` carries: the element's last, as the queue leaves it —
/// an empty snapshot when it holds none.
fn last_version(base: &Row) -> Option<Kept> {
    Some(Kept {
        id: base
            .get("entityVersionId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        number: base.get("versionNumber")?.as_u64()?,
        snapshot: base
            .get("snapshot")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
        diff: Vec::new(),
    })
}
