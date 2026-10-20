//! The invariants of the `BattlemapInstance` aggregate
//! (`crates/dataguard/aggregates/BattlemapInstance/aggregate.json`), which
//! its rules write in words: the engine judges here what one row and its
//! command can say, on the row's state after the command. The aggregate
//! spans four tables (`crates/data/migrations/0010_battlemap_instance.sql`)
//! — a scene's instance of a battlemap, its tokens, its combat and the
//! combat's initiative — and the order view; a row is judged by the fields
//! of each table it holds.
//!
//! The derived fields are the tables': the token's death, band and initial
//! are generated columns, the order is `initiative_order_view`'s. A command
//! never writes them. What reads the other rows — the only instance of a
//! scene, the only combat running, a drop's numbers, the PCs present at the
//! session, the ranks a launch gives, the participant « Suivant » lands
//! on — is not the row's to tell: the engine judges a row alone.
//!
//! What the row held before is the one the command saw when it was queued
//! (`projection.confirmed`): commands queued ahead of it may have changed
//! the row since. A field the judge keeps never changes once set, so that
//! view can only miss a value a command ahead set first — and a command
//! that writes such a field back to null is refused whatever it saw; a
//! round, which moves, is judged by the command's own step and against the
//! row as it is.

use std::collections::BTreeMap;

use harness_dataguard::command::{Effect, Entry, Mode};
use harness_dataguard::guard::Invariants;
use serde_json::Value;

const AGGREGATE: &str = "BattlemapInstance";

/// The fields of the instance, `battlemap_instance`'s.
const INSTANCE: [&str; 6] = [
    "battlemapInstanceId",
    "battlemapMediaId",
    "sessionId",
    "sceneId",
    "createdAt",
    "resetAt",
];

/// The fields of a token, `token`'s.
const TOKEN: [&str; 14] = [
    "tokenId",
    "tokenInstanceId",
    "characterId",
    "number",
    "name",
    "q",
    "r",
    "visible",
    "hpCurrent",
    "hpMax",
    "conditions",
    "dead",
    "healthBand",
    "labelInitial",
];

/// The fields of a combat, `combat`'s.
const COMBAT: [&str; 6] = [
    "combatId",
    "combatInstanceId",
    "round",
    "currentEntryId",
    "startedAt",
    "endedAt",
];

/// The fields of a participant's place in a combat, `initiative_entry`'s.
const ENTRY: [&str; 8] = [
    "initiativeEntryId",
    "initiativeEntryCombatId",
    "initiativeEntryTokenId",
    "roll",
    "initiative",
    "dexScore",
    "tiebreak",
    "rank",
];

/// The fields of the carousel's order, `initiative_order_view`'s.
const ORDER: [&str; 5] = [
    "initiativeOrderViewCombatId",
    "entryId",
    "orderIndex",
    "shownToPlayers",
    "skipped",
];

/// Each derived field, and the invariant its writing breaks: the tables
/// compute them, from the token's HP, its visibility, its name and the
/// entries' ranks.
const DERIVED: [(&str, &str); 8] = [
    ("dead", "dead-derived"),
    ("skipped", "dead-derived"),
    ("healthBand", "players-see-band-only"),
    ("shownToPlayers", "players-see-band-only"),
    ("labelInitial", "multi-drop-numbering"),
    ("orderIndex", "hidden-keeps-rank"),
    ("entryId", "hidden-keeps-rank"),
    ("initiativeOrderViewCombatId", "hidden-keeps-rank"),
];

/// The links a token, a combat and an entry belong to their instance by.
const OWNED_BY_INSTANCE: [&str; 4] = [
    "tokenInstanceId",
    "combatInstanceId",
    "initiativeEntryCombatId",
    "initiativeEntryTokenId",
];

/// What an instance is the instance of: its battlemap, its session, its
/// scene.
const PLACED: [&str; 3] = ["battlemapMediaId", "sessionId", "sceneId"];

/// A d20.
const D20: std::ops::RangeInclusive<i64> = 1..=20;

/// One invariant: its id, and whether it holds on the state the command
/// leaves.
type Check = (&'static str, fn(&BTreeMap<String, Value>, &Entry) -> bool);

const CHECKS: [Check; 15] = [
    ("instance-isolated", instance_isolated),
    ("one-instance-per-scene", one_instance_per_scene),
    ("token-roster", token_roster),
    ("multi-drop-numbering", multi_drop_numbering),
    ("pj-hp-not-tracked", pj_hp_not_tracked),
    ("hp-change-one-token", hp_change_one_token),
    ("dead-derived", dead_derived),
    ("players-see-band-only", players_see_band_only),
    ("npc-initiative-bounds", npc_initiative_bounds),
    ("initiative-tiebreak", initiative_tiebreak),
    ("hidden-keeps-rank", hidden_keeps_rank),
    ("next-turn", next_turn),
    ("one-active-combat", one_active_combat),
    ("state-persisted-reset", state_persisted_reset),
    ("single-position-source", single_position_source),
];

/// The invariants `plant()` binds the engine with.
pub struct BattlemapInstance;

impl Invariants for BattlemapInstance {
    fn violated(
        &self,
        aggregate: &str,
        state: &BTreeMap<String, Value>,
        entry: &Entry,
    ) -> Vec<String> {
        // A deletion is the engine's tombstone — « Réinitialiser la carte »
        // removes tokens, combat and initiative so: nothing of the row is
        // left to judge.
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

/// Whether `row` holds a value for one of `fields`: a row of their table.
fn holds_any(row: &BTreeMap<String, Value>, fields: &[&str]) -> bool {
    fields.iter().any(|field| !value(row, field).is_null())
}

/// A whole number as JSON writes it, within the table's `integer`.
fn integer(value: &Value) -> Option<i64> {
    value.as_i64().filter(|n| i32::try_from(*n).is_ok())
}

/// A whole number within the table's `smallint`: HP, a token's number, a
/// roll, an initiative, a score.
fn smallint(value: &Value) -> Option<i64> {
    value.as_i64().filter(|n| i16::try_from(*n).is_ok())
}

/// Two values alike, an id in either case being the same one.
fn same(a: &Value, b: &Value) -> bool {
    match (a.as_str(), b.as_str()) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => a == b,
    }
}

/// Once the row held a value for `field` — as the command saw it when
/// queued — the command leaves it as it was.
fn kept(state: &BTreeMap<String, Value>, entry: &Entry, field: &str) -> bool {
    let before = value(&entry.projection.confirmed, field);
    before.is_null() || same(before, value(state, field))
}

/// Whether the command writes `field` back to null on a row it does not
/// create: the row it lands on may hold a value its author never saw.
fn clears(entry: &Entry, field: &str) -> bool {
    entry.effect != Effect::Insert && entry.payload.get(field).is_some_and(Value::is_null)
}

/// Whether `field` is, when the command applies, what its author saw: the
/// queue matched it against what was ahead, and nothing was.
fn unmoved_since_seen(entry: &Entry, field: &str) -> bool {
    entry.projection.pending_ahead.is_empty() && entry.fields().iter().any(|own| own == field)
}

/// Whether the command writes `field`, by its payload or by a touch.
fn writes(entry: &Entry, field: &str) -> bool {
    entry.payload.contains_key(field)
        || entry
            .touches
            .iter()
            .any(|touch| touch.rsplit('.').next() == Some(field))
}

/// Whether the command writes one of the derived fields `invariant` keeps
/// to the tables.
fn writes_derived(entry: &Entry, invariant: &str) -> bool {
    DERIVED
        .iter()
        .any(|(field, id)| *id == invariant && writes(entry, field))
}

/// An instance writes its own fields alone, by its payload and by its
/// touches, never the battlemap's nor its grid's; its tokens, its combat and
/// their initiative stay its own: an entry names its combat and its token,
/// and none moves to another instance, combat or token.
fn instance_isolated(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    entry.payload.keys().all(|field| declared(field))
        && entry.touches.iter().all(|touch| {
            touch
                .split_once('.')
                .is_some_and(|(aggregate, field)| aggregate == AGGREGATE && declared(field))
        })
        && (!holds_any(state, &ENTRY)
            || ["initiativeEntryCombatId", "initiativeEntryTokenId"]
                .iter()
                .all(|field| !value(state, field).is_null()))
        && OWNED_BY_INSTANCE
            .iter()
            .all(|field| kept(state, entry, field))
}

/// Whether `field` is one of the aggregate's.
fn declared(field: &str) -> bool {
    INSTANCE
        .iter()
        .chain(&TOKEN)
        .chain(&COMBAT)
        .chain(&ENTRY)
        .chain(&ORDER)
        .any(|own| *own == field)
}

/// An instance is the instance of one battlemap, in one session, for one
/// scene: it names the three, and keeps them. That no other instance has its
/// scene, and that the battlemap is the scene's and its campaign the
/// session's, reads other rows.
fn one_instance_per_scene(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    !holds_any(state, &INSTANCE)
        || PLACED
            .iter()
            .all(|field| !value(state, field).is_null() && kept(state, entry, field))
}

/// A token is dropped on an instance, for one character, which it keeps:
/// re-pointing it at another would pass a PC the roster never checked. That
/// the PC is present at the session, and has no other token here, reads
/// other rows.
fn token_roster(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    !holds_any(state, &TOKEN)
        || (!value(state, "tokenInstanceId").is_null()
            && !value(state, "characterId").is_null()
            && kept(state, entry, "characterId"))
}

/// A token has a name (« Bandit 2 »), and its number, when it has one,
/// counts from 1 and is the one its drop gave it: renaming the token touches
/// its name alone, and the name's initial is the table's. That the numbers
/// follow the ones taken, and are unique for the character in the instance,
/// reads other rows.
fn multi_drop_numbering(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    let number = value(state, "number");
    let named = !holds_any(state, &TOKEN)
        || value(state, "name")
            .as_str()
            .is_some_and(|name| !name.trim().is_empty());
    named
        && (number.is_null() || smallint(number).is_some_and(|number| number >= 1))
        && kept(state, entry, "number")
        && !writes_derived(entry, "multi-drop-numbering")
}

/// A PC's token tracks no HP, neither current nor maximum; an NPC's tracks
/// both, its current HP from 0 to its maximum.
fn pj_hp_not_tracked(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    match (value(state, "hpCurrent"), value(state, "hpMax")) {
        (Value::Null, Value::Null) => true,
        (current, max) => match (smallint(current), smallint(max)) {
            (Some(current), Some(max)) => 0 <= current && current <= max,
            _ => false,
        },
    }
}

/// HP are written to one token, the one the command's row is: a token's,
/// which stays the same token.
fn hp_change_one_token(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    let hp = writes(entry, "hpCurrent") || writes(entry, "hpMax");
    kept(state, entry, "tokenId") && (!hp || !value(state, "tokenInstanceId").is_null())
}

/// Death is the HP's: the table's generated column, which the order view
/// skips. A command writes neither.
fn dead_derived(_state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    !writes_derived(entry, "dead-derived")
}

/// What the players see of a token is the band the table derives from its
/// HP, and whether it is shown follows its visibility, which every token
/// says: a command writes neither of the two.
fn players_see_band_only(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    (!holds_any(state, &TOKEN) || value(state, "visible").is_boolean())
        && !writes_derived(entry, "players-see-band-only")
}

/// An entry's initiative is a whole number. An NPC rolls a d20, and its
/// initiative is the roll plus its DEX modifier (AbilityModifier@1); a PC
/// rolls nothing, and its initiative is the DM's.
fn npc_initiative_bounds(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    if !holds_any(state, &ENTRY) {
        return true;
    }
    let dex = value(state, "dexScore");
    let Some(initiative) =
        smallint(value(state, "initiative")).filter(|_| dex.is_null() || smallint(dex).is_some())
    else {
        return false;
    };
    let roll = value(state, "roll");
    if roll.is_null() {
        return true;
    }
    match (smallint(roll), smallint(dex)) {
        (Some(roll), Some(dex)) => D20.contains(&roll) && initiative == roll + modifier(dex),
        _ => false,
    }
}

/// A score's modifier: half its distance to 10, rounded down
/// (AbilityModifier@1).
fn modifier(score: i64) -> i64 {
    (score - 10).div_euclid(2)
}

/// An entry has a rank, from 1, and the draw that breaks the ties left at
/// equal DEX: a number drawn at the launch and kept from then on, so the
/// same participants always give the same order. That the ranks follow
/// initiative, DEX and draw reads the combat's other entries. A rank step
/// (`table.addParticipant`) on a row that holds no entry — a removed
/// participant's tombstone reads as none — leaves an entry with no draw: it
/// is refused, and brings no row back.
fn initiative_tiebreak(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    !holds_any(state, &ENTRY)
        || (integer(value(state, "rank")).is_some_and(|rank| rank >= 1)
            && value(state, "tiebreak")
                .as_f64()
                .is_some_and(f64::is_finite)
            && kept(state, entry, "tiebreak"))
}

/// Hiding or revealing a token moves no rank: a command that writes the
/// token's visibility writes no rank. The order is the view's, by rank: a
/// command writes none of its fields.
fn hidden_keeps_rank(_state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    !(writes(entry, "visible") && writes(entry, "rank"))
        && !writes_derived(entry, "hidden-keeps-rank")
}

/// A combat starts at round 1. « Suivant » takes it one round further at
/// most, when it lands on a participant, and never back; an ended combat
/// turns no more. Which participant is next
/// reads the combat's entries.
fn next_turn(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    if !holds_any(state, &COMBAT) {
        return true;
    }
    let Some(round) = integer(value(state, "round")).filter(|round| *round >= 1) else {
        return false;
    };
    // A command that writes no round leaves the one commands ahead moved
    // to. A relative one steps by its own number, on the round as it is,
    // whatever its author saw. An absolute one replaces the round the judge
    // no longer sees: it is judged against the round its author saw, only
    // when nothing queued ahead of it could have moved it since; a round
    // written on a combat its author never saw starts it.
    let step = if !entry.payload.contains_key("round") {
        0
    } else if entry.mode == Mode::Relative {
        integer(value(&entry.payload, "round")).unwrap_or(0)
    } else if !unmoved_since_seen(entry, "round") {
        return false;
    } else if let Some(seen) = integer(value(&entry.projection.confirmed, "round")) {
        round - seen
    } else {
        return round == 1;
    };
    // The row as it is: an end queued ahead of this command, or written by
    // it, is in it.
    let ended = !value(state, "endedAt").is_null();
    let turns = entry.payload.contains_key("round")
        || entry
            .payload
            .get("currentEntryId")
            .is_some_and(|current| !current.is_null());
    (0..=1).contains(&step)
        && (step == 0 || !value(state, "currentEntryId").is_null())
        && !(ended && turns)
}

/// A combat starts on one instance and, once ended, stays ended: reopening
/// it beside the one started since would leave two running. That no other
/// combat of the instance runs reads other rows.
fn one_active_combat(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    !holds_any(state, &COMBAT)
        || (!value(state, "combatInstanceId").is_null()
            && !value(state, "startedAt").is_null()
            && kept(state, entry, "startedAt")
            && kept(state, entry, "endedAt")
            && !clears(entry, "endedAt"))
}

/// What a command persists is what the row reads after: the instance keeps
/// its creation and its last reset, which a new one replaces but never
/// clears; a token's conditions are a list of names, empty when it has none.
fn state_persisted_reset(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    let conditions = match value(state, "conditions") {
        Value::Null => !holds_any(state, &TOKEN),
        Value::Array(names) => names
            .iter()
            .all(|name| name.as_str().is_some_and(|name| !name.trim().is_empty())),
        _ => false,
    };
    !clears(entry, "resetAt") && kept(state, entry, "createdAt") && conditions
}

/// A token stands on one hex, its axial (q, r), which both views read: a
/// token has both, whole numbers.
fn single_position_source(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    !holds_any(state, &TOKEN)
        || ["q", "r"]
            .iter()
            .all(|axis| integer(value(state, axis)).is_some())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::{CHECKS, COMBAT, DERIVED, ENTRY, INSTANCE, ORDER, TOKEN, modifier};

    fn aggregate() -> Value {
        serde_json::from_str(include_str!(
            "../../aggregates/BattlemapInstance/aggregate.json"
        ))
        .expect("contract I")
    }

    #[test]
    fn the_tables_fields_are_the_aggregate_s_and_its_derived_ones_the_tables() {
        let aggregate = aggregate();
        let fields = aggregate["fields"].as_object().expect("fields");
        let mut tables: Vec<&str> = INSTANCE
            .iter()
            .chain(&TOKEN)
            .chain(&COMBAT)
            .chain(&ENTRY)
            .chain(&ORDER)
            .copied()
            .collect();
        tables.sort_unstable();
        let mut declared: Vec<&str> = fields.keys().map(String::as_str).collect();
        declared.sort_unstable();
        assert_eq!(tables, declared);
        let mut derived: Vec<&str> = DERIVED.iter().map(|(field, _)| *field).collect();
        derived.sort_unstable();
        let mut declared_derived: Vec<&str> = fields
            .iter()
            .filter(|(_, field)| field["kind"] == "derived")
            .map(|(name, _)| name.as_str())
            .collect();
        declared_derived.sort_unstable();
        assert_eq!(derived, declared_derived);
    }

    #[test]
    fn the_judge_checks_every_invariant_the_aggregate_declares() {
        let aggregate = aggregate();
        let declared: Vec<&str> = aggregate["invariants"]
            .as_array()
            .expect("invariants")
            .iter()
            .map(|invariant| invariant["id"].as_str().expect("an id"))
            .collect();
        let judged: Vec<&str> = CHECKS.iter().map(|(id, _)| *id).collect();
        assert_eq!(judged, declared);
        for (_, id) in DERIVED {
            assert!(declared.contains(&id), "{id}");
        }
    }

    #[test]
    fn npc_initiative_bounds_adds_the_dex_modifier_of_ability_modifier_1() {
        let fixtures = platform_conformance::fixtures_of("AbilityModifier@1");
        let computed: BTreeMap<String, Value> = fixtures
            .cases
            .iter()
            .map(|case| {
                let score = case.input["score"].as_i64().expect("a score");
                (
                    case.input.to_string(),
                    json!({ "modifier": modifier(score) }),
                )
            })
            .collect();
        platform_conformance::check("AbilityModifier@1", |input| {
            computed[&input.to_string()].clone()
        });
    }
}
