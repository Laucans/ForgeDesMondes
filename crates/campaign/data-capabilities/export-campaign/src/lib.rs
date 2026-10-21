//! DataCapability `campaign.exportCampaign` (`data-capability.json`): the DM
//! exports a campaign to keep a copy of it, and the home page shows when that
//! last happened. The export is an archive — the campaign, its players, its
//! elements with their sheets, their aliases and their versions, as the
//! tables hold them now — and one command on `Campaign/<its id>` stamping
//! `lastExportedAt`, which writes nothing else (`export-stamps-date`).
//!
//! The archive never leaves the product through this crate: it is answered
//! with the command's result once the stamp is applied, and the shell hands
//! it to the DM as a download. [`prepare`] turns what the tables hold of the
//! campaign — one JSON document, `{campaign, players, elements}`, each row
//! under its columns' names — into that archive and the command's payload:
//!
//! - the archive is one self-contained JSON document: its [`FORMAT`] and
//!   [`FORMAT_VERSION`], the campaign's id as `origin_campaign_id` (what an
//!   import names the copy's original by), the export's time, and the rows
//!   as the tables hold them — the campaign's own `last_exported_at` the one
//!   before this export;
//! - its file is named after the campaign and the time ([`file_name`]);
//! - the command targets the campaign by its id, lowercase as the table
//!   writes it — one partition per campaign whatever case the DM's request
//!   used — and its payload stamps `lastExportedAt` with the export's time,
//!   and nothing else;
//! - what the tables answered but cannot be an archive — no campaign, a
//!   campaign without its id, players or elements that are no list — is
//!   refused before anything is queued ([`Refusal`]): the stamp is left as it
//!   was.
//!
//! Media, sounds and sessions join the archive with their milestones; the
//! import that reads it back is a DataCapability of its own.

use std::collections::BTreeMap;

use harness_dataguard::plugins::Plugin;
use serde_json::{Map, Value};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "campaign.exportCampaign";

/// What an archive says it is.
pub const FORMAT: &str = "campaign-archive";

/// The version of its shape: an import reads the versions it knows.
pub const FORMAT_VERSION: u64 = 1;

/// The one field an export writes: when it happened.
const STAMP: &str = "lastExportedAt";

/// What a file is named after when the campaign has no name to give it.
const UNNAMED: &str = "campagne";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Why an export is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// What the tables answered holds no campaign with its id.
    NoCampaign,
    /// The campaign's players or elements are not a list: no archive holds
    /// them.
    Unreadable(&'static str),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCampaign => f.write_str("cette campagne n'existe pas"),
            Self::Unreadable(what) => {
                write!(
                    f,
                    "l'archive n'a pas pu être construite : {what} illisibles"
                )
            }
        }
    }
}

/// The archive an export answers: its file's name, and its content.
#[derive(Clone, Debug, PartialEq)]
pub struct Archive {
    /// `<the campaign's name>-<the time>.json`.
    pub file: String,
    /// The JSON document itself.
    pub content: Value,
}

/// What an export becomes before it is submitted: what the command writes
/// on `Campaign/<target_id>`, and the archive it answers once applied.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The fields written: the stamp alone.
    pub payload: Row,
    /// The campaign's id, lowercase: the command's target.
    pub target_id: String,
    /// The archive.
    pub archive: Archive,
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

/// The campaign a request names in its payload, when it names one there —
/// `id`, `campaignId` or `campaign` — rather than as the command's target.
#[must_use]
pub fn campaign_id(wire: &Row) -> Option<String> {
    ["id", "campaignId", "campaign"]
        .iter()
        .find_map(|field| wire.get(*field).and_then(Value::as_str))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

/// The export of the campaign `tables` holds — `{campaign, players,
/// elements}`, as the tables hold them — at `now`, RFC 3339.
///
/// # Errors
///
/// [`Refusal`] when `tables` holds no campaign with its id, or players or
/// elements that are not a list.
pub fn prepare(tables: &Value, now: &str) -> Result<Prepared, Refusal> {
    let campaign = tables
        .get("campaign")
        .and_then(Value::as_object)
        .ok_or(Refusal::NoCampaign)?;
    let id = campaign
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or(Refusal::NoCampaign)?
        .to_ascii_lowercase();
    let list = |field: &'static str| {
        tables
            .get(field)
            .and_then(Value::as_array)
            .cloned()
            .ok_or(Refusal::Unreadable(field))
    };
    let players = list("players")?;
    let elements = list("elements")?;
    let name = campaign.get("name").and_then(Value::as_str).unwrap_or("");
    let mut content = Map::new();
    for (field, value) in [
        ("format", Value::from(FORMAT)),
        ("version", FORMAT_VERSION.into()),
        ("origin_campaign_id", id.clone().into()),
        ("exported_at", now.into()),
        ("campaign", Value::Object(campaign.clone())),
        ("players", Value::Array(players)),
        ("elements", Value::Array(elements)),
    ] {
        content.insert(field.into(), value);
    }
    let payload = Row::from([(STAMP.into(), now.into())]);
    Ok(Prepared {
        payload,
        archive: Archive {
            file: file_name(name, now),
            content: Value::Object(content),
        },
        target_id: id,
    })
}

/// The archive's file name: the campaign's name, lowercase, every run of
/// characters that are neither letters nor digits one dash — `campagne` when
/// nothing is left — then the time to the millisecond, so two exports in a
/// second are two files, its colons and its point dashes so every file
/// system takes it: `les-cendres-de-valombre-2026-10-18T09-30-00-123Z.json`.
#[must_use]
pub fn file_name(name: &str, now: &str) -> String {
    let mut slug = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    let slug = if slug.is_empty() { UNNAMED } else { slug };
    let time = now.trim_end_matches('Z').replace([':', '.'], "-");
    format!("{slug}-{time}Z.json")
}
