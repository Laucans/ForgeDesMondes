//! DataCapability `campaign.createEntitiesFromNote` (`data-capability.json`):
//! the DM validates, in one click, the proposals an analysis of their notes
//! made — the characters, places and events they keep become elements of
//! the campaign.
//!
//! The engine writes one partition per command. A validation is the
//! proposals' commands, on their `NoteImport/<proposal id>` partitions,
//! which the NoteImport aggregate judges (`no-creation-before-validation`),
//! and the elements it creates, each on its `Entity/<element id>` partition,
//! which the Entity aggregate judges and its store projects into `entity`,
//! `character` and `entity_version`. What the DM sends is `{ campaign,
//! accepted }`; [`Request::batch`] turns it into those commands, against the
//! proposals of the notes the accepted ones are of, as the engine holds
//! them:
//!
//! - an accepted proposal is checked, and becomes an element of its type,
//!   its name and its excerpt for description — unless it already created
//!   one (`createdEntityId`), or recognises an element the campaign still
//!   holds: no element is made twice. The proposal records the element it
//!   became;
//! - a PC or an NPC has its sheet, with a token colour to fall back on
//!   (`default-token`); a place or an event has none
//!   (`character-is-pj-or-pnj`);
//! - every element made starts with its first version (EntityVersion@1,
//!   source `dm`: the DM's validation is their explicit save): number 1, its
//!   complete snapshot, and the diff from an empty one;
//! - every other proposal of those notes is unchecked, unless it created its
//!   element already, which it never forgets;
//! - a validation naming no campaign or no proposal is refused before
//!   anything is queued ([`Refusal`]), as is one naming a proposal of no
//!   note of its campaign.

use std::collections::{BTreeMap, BTreeSet};

use harness_dataguard::plugins::Plugin;
use serde_json::{Map, Value};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "campaign.createEntitiesFromNote";

/// Where the first version of an element the DM validated comes from
/// (EntityVersion@1).
pub const SOURCE: &str = "dm";

/// The colour a new PC's or NPC's token falls back on (`default-token`).
pub const DEFAULT_TOKEN_COLOR: &str = "accent";

/// The types of element that have a character sheet.
const CHARACTERS: [&str; 2] = ["pj", "pnj"];

/// The types a proposal may have.
const TYPES: [&str; 4] = ["pj", "pnj", "lieu", "evenement"];

/// The proposal's field naming its note.
const NOTE: &str = "noteId";

/// The proposal's field naming the element it created.
const CREATED: &str = "createdEntityId";

/// The proposal's field telling whether the DM kept it.
const CHECKED: &str = "checked";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Why a validation is refused before anything is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The validation names no campaign.
    NoCampaign,
    /// `accepted` is no list of proposal ids, or an empty one.
    NothingAccepted,
    /// An accepted id names no proposal of a note of the campaign.
    Unknown(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCampaign => f.write_str("une validation nomme sa campagne"),
            Self::NothingAccepted => f.write_str("une validation nomme les propositions acceptées"),
            Self::Unknown(id) => write!(f, "aucune proposition {id} dans cette campagne"),
        }
    }
}

/// What the DM validated: the campaign, and the proposals kept, each once,
/// in lowercase, in the order given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The campaign the notes were imported into.
    pub campaign: String,
    /// The proposals the DM kept.
    pub accepted: Vec<String>,
}

/// A proposal as the engine holds it, with what the server knows of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    /// Its id: the partition `NoteImport/<id>`.
    pub id: String,
    /// Its row.
    pub row: Row,
    /// The campaign its note is of, when the engine holds its note.
    pub campaign: Option<String>,
    /// Whether the element it recognises (`existingEntityId`) is one the
    /// campaign still holds.
    pub recognised: bool,
}

/// The ids an element made is given: its own, and its first version's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fresh {
    /// The element's id.
    pub entity: String,
    /// Its first version's id.
    pub version: String,
}

impl Fresh {
    /// The ids the element `proposal` becomes is given: the same ones every
    /// time, so a validation retried after a failure half-way finds the
    /// element it made and does not make it twice.
    #[must_use]
    pub fn of(proposal: &str) -> Self {
        Self {
            entity: derived_uuid("entity", proposal),
            version: derived_uuid("version", proposal),
        }
    }
}

/// A uuid (version 5 layout) derived from `what` and the proposal's id.
fn derived_uuid(what: &str, proposal: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::new()
        .chain_update(DATA_CAPABILITY)
        .chain_update([0])
        .chain_update(what)
        .chain_update([0])
        .chain_update(proposal)
        .finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    // The version (5) and the variant (10), on the bits cleared for them.
    bytes[6] = 0x50 + (bytes[6] & 0x0f);
    bytes[8] = 0x80 + (bytes[8] & 0x3f);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// One command: the partition's id and what it writes there.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    /// `NoteImport/<target_id>` for a proposal, `Entity/<target_id>` for an
    /// element.
    pub target_id: String,
    /// The fields it writes, by the aggregate's names.
    pub payload: Row,
}

/// An element a validation makes, and the proposal it is made of.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// The proposal it is made of.
    pub proposal: String,
    /// Its command, on its Entity partition.
    pub command: Command,
}

/// What a validation submits.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    /// The elements made, in the order accepted.
    pub elements: Vec<Element>,
    /// The proposals checked or unchecked, by id: the last is the one
    /// applied last, its result the validation's.
    pub stamps: Vec<Command>,
}

/// The version an element's first save adds (EntityVersion@1).
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    /// 1, then the previous number plus 1.
    pub number: u64,
    /// The element after the save, complete.
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

/// What the DataCapability plugs into the engine: nothing yet — the legacy
/// server (`crates/dataguard/server`) still prepares it, until its
/// `Prepare` moves here.
#[must_use]
pub fn plugin() -> Plugin {
    Default::default()
}

/// A string that holds more than blanks.
fn filled(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.trim().is_empty())
}

/// What the DM's validation `wire` asks.
///
/// # Errors
///
/// [`Refusal::NoCampaign`] when it names no campaign,
/// [`Refusal::NothingAccepted`] when `accepted` is no list of ids or an empty
/// one.
pub fn request(wire: &Row) -> Result<Request, Refusal> {
    let campaign = wire
        .get("campaign")
        .and_then(filled)
        .ok_or(Refusal::NoCampaign)?;
    let given = wire
        .get("accepted")
        .and_then(Value::as_array)
        .filter(|given| !given.is_empty())
        .ok_or(Refusal::NothingAccepted)?;
    let mut accepted: Vec<String> = Vec::new();
    for id in given {
        // A proposal's id is a uuid, written in lowercase.
        let id = filled(id)
            .ok_or(Refusal::NothingAccepted)?
            .to_ascii_lowercase();
        if !accepted.contains(&id) {
            accepted.push(id);
        }
    }
    Ok(Request {
        campaign: campaign.to_owned(),
        accepted,
    })
}

/// Whether `a` and `b` name the same row: the same id, whatever its case.
fn same_id(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// `field`'s value in `row`, an absent field and a null being the same.
fn value<'a>(row: &'a Row, field: &str) -> &'a Value {
    row.get(field).unwrap_or(&Value::Null)
}

impl Request {
    /// Whether `proposal` is one of a note of this campaign, of a type an
    /// element has and with a name.
    fn owns(&self, proposal: &Proposal) -> bool {
        proposal
            .campaign
            .as_deref()
            .is_some_and(|campaign| same_id(campaign, &self.campaign))
            && value(&proposal.row, NOTE).is_string()
            && value(&proposal.row, "type")
                .as_str()
                .is_some_and(|kind| TYPES.contains(&kind))
            && filled(value(&proposal.row, "name")).is_some()
    }

    /// The commands this validation submits, `proposals` being every
    /// proposal of the notes the accepted ones are of — those included — and
    /// `fresh` the ids of the element each accepted proposal may become, in
    /// the order accepted; `now` is when the elements are made, RFC 3339.
    ///
    /// # Errors
    ///
    /// [`Refusal::Unknown`] when an accepted id names no proposal of a note
    /// of this campaign.
    ///
    /// # Panics
    ///
    /// When `fresh` does not hold one pair of ids per accepted proposal.
    pub fn batch(
        &self,
        proposals: &[Proposal],
        fresh: Vec<Fresh>,
        now: &str,
    ) -> Result<Batch, Refusal> {
        assert_eq!(
            fresh.len(),
            self.accepted.len(),
            "one pair of ids per accepted proposal"
        );
        let mut elements = Vec::new();
        let mut stamps = BTreeMap::new();
        for (id, fresh) in self.accepted.iter().zip(fresh) {
            let proposal = proposals
                .iter()
                .find(|proposal| &proposal.id == id && self.owns(proposal))
                .ok_or_else(|| Refusal::Unknown(id.clone()))?;
            let mut stamp = stamp_of(proposal, true);
            if value(&proposal.row, CREATED).is_null() && !proposal.recognised {
                // Of the campaign its note is of, as the engine writes it.
                let campaign = proposal.campaign.as_deref().unwrap_or(&self.campaign);
                let command = element(proposal, campaign, &fresh, now);
                stamp.insert(CREATED.into(), fresh.entity.into());
                elements.push(Element {
                    proposal: id.clone(),
                    command,
                });
            }
            stamps.insert(id.clone(), stamp);
        }
        let accepted: BTreeSet<&String> = self.accepted.iter().collect();
        for proposal in proposals {
            let unchecked = !accepted.contains(&proposal.id)
                && self.owns(proposal)
                && value(&proposal.row, CREATED).is_null()
                && value(&proposal.row, CHECKED) != &Value::Bool(false);
            if unchecked {
                stamps.insert(proposal.id.clone(), stamp_of(proposal, false));
            }
        }
        Ok(Batch {
            elements,
            stamps: stamps
                .into_iter()
                .map(|(target_id, payload)| Command { target_id, payload })
                .collect(),
        })
    }
}

/// A proposal's command checking it, or unchecking it: the note it is of,
/// which its store projects it on, unchanged.
fn stamp_of(proposal: &Proposal, checked: bool) -> Row {
    Row::from([
        (NOTE.to_owned(), value(&proposal.row, NOTE).clone()),
        (CHECKED.to_owned(), checked.into()),
    ])
}

/// The element `proposal` becomes in `campaign`, with its first version.
fn element(proposal: &Proposal, campaign: &str, fresh: &Fresh, now: &str) -> Command {
    let kind = value(&proposal.row, "type").clone();
    let mut payload = Row::from([
        ("entityId".to_owned(), fresh.entity.clone().into()),
        ("entityCampaignId".to_owned(), campaign.into()),
        ("type".to_owned(), kind.clone()),
        ("name".to_owned(), value(&proposal.row, "name").clone()),
        ("entityCreatedAt".to_owned(), now.into()),
        ("updatedAt".to_owned(), now.into()),
    ]);
    if let Some(excerpt) = filled(value(&proposal.row, "excerpt")) {
        payload.insert("description".into(), excerpt.trim().into());
    }
    if kind.as_str().is_some_and(|kind| CHARACTERS.contains(&kind)) {
        for (field, value) in [
            ("characterEntityId", Value::from(fresh.entity.clone())),
            ("tokenColor", DEFAULT_TOKEN_COLOR.into()),
            ("spells", Value::Array(Vec::new())),
            ("actions", Value::Array(Vec::new())),
        ] {
            payload.insert(field.into(), value);
        }
    }
    let snapshot: Map<String, Value> = payload
        .iter()
        .filter(|(field, _)| SNAPSHOT.contains(&field.as_str()))
        .map(|(field, value)| (field.clone(), value.clone()))
        .collect();
    let version = version(None, snapshot);
    for (field, value) in [
        ("entityVersionId", Value::from(fresh.version.clone())),
        ("entityVersionEntityId", fresh.entity.clone().into()),
        ("entityVersionCreatedAt", now.into()),
        ("source", SOURCE.into()),
        ("restoredFromVersionId", Value::Null),
        ("versionNumber", version.number.into()),
        ("snapshot", Value::Object(version.snapshot)),
        ("diff", Value::Array(version.diff)),
    ] {
        payload.insert(field.into(), value);
    }
    Command {
        target_id: fresh.entity.clone(),
        payload,
    }
}

/// The fields of an element made from a proposal its snapshot holds: what
/// the DM sees and restores — never an id or a date.
const SNAPSHOT: [&str; 6] = [
    "type",
    "name",
    "description",
    "tokenColor",
    "spells",
    "actions",
];

/// The version `snapshot` is, after `previous` — its number and its
/// snapshot — or as the first one, `None` (EntityVersion@1).
#[must_use]
pub fn version(
    previous: Option<(u64, &Map<String, Value>)>,
    snapshot: Map<String, Value>,
) -> Version {
    let empty = Map::new();
    let (number, before) = previous.map_or((1, &empty), |(number, before)| (number + 1, before));
    let diff = diff(before, &snapshot);
    Version {
        number,
        snapshot,
        diff,
    }
}

/// The field-by-field difference from `previous` to `snapshot`
/// (EntityVersion@1): a field whose value differs, an absent field and a
/// `null` being the same, sorted by field in code point order.
fn diff(previous: &Map<String, Value>, snapshot: &Map<String, Value>) -> Vec<Value> {
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
