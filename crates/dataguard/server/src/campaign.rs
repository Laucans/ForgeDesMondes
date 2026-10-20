//! The invariants of the `Campaign` aggregate
//! (`crates/dataguard/aggregates/Campaign/aggregate.json`), which its rules
//! write in words: the engine judges them here, on the row's state after the
//! command. The `campaign` table's CHECKs (`crates/data/migrations/`) judge
//! the kind and the master volume there again.
//!
//! What a campaign's own row can say is judged here; the rest waits for the
//! tables it needs. `oneshot-single-session` counts sessions, which the
//! Session aggregate keeps: here, a campaign is a oneshot or a campagne and
//! nothing else, so that count has a kind to be judged against.
//! `import-creates-copy` gives the archive's other rows new ids too, which
//! the import's DataCapability does: here, an import is a new row, never one
//! that exists — the original's least of all.

use std::collections::BTreeMap;

use harness_dataguard::command::{Effect, Entry};
use harness_dataguard::guard::Invariants;
use serde_json::Value;

const AGGREGATE: &str = "Campaign";

/// The kinds of campaign, as the table types them.
const KINDS: [&str; 2] = ["campagne", "oneshot"];

/// The DataCapability behind an export to an archive.
const EXPORT_CAMPAIGN: &str = "campaign.exportCampaign";

/// What an export may write: the date it stamps.
const EXPORT_STAMP: &str = "lastExportedAt";

/// The fields naming the campaign itself, which a command may carry to say
/// which row it writes.
const IDENTIFIERS: [&str; 2] = ["id", "campaignId"];

/// What only an import writes: where the copy comes from, and when.
const IMPORT: [&str; 2] = ["originCampaignId", "importedAt"];

/// One invariant: its id, and whether it holds on the state the command
/// leaves.
type Check = (&'static str, fn(&BTreeMap<String, Value>, &Entry) -> bool);

const CHECKS: [Check; 4] = [
    ("oneshot-single-session", oneshot_single_session),
    ("import-creates-copy", import_creates_copy),
    ("export-stamps-date", export_stamps_date),
    ("master-volume-range", master_volume_range),
];

/// The invariants `plant()` binds the engine with.
pub struct Campaign;

impl Invariants for Campaign {
    fn violated(
        &self,
        aggregate: &str,
        state: &BTreeMap<String, Value>,
        entry: &Entry,
    ) -> Vec<String> {
        // A deletion is the engine's tombstone: nothing of the row is left to
        // judge.
        if aggregate != AGGREGATE || entry.effect == Effect::Delete {
            return Vec::new();
        }
        CHECKS
            .iter()
            .filter(|(_, holds)| !holds(state, entry))
            .map(|(id, _)| (*id).to_string())
            .collect()
    }
}

/// The fields the command writes: its `touches`, and its payload's keys.
fn written(entry: &Entry) -> impl Iterator<Item = String> + '_ {
    entry
        .fields()
        .into_iter()
        .chain(entry.payload.keys().cloned())
}

/// Whether the campaign was there before the command: the engine held its
/// row when the command was queued, or holds a field the command does not
/// write.
fn existed(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    !entry.projection.confirmed.is_empty()
        || state.keys().any(|field| !entry.payload.contains_key(field))
}

/// Whether `id` names the campaign the command writes, whatever its case.
fn names_target(id: &Value, entry: &Entry) -> bool {
    id.as_str()
        .is_some_and(|id| id.eq_ignore_ascii_case(entry.target_id()))
}

/// Every campaign is a campagne or a oneshot: a command that leaves a row of
/// another kind, or of none — created without it, or written to a campaign
/// the engine does not hold — is refused.
fn oneshot_single_session(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    state
        .get("kind")
        .and_then(Value::as_str)
        .is_some_and(|kind| KINDS.contains(&kind))
}

/// An import is a new row, under an id that is not its original's: no
/// campaign that exists is written, the original least of all. A command
/// that declares it touches where a copy comes from, or changes it, is an
/// import, and so is a creation that carries it; an update that carries it
/// unchanged is not.
fn import_creates_copy(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    let declared = !entry.touches.is_empty()
        && entry
            .fields()
            .iter()
            .any(|field| IMPORT.contains(&field.as_str()));
    let changed = IMPORT.iter().any(|field| {
        entry.payload.get(*field).is_some_and(|value| {
            entry.effect == Effect::Insert || entry.projection.confirmed.get(*field) != Some(value)
        })
    });
    if !declared && !changed {
        return true;
    }
    entry.effect == Effect::Insert
        && !existed(state, entry)
        && state
            .get("originCampaignId")
            .is_none_or(|origin| !names_target(origin, entry))
}

/// An export updates the campaign it names, which exists, stamps
/// `lastExportedAt`, and writes nothing else.
fn export_stamps_date(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    if entry.data_capability.split('@').next() != Some(EXPORT_CAMPAIGN) {
        return true;
    }
    let names_itself = |field: &str| {
        IDENTIFIERS.contains(&field)
            && entry
                .payload
                .get(field)
                .is_none_or(|id| names_target(id, entry))
    };
    entry.effect == Effect::Update
        && existed(state, entry)
        && entry
            .payload
            .get(EXPORT_STAMP)
            .is_some_and(|stamp| stamp.as_str().is_some_and(|stamp| !stamp.is_empty()))
        && written(entry).all(|field| field == EXPORT_STAMP || names_itself(&field))
}

/// The master volume, when the row has one, is a number from 0 to 1 — a
/// JSON number, or the text of one as PostgreSQL's `numeric` reads.
fn master_volume_range(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    let volume = match state.get("soundMasterVolume") {
        None => return true,
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        Some(_) => None,
    };
    volume.is_some_and(|v| (0.0..=1.0).contains(&v))
}
