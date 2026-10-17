//! The invariants of the `Entity` aggregate
//! (`crates/dataguard/aggregates/Entity/aggregate.json`), which its rules
//! write in words: the engine judges here what one element's row and its
//! command can say, on the row's state after the command. The tables
//! (`crates/data/migrations/0004_entity.sql`) judge again what reads other
//! rows: the player's campaign, the other elements' aliases
//! (`alias-unique-in-campaign`, there alone), the versions already written.
//!
//! A command writes an element as one explicit save: the element, its sheet
//! and its aliases, with the new version it adds — its number, source,
//! snapshot and diff (EntityVersion@1). The version's fields are judged on
//! the payload alone: the row keeps the last version's, which a new save does
//! not inherit.

use std::collections::BTreeMap;

use harness_dataguard::command::{Effect, Entry, Mode};
use harness_dataguard::guard::Invariants;
use serde_json::Value;

const AGGREGATE: &str = "Entity";

/// The types of element that have a character sheet.
const CHARACTERS: [&str; 2] = ["pj", "pnj"];

/// The fields of the character sheet: a row holding one has a sheet.
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

/// The fields of the version a save adds.
const VERSION: [&str; 8] = [
    "entityVersionId",
    "entityVersionEntityId",
    "versionNumber",
    "source",
    "snapshot",
    "diff",
    "restoredFromVersionId",
    "entityVersionCreatedAt",
];

/// What names the element, or stamps the save: written by any source.
const BOOKKEEPING: [&str; 5] = [
    "entityId",
    "characterEntityId",
    "entityVersionEntityId",
    "entityAliasEntityId",
    "updatedAt",
];

/// Where a change comes from (EntityVersion@1).
const SOURCES: [&str; 5] = ["dm", "ia", "dndbeyond", "brief", "restauration"];

/// The six ability scores.
const ABILITIES: [&str; 6] = [
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
];

/// What a D&D Beyond resync changes, besides the scores.
const RESYNC: [&str; 6] = [
    "armorClass",
    "hpMax",
    "spells",
    "actions",
    "dndbeyondId",
    "dndbeyondSyncedAt",
];

/// What an AI proposal fills, besides the scores — and the text it was built
/// from.
const PROPOSAL: [&str; 6] = [
    "background",
    "behavior",
    "armorClass",
    "hpMax",
    "actions",
    "initialPrompt",
];

/// The DEX modifier: computed from the score (AbilityModifier@1), never
/// written.
const DEX_MODIFIER: &str = "dexterityModifier";

/// One invariant: its id, and whether it holds on the state the command
/// leaves.
type Check = (&'static str, fn(&BTreeMap<String, Value>, &Entry) -> bool);

const CHECKS: [Check; 8] = [
    ("character-is-pj-or-pnj", character_is_pj_or_pnj),
    ("every-change-versioned", every_change_versioned),
    ("restore-exact", restore_exact),
    ("dndbeyond-resync-scope", dndbeyond_resync_scope),
    ("proposal-fills-sheet-fields", proposal_fills_sheet_fields),
    ("explicit-save-only", explicit_save_only),
    ("default-token", default_token),
    ("dex-modifier-derived", dex_modifier_derived),
];

/// The invariants `plant()` binds the engine with.
pub struct Entity;

impl Invariants for Entity {
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

/// `field`'s value in `row`, an absent field and a null being the same.
fn value<'a>(row: &'a BTreeMap<String, Value>, field: &str) -> &'a Value {
    row.get(field).unwrap_or(&Value::Null)
}

/// The source the command's version names.
fn source(entry: &Entry) -> Option<&str> {
    entry.payload.get("source").and_then(Value::as_str)
}

/// Whether the row holds a character sheet.
fn has_sheet(state: &BTreeMap<String, Value>) -> bool {
    SHEET.iter().any(|field| !value(state, field).is_null())
}

/// A sheet belongs to a PJ or a PNJ; only a PJ has a player. That the player
/// sits at the element's own campaign is the table's to judge: the row does
/// not hold the player's campaign.
fn character_is_pj_or_pnj(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    let kind = value(state, "type").as_str();
    (!has_sheet(state) || kind.is_some_and(|kind| CHARACTERS.contains(&kind)))
        && (value(state, "playerId").is_null() || kind == Some("pj"))
}

/// Every save adds a version: a number past the last one the engine holds, a
/// known source, the complete snapshot and the diff. A version written is
/// never written again.
///
/// Past, not next: the engine holds the row as it was when the command was
/// queued, and a save queued behind another rightly takes the number after
/// it. The table, which holds the versions written, wants the next one
/// exactly.
fn every_change_versioned(_state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    let confirmed = &entry.projection.confirmed;
    let number = value(&entry.payload, "versionNumber").as_u64();
    let last = value(confirmed, "versionNumber").as_u64().unwrap_or(0);
    let id = value(&entry.payload, "entityVersionId");
    number.is_some_and(|number| number > last)
        && source(entry).is_some_and(|source| SOURCES.contains(&source))
        && value(&entry.payload, "snapshot").is_object()
        && value(&entry.payload, "diff").is_array()
        && (id.is_null() || id != value(confirmed, "entityVersionId"))
}

/// A restoration names the version it restores, which is not itself; no
/// other source names one.
fn restore_exact(_state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    let restored = value(&entry.payload, "restoredFromVersionId");
    if source(entry) != Some("restauration") {
        return restored.is_null();
    }
    restored.as_str().is_some_and(|id| !id.is_empty())
        && restored != value(&entry.payload, "entityVersionId")
}

/// The fields the command changes from what the engine holds, beside the
/// version and the bookkeeping, outside `scope`.
fn changed_outside<'a>(entry: &'a Entry, scope: &[&str]) -> impl Iterator<Item = &'a str> {
    entry
        .payload
        .iter()
        .filter(|(field, _)| {
            let field = field.as_str();
            !VERSION.contains(&field)
                && !BOOKKEEPING.contains(&field)
                && !ABILITIES.contains(&field)
                && !scope.contains(&field)
        })
        .filter(|(field, now)| *now != value(&entry.projection.confirmed, field))
        .map(|(field, _)| field.as_str())
}

/// A D&D Beyond resync changes the scores, the AC, the HP max, the spells and
/// the actions, and nothing else: the aliases, the notes, the token and the
/// portrait stay as they are.
fn dndbeyond_resync_scope(_state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    source(entry) != Some("dndbeyond") || changed_outside(entry, &RESYNC).next().is_none()
}

/// An AI proposal fills the background, the behaviour, the scores, the AC,
/// the HP max and the actions, and nothing else, whatever the provider.
fn proposal_fills_sheet_fields(_state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    source(entry) != Some("ia") || changed_outside(entry, &PROPOSAL).next().is_none()
}

/// The sheet is written by an explicit save, someone's: never an increment
/// composed with what is ahead, which nobody saved as it stands.
fn explicit_save_only(_state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    !entry.by.trim().is_empty() && entry.mode != Mode::Relative
}

/// A token falls back on a disc of the sheet's colour, so a sheet has one; a
/// crop is a crop of the portrait, so a cropped token has a portrait.
fn default_token(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    (!has_sheet(state)
        || value(state, "tokenColor")
            .as_str()
            .is_some_and(|color| !color.trim().is_empty()))
        && (value(state, "tokenCrop").is_null() || !value(state, "portraitMediaId").is_null())
}

/// The DEX modifier is never written, by the payload or by a touch: whoever
/// reads the sheet computes it from the score.
fn dex_modifier_derived(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    !state.contains_key(DEX_MODIFIER)
        && !entry
            .touches
            .iter()
            .any(|touch| touch.rsplit('.').next() == Some(DEX_MODIFIER))
}

#[cfg(test)]
mod tests {
    use super::{ABILITIES, BOOKKEEPING, CHECKS, DEX_MODIFIER, PROPOSAL, RESYNC, SHEET, VERSION};

    fn aggregate() -> serde_json::Value {
        serde_json::from_str(include_str!("../../aggregates/Entity/aggregate.json"))
            .expect("contract I")
    }

    #[test]
    fn every_field_the_judge_names_is_one_the_aggregate_declares() {
        let aggregate = aggregate();
        let fields = aggregate["fields"].as_object().expect("fields");
        for field in SHEET
            .iter()
            .chain(&VERSION)
            .chain(&BOOKKEEPING)
            .chain(&ABILITIES)
            .chain(&RESYNC)
            .chain(&PROPOSAL)
            .chain(&[DEX_MODIFIER])
        {
            assert!(fields.contains_key(*field), "{field} is not declared");
        }
        assert_eq!(fields[DEX_MODIFIER]["kind"], "derived");
    }

    #[test]
    fn the_judge_checks_the_invariants_the_aggregate_declares_but_alias_unique_in_campaign() {
        let aggregate = aggregate();
        let mut declared: Vec<&str> = aggregate["invariants"]
            .as_array()
            .expect("invariants")
            .iter()
            .map(|invariant| invariant["id"].as_str().expect("an id"))
            .filter(|id| *id != "alias-unique-in-campaign")
            .collect();
        let mut judged: Vec<&str> = CHECKS.iter().map(|(id, _)| *id).collect();
        declared.sort_unstable();
        judged.sort_unstable();
        assert_eq!(judged, declared);
    }
}
