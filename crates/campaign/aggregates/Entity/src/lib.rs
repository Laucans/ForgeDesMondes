//! The invariants of the `Entity` aggregate
//! (`crates/campaign/aggregates/Entity/aggregate.json`), which its rules
//! write in words: the engine judges here what one element's row and its
//! command can say, on the row's state after the command — and, for
//! `alias-unique-in-campaign`, against the other elements of its campaign
//! the engine has applied ([`Aliases`]). The tables
//! (`crates/data/migrations/0004_entity.sql`) judge again what reads other
//! rows: the player's campaign, the other elements' aliases, the versions
//! already written.
//!
//! A command writes an element as one explicit save: the element, its sheet
//! and its aliases, with the new version it adds — its number, source,
//! snapshot and diff (EntityVersion@1). The version's fields are judged on
//! the payload alone: the row keeps the last version's, which a new save does
//! not inherit. The version the element really takes is settled once the
//! save is applied ([`settled`]): in the tables, in the apply transaction
//! (`projection`), and in the [`History`] a restoration is prepared against.

use std::collections::BTreeMap;
use std::sync::Arc;

use harness_dataguard::command::{Effect, Entry, Mode};
use harness_dataguard::plugins::{Judge, Plugin, Project};
use harness_dataguard::store::Write;
use serde_json::Value;

mod aliases;
mod history;
mod projection;
mod version;

pub use aliases::{Aliases, Element, parsed};
pub use history::History;
pub use version::{Kept, settled, timestamp};

/// The aggregate's manifest (contract I), as the engine parses it.
pub const MANIFEST: &str = include_str!("../aggregate.json");

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

/// What an element holds from its creation on: its campaign, its type, its
/// name and the time it was made. A complete snapshot is of an element
/// holding them.
const WHOLE: [&str; 4] = ["entityCampaignId", "type", "name", "entityCreatedAt"];

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

/// The invariant judged against the rest of the campaign.
const ALIAS_UNIQUE: &str = "alias-unique-in-campaign";

/// The aggregate's invariants as code, `alias-unique-in-campaign` read from
/// the other elements' terms.
struct Entity {
    aliases: Arc<Aliases>,
}

impl Judge for Entity {
    fn violated(&self, state: &BTreeMap<String, Value>, entry: &Entry) -> Vec<String> {
        // A deletion is the engine's tombstone: nothing of the row is left to
        // judge.
        if entry.effect == Effect::Delete {
            return Vec::new();
        }
        let mut violated: Vec<String> = CHECKS
            .iter()
            .filter(|(_, holds)| !holds(state, entry))
            .map(|(id, _)| (*id).to_string())
            .collect();
        if !alias_unique_in_campaign(&self.aliases, state) {
            violated.push(ALIAS_UNIQUE.to_string());
        }
        violated
    }
}

/// What the aggregate plugs into the engine: its judge, over an index of
/// its own; what follows each write the engine applies — that index and the
/// [`History`] of the versions, kept in memory, and over PostgreSQL its
/// tables, in the apply transaction ([`projection::Tables`]); and the
/// history, shared for a restoration to be prepared against.
#[must_use]
pub fn plugin() -> Plugin {
    keeping(Arc::default())
}

/// [`plugin`], its versions kept in `history`, which the caller reads too.
#[must_use]
pub fn keeping(history: Arc<History>) -> Plugin {
    let aliases = Arc::new(Aliases::default());
    Plugin::new()
        .judge(
            AGGREGATE,
            Entity {
                aliases: aliases.clone(),
            },
        )
        .project(
            AGGREGATE,
            Recorded {
                aliases: aliases.clone(),
                history: history.clone(),
            },
        )
        .project_pg(
            AGGREGATE,
            projection::Tables {
                aliases,
                history: history.clone(),
            },
        )
        .share(history)
}

/// Keeps the index and the history as the engine applies the aggregate's
/// writes — a write the tables refused is never applied: an element's terms
/// are its row's, its version is settled after its last one, and a removed
/// element has neither.
struct Recorded {
    aliases: Arc<Aliases>,
    history: Arc<History>,
}

impl Project for Recorded {
    fn observe(&self, write: &Write) {
        let Some(element) = write.partition.strip_prefix("Entity/") else {
            return;
        };
        if write.tombstone {
            self.aliases.forget(element);
            self.history.forget(element);
        } else {
            self.aliases.record(element, &write.values);
            self.history.settle(element, &write.values);
        }
    }
}

/// No alias of the element, normalised (AliasMatch@1), names another element
/// of its campaign, by its name or one of its aliases; nor does its name
/// double another's alias.
fn alias_unique_in_campaign(aliases: &Aliases, state: &BTreeMap<String, Value>) -> bool {
    value(state, "entityId")
        .as_str()
        .is_none_or(|element| aliases.taken(element, state).is_none())
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
/// The snapshot is complete of a whole element alone ([`WHOLE`]): judged at
/// application on the row as it really is, a save queued behind the creation
/// of its element is rejected when that creation was not applied — cancelled
/// or rejected — instead of writing an element that never was.
///
/// Past, not next: the engine holds the row as it was when the command was
/// queued, and a save queued behind another rightly takes the number after
/// it. The table, which holds the versions written, wants the next one
/// exactly.
fn every_change_versioned(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    let confirmed = &entry.projection.confirmed;
    let number = value(&entry.payload, "versionNumber").as_u64();
    let last = value(confirmed, "versionNumber").as_u64().unwrap_or(0);
    let id = value(&entry.payload, "entityVersionId");
    number.is_some_and(|number| number > last)
        && source(entry).is_some_and(|source| SOURCES.contains(&source))
        && value(&entry.payload, "snapshot").is_object()
        && value(&entry.payload, "diff").is_array()
        && (id.is_null() || id != value(confirmed, "entityVersionId"))
        && WHOLE.iter().all(|field| value(state, field).is_string())
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
/// portrait stay as they are. A resync is a write on an element that exists
/// when it is queued — linked to D&D Beyond or not: an import that creates
/// its PC, on an empty row, writes it whole.
fn dndbeyond_resync_scope(_state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    source(entry) != Some("dndbeyond")
        || entry.projection.confirmed.is_empty()
        || changed_outside(entry, &RESYNC).next().is_none()
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
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use harness_dataguard::command::{BasedOn, Effect, Entry, Mode, Projection, State};
    use harness_dataguard::plugins::Project;
    use harness_dataguard::store::Write;
    use serde_json::json;

    use super::{
        ABILITIES, ALIAS_UNIQUE, Aliases, BOOKKEEPING, CHECKS, DEX_MODIFIER, History, PROPOSAL,
        RESYNC, Recorded, SHEET, VERSION, WHOLE, dex_modifier_derived,
    };

    const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";

    #[test]
    fn an_applied_element_is_recorded_and_a_removed_one_forgotten_another_aggregate_ignored() {
        let aliases = Arc::new(Aliases::default());
        let history = Arc::new(History::default());
        let recorded = Recorded {
            aliases: aliases.clone(),
            history: history.clone(),
        };
        let write = |partition: &str, tombstone: bool| Write {
            partition: partition.to_owned(),
            values: serde_json::from_value(json!({
                "entityId": "e1000000-0000-4000-8000-000000000003",
                "entityCampaignId": VALOMBRE,
                "name": "Morlox",
                "alias": ["le gobelin"],
                "entityVersionId": "v1",
            }))
            .expect("a row"),
            tombstone,
        };
        let morlox = "Entity/e1000000-0000-4000-8000-000000000003";
        recorded.observe(&write(
            "Campaign/e1000000-0000-4000-8000-000000000003",
            false,
        ));
        assert!(aliases.known(VALOMBRE).is_empty());
        recorded.observe(&write(morlox, false));
        assert_eq!(aliases.known(VALOMBRE).len(), 1);
        assert_eq!(history.of(&morlox[7..]).len(), 1);
        recorded.observe(&write(morlox, true));
        assert!(aliases.known(VALOMBRE).is_empty());
        assert!(history.of(&morlox[7..]).is_empty());
    }

    /// A save touching `touches`, which the engine would have queued.
    fn touching(touches: &[&str]) -> Entry {
        Entry {
            command: "c1".to_owned(),
            data_capability: "test.saveEntity@1".to_owned(),
            by: "dm".to_owned(),
            partition: "Entity/e1000000-0000-4000-8000-000000000001".to_owned(),
            position: 0,
            based_on: BasedOn::default(),
            payload: BTreeMap::new(),
            effect: Effect::Update,
            touches: touches.iter().map(|touch| (*touch).to_owned()).collect(),
            mode: Mode::Overwrite,
            projection: Projection::default(),
            state: State::Queued,
            confirmation: None,
            at_head_since: None,
            parked: None,
            requeued_from: None,
            idempotency_key: None,
            attachment: None,
        }
    }

    #[test]
    fn dex_modifier_derived_holds_only_when_neither_the_row_nor_a_touch_carries_the_modifier() {
        let row = |fields: serde_json::Value| -> BTreeMap<String, serde_json::Value> {
            serde_json::from_value(fields).expect("a row")
        };
        let score = row(json!({ "dexterity": 16 }));
        let typed = row(json!({ "dexterity": 16, DEX_MODIFIER: 3 }));
        // The score alone, written or touched: the modifier is left to be computed.
        assert!(dex_modifier_derived(&score, &touching(&[])));
        assert!(dex_modifier_derived(
            &score,
            &touching(&["Entity.dexterity"])
        ));
        // The modifier on the row breaks it, whatever the command touches.
        assert!(!dex_modifier_derived(&typed, &touching(&[])));
        assert!(!dex_modifier_derived(
            &typed,
            &touching(&["Entity.dexterity"])
        ));
        // The modifier touched breaks it, though the row does not carry it.
        assert!(!dex_modifier_derived(
            &score,
            &touching(&["Entity.dexterity", "Entity.dexterityModifier"])
        ));
        assert!(!dex_modifier_derived(&score, &touching(&[DEX_MODIFIER])));
        // A touch that only ends like the modifier is another field.
        assert!(dex_modifier_derived(
            &score,
            &touching(&["Entity.notDexterityModifier"])
        ));
    }

    fn aggregate() -> serde_json::Value {
        serde_json::from_str(include_str!("../aggregate.json")).expect("contract I")
    }

    #[test]
    fn every_field_the_judge_names_is_one_the_aggregate_declares() {
        let aggregate = aggregate();
        let fields = aggregate["fields"].as_object().expect("fields");
        for field in SHEET
            .iter()
            .chain(&VERSION)
            .chain(&BOOKKEEPING)
            .chain(&WHOLE)
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
    fn the_judge_checks_every_invariant_the_aggregate_declares() {
        let aggregate = aggregate();
        let mut declared: Vec<&str> = aggregate["invariants"]
            .as_array()
            .expect("invariants")
            .iter()
            .map(|invariant| invariant["id"].as_str().expect("an id"))
            .collect();
        let mut judged: Vec<&str> = CHECKS.iter().map(|(id, _)| *id).collect();
        judged.push(ALIAS_UNIQUE);
        declared.sort_unstable();
        judged.sort_unstable();
        assert_eq!(judged, declared);
    }
}
