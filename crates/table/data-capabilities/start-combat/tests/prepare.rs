//! `table.startCombat`'s judgement before a command is queued: what the DM
//! sends, read ([`request`]), and made the combat's row and its entries'
//! against the instance's lineup ([`prepare`]), with scripted dice; and each
//! follow-up's seat of one entry ([`seat`], [`seatable`], [`follow_ups`]).

use std::collections::VecDeque;

use serde_json::{Value, json};
use start_combat::{
    DATA_CAPABILITY, DEFAULT_DEX, Dice, Lineup, Participant, Refusal, Request, Row, Started,
    combat_id, entry_id, follow_ups, partition, prepare, request, seat, seatable,
};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const NOW: &str = "2026-10-20T20:05:00.000Z";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000002";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000003";
const VESNA: &str = "e1000000-0000-4000-8000-000000000004";
const T_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const T_KAEL: &str = "70000000-0000-4000-8000-000000000002";
const T_BANDIT: &str = "70000000-0000-4000-8000-000000000003";
const T_VESNA: &str = "70000000-0000-4000-8000-000000000004";

fn row(value: Value) -> Row {
    let Value::Object(fields) = value else {
        panic!("a row is an object");
    };
    fields.into_iter().collect()
}

fn manifest() -> Value {
    serde_json::from_str(include_str!("../data-capability.json")).expect("contract F")
}

/// Dice that throw what the test scripts, in order; then 10s and 0s.
#[derive(Default)]
struct Scripted {
    d20s: VecDeque<i64>,
    draws: VecDeque<i64>,
    thrown: usize,
}

impl Scripted {
    fn new(d20s: &[i64], draws: &[i64]) -> Self {
        Self {
            d20s: d20s.iter().copied().collect(),
            draws: draws.iter().copied().collect(),
            thrown: 0,
        }
    }
}

impl Dice for Scripted {
    fn d20(&mut self) -> i64 {
        self.thrown += 1;
        self.d20s.pop_front().unwrap_or(10)
    }

    fn draw(&mut self) -> i64 {
        self.draws.pop_front().unwrap_or(0)
    }
}

fn token(token: &str, character: &str, kind: &str, dex: Option<i64>) -> Participant {
    Participant {
        token: token.to_string(),
        character: character.to_string(),
        kind: Some(kind.to_string()),
        dex,
    }
}

/// R105's table: Elowen a PC of DEX 14, the Bandit an NPC of DEX 12, Vesna
/// an NPC of DEX 16.
fn lineup() -> Lineup {
    Lineup {
        instance: true,
        tokens: vec![
            token(T_ELOWEN, ELOWEN, "pj", Some(14)),
            token(T_BANDIT, BANDIT, "pnj", Some(12)),
            token(T_VESNA, VESNA, "pnj", Some(16)),
        ],
        open: false,
    }
}

/// The start `payload` asks for on the instance, against `lineup`.
fn start(payload: Value, lineup: &Lineup, dice: &mut Scripted) -> Result<Started, Refusal> {
    let request = request(&row(payload), Some(INSTANCE)).expect("a request");
    prepare(&request, lineup, COMBAT, NOW, dice)
}

/// The entries' tokens, rank 1 first.
fn order(prepared: &Started) -> Vec<&str> {
    prepared
        .entries
        .iter()
        .map(|entry| entry["initiativeEntryTokenId"].as_str().expect("a token"))
        .collect()
}

fn entry_of<'a>(prepared: &'a Started, token: &str) -> &'a Row {
    prepared
        .entries
        .iter()
        .find(|entry| entry["initiativeEntryTokenId"] == token)
        .expect("an entry")
}

#[test]
fn a_combat_and_its_entries_are_partitions_of_the_aggregate_with_derived_ids() {
    assert_eq!(partition(COMBAT), format!("BattlemapInstance/{COMBAT}"));
    // The same instance and generation give the same combat, another
    // generation or instance another one.
    assert_eq!(combat_id(INSTANCE, 0), combat_id(INSTANCE, 0));
    assert_ne!(combat_id(INSTANCE, 0), combat_id(INSTANCE, 1));
    assert_ne!(
        combat_id(INSTANCE, 0),
        combat_id("b1000000-0000-4000-8000-000000000002", 0)
    );
    assert_eq!(entry_id(COMBAT, T_BANDIT), entry_id(COMBAT, T_BANDIT));
    assert_ne!(entry_id(COMBAT, T_BANDIT), entry_id(COMBAT, T_VESNA));
    assert_ne!(
        entry_id(COMBAT, T_BANDIT),
        entry_id(&combat_id(INSTANCE, 0), T_BANDIT)
    );
    for id in [combat_id(INSTANCE, 0), entry_id(COMBAT, T_BANDIT)] {
        assert_eq!(id.len(), 36, "{id}");
        assert_eq!(id, id.to_ascii_lowercase(), "{id}");
    }
}

#[test]
fn the_manifest_is_an_insert_touching_every_field_a_start_writes() {
    let manifest = manifest();
    assert_eq!(manifest["dataCapability"], "table.startCombat");
    assert_eq!(manifest["effect"], "insert");
    assert_eq!(manifest["target"]["aggregate"], "BattlemapInstance");
    let prepared = start(
        json!({ "initiative": { ELOWEN: 18 } }),
        &lineup(),
        &mut Scripted::default(),
    )
    .expect("started");
    let mut written: Vec<String> = prepared
        .entries
        .iter()
        .flat_map(|entry| entry.keys())
        .chain(prepared.combat.keys())
        .map(|field| format!("BattlemapInstance.{field}"))
        .collect();
    written.sort_unstable();
    written.dedup();
    let mut touched: Vec<String> = manifest["touches"]
        .as_array()
        .expect("touches")
        .iter()
        .map(|touch| touch.as_str().expect("a field").to_string())
        .collect();
    touched.sort_unstable();
    assert_eq!(written, touched);
    for invariant in [
        "npc-initiative-bounds",
        "initiative-tiebreak",
        "hidden-keeps-rank",
        "one-active-combat",
    ] {
        assert!(
            manifest["invariants"]
                .as_array()
                .expect("invariants")
                .contains(&json!(invariant)),
            "{invariant}"
        );
    }
}

#[test]
fn initiative_tiebreak_the_pc_at_18_first_then_dex_16_before_dex_12() {
    // Bandit 13 + 1 and Vesna 11 + 3 both total 14: DEX settles it.
    let prepared = start(
        json!({ "initiative": { ELOWEN: 18 }, "rolls": { T_BANDIT: 13, T_VESNA: 11 } }),
        &lineup(),
        // The draws would put the Bandit first: DEX comes before them.
        &mut Scripted::new(&[], &[1, 99, 2]),
    )
    .expect("started");
    assert_eq!(order(&prepared), [T_ELOWEN, T_VESNA, T_BANDIT]);
    let ranks: Vec<&Value> = prepared.entries.iter().map(|e| &e["rank"]).collect();
    assert_eq!(ranks, [&json!(1), &json!(2), &json!(3)]);
    assert_eq!(entry_of(&prepared, T_BANDIT)["initiative"], 14);
    assert_eq!(entry_of(&prepared, T_VESNA)["initiative"], 14);
}

#[test]
fn initiative_tiebreak_equal_initiative_and_dex_go_to_the_higher_draw() {
    let twins = Lineup {
        instance: true,
        tokens: vec![
            token(T_BANDIT, BANDIT, "pnj", Some(12)),
            token(T_VESNA, BANDIT, "pnj", Some(12)),
        ],
        open: false,
    };
    let rolls = json!({ "rolls": { T_BANDIT: 10, T_VESNA: 10 } });
    let low_first =
        start(rolls.clone(), &twins, &mut Scripted::new(&[], &[3, 7])).expect("started");
    assert_eq!(order(&low_first), [T_VESNA, T_BANDIT]);
    let high_first = start(rolls, &twins, &mut Scripted::new(&[], &[7, 3])).expect("started");
    assert_eq!(order(&high_first), [T_BANDIT, T_VESNA]);
    // The draw is kept on the entry, so the same order is read again.
    assert_eq!(entry_of(&high_first, T_BANDIT)["tiebreak"], 7);
    assert_eq!(entry_of(&high_first, T_VESNA)["tiebreak"], 3);
}

#[test]
fn npc_initiative_bounds_an_npc_with_no_roll_sent_throws_a_d20_plus_its_modifier() {
    let mut dice = Scripted::new(&[1, 20], &[]);
    let prepared = start(
        json!({ "initiative": { ELOWEN: 18 } }),
        &lineup(),
        &mut dice,
    )
    .expect("started");
    assert_eq!(dice.thrown, 2, "one d20 per NPC, none for the PC");
    let bandit = entry_of(&prepared, T_BANDIT);
    assert_eq!(
        (&bandit["roll"], &bandit["initiative"]),
        (&json!(1), &json!(2))
    );
    assert_eq!(bandit["dexScore"], 12);
    let vesna = entry_of(&prepared, T_VESNA);
    assert_eq!(
        (&vesna["roll"], &vesna["initiative"]),
        (&json!(20), &json!(23))
    );
    assert_eq!(vesna["dexScore"], 16);
}

#[test]
fn npc_initiative_bounds_a_roll_sent_is_thrown_no_more() {
    let mut dice = Scripted::new(&[20], &[]);
    let prepared = start(
        json!({ "initiative": { ELOWEN: 18 }, "rolls": { T_BANDIT: 4 } }),
        &lineup(),
        &mut dice,
    )
    .expect("started");
    assert_eq!(dice.thrown, 1, "Vesna alone throws");
    assert_eq!(entry_of(&prepared, T_BANDIT)["roll"], 4);
    assert_eq!(entry_of(&prepared, T_BANDIT)["initiative"], 5);
}

#[test]
fn npc_initiative_bounds_a_pc_rolls_nothing_its_initiative_is_the_dm_s() {
    let prepared = start(
        json!({ "initiative": { ELOWEN: 7 } }),
        &lineup(),
        &mut Scripted::default(),
    )
    .expect("started");
    let elowen = entry_of(&prepared, T_ELOWEN);
    assert_eq!(elowen["roll"], Value::Null);
    assert_eq!(elowen["initiative"], 7);
    // Its DEX is the sheet's: the ties read it.
    assert_eq!(elowen["dexScore"], 14);
}

#[test]
fn an_npc_whose_sheet_gives_no_dex_rolls_with_dex_10() {
    let lineup = Lineup {
        instance: true,
        tokens: vec![token(T_BANDIT, BANDIT, "pnj", None)],
        open: false,
    };
    let prepared = start(
        json!({ "rolls": { T_BANDIT: 9 } }),
        &lineup,
        &mut Scripted::default(),
    )
    .expect("started");
    let bandit = entry_of(&prepared, T_BANDIT);
    assert_eq!(bandit["dexScore"], DEFAULT_DEX);
    assert_eq!(bandit["initiative"], 9);
}

#[test]
fn hidden_keeps_rank_every_token_of_the_lineup_has_an_entry() {
    let prepared = start(
        json!({ "initiative": { ELOWEN: 18 } }),
        &lineup(),
        &mut Scripted::default(),
    )
    .expect("started");
    let mut tokens = order(&prepared);
    tokens.sort_unstable();
    assert_eq!(tokens, [T_ELOWEN, T_BANDIT, T_VESNA]);
    // No entry writes a token's visibility: a hidden one stays hidden.
    assert!(prepared.entries.iter().all(|e| !e.contains_key("visible")));
}

#[test]
fn next_turn_the_combat_opens_at_round_1_on_rank_1() {
    let prepared = start(
        json!({ "initiative": { ELOWEN: 18 }, "rolls": { T_BANDIT: 13, T_VESNA: 11 } }),
        &lineup(),
        &mut Scripted::default(),
    )
    .expect("started");
    let first = &prepared.entries[0]["initiativeEntryId"];
    assert_eq!(first, &json!(entry_id(COMBAT, T_ELOWEN)));
    assert_eq!(
        prepared.combat,
        row(json!({
            "combatId": COMBAT,
            "combatInstanceId": INSTANCE,
            "round": 1,
            "currentEntryId": first,
            "startedAt": NOW,
        }))
    );
    for entry in &prepared.entries {
        assert_eq!(entry["initiativeEntryCombatId"], COMBAT);
        let token = entry["initiativeEntryTokenId"].as_str().expect("a token");
        assert_eq!(entry["initiativeEntryId"], entry_id(COMBAT, token));
    }
    assert_eq!(prepared.entries[0]["initiativeEntryTokenId"], T_ELOWEN);
}

#[test]
fn one_active_combat_an_instance_whose_combat_is_open_starts_none() {
    let open = Lineup {
        open: true,
        ..lineup()
    };
    assert_eq!(
        start(
            json!({ "initiative": { ELOWEN: 18 } }),
            &open,
            &mut Scripted::default()
        ),
        Err(Refusal::CombatOpen)
    );
}

#[test]
fn a_start_the_lineup_forbids_is_refused() {
    let mut dice = Scripted::default();
    let unknown = Lineup::default();
    assert_eq!(
        start(json!({}), &unknown, &mut dice),
        Err(Refusal::UnknownInstance)
    );
    let empty = Lineup {
        instance: true,
        ..Lineup::default()
    };
    assert_eq!(
        start(json!({}), &empty, &mut dice),
        Err(Refusal::NoParticipant)
    );
    // Elowen's initiative was not entered; Kael's is of a PC with no token.
    assert_eq!(
        start(json!({ "initiative": { KAEL: 12 } }), &lineup(), &mut dice),
        Err(Refusal::MissingInitiative(ELOWEN.to_string()))
    );
    // A PC rolls nothing, and a token of another instance is none here.
    for token in [T_ELOWEN, T_KAEL] {
        assert_eq!(
            start(
                json!({ "initiative": { ELOWEN: 18 }, "rolls": { token: 12 } }),
                &lineup(),
                &mut dice
            ),
            Err(Refusal::RollOfNoNpc),
            "{token}"
        );
    }
    let orphan = Lineup {
        instance: true,
        tokens: vec![Participant {
            kind: None,
            ..token(T_BANDIT, BANDIT, "pnj", Some(12))
        }],
        open: false,
    };
    assert_eq!(
        start(json!({}), &orphan, &mut dice),
        Err(Refusal::UnknownCharacter)
    );
}

#[test]
fn ids_of_any_case_are_read_lowercase_and_the_target_comes_first() {
    let upper = INSTANCE.to_ascii_uppercase();
    let wire = row(json!({
        "instance": "b1000000-0000-4000-8000-0000000000ff",
        "initiative": { ELOWEN.to_ascii_uppercase(): 18 },
        "rolls": { T_BANDIT.to_ascii_uppercase(): 20 },
    }));
    assert_eq!(
        request(&wire, Some(&upper)),
        Ok(Request {
            instance: INSTANCE.to_string(),
            initiative: [(ELOWEN.to_string(), 18)].into(),
            rolls: [(T_BANDIT.to_string(), 20)].into(),
        })
    );
    let named = request(&row(json!({ "instance": INSTANCE })), None).expect("a request");
    assert_eq!(named.instance, INSTANCE);
    assert!(named.initiative.is_empty() && named.rolls.is_empty());
}

#[test]
fn what_the_tables_could_never_hold_is_refused() {
    let refused = |payload: Value| request(&row(payload), Some(INSTANCE)).err();
    assert_eq!(
        request(&row(json!({})), Some("la-taverne")),
        Err(Refusal::NotAnId("instance"))
    );
    assert_eq!(
        request(&row(json!({})), None),
        Err(Refusal::NotAnId("instance"))
    );
    // Hex digits alone are no uuid, nor 36 characters of anything.
    for instance in [
        "b1000000",
        INSTANCE.replace('-', "0").as_str(),
        &INSTANCE[..35],
    ] {
        assert_eq!(
            request(&row(json!({})), Some(instance)),
            Err(Refusal::NotAnId("instance")),
            "{instance}"
        );
    }
    assert_eq!(
        refused(json!({ "initiative": { "elowen": 18 } })),
        Some(Refusal::NotAnId("initiative"))
    );
    assert_eq!(
        refused(json!({ "rolls": { "bandit-1": 12 } })),
        Some(Refusal::NotAnId("rolls"))
    );
    for initiative in [
        json!(18),
        json!([18]),
        json!({ ELOWEN: 1.5 }),
        json!({ ELOWEN: "18" }),
        json!({ ELOWEN: 40_000 }),
    ] {
        assert_eq!(
            refused(json!({ "initiative": initiative })),
            Some(Refusal::NotAnInitiative),
            "{initiative}"
        );
    }
    for rolls in [
        json!(12),
        json!({ T_BANDIT: 0 }),
        json!({ T_BANDIT: 21 }),
        json!({ T_BANDIT: 12.5 }),
    ] {
        assert_eq!(
            refused(json!({ "rolls": rolls })),
            Some(Refusal::NotARoll),
            "{rolls}"
        );
    }
    // Negative initiatives are a PC's to have; nulls are none sent.
    assert_eq!(
        refused(json!({ "initiative": { ELOWEN: -2 }, "rolls": null })),
        None
    );
}

#[test]
fn every_refusal_tells_the_dm_why_in_words_of_its_own() {
    let refusals = [
        Refusal::NotAnId("instance"),
        Refusal::NotAnInitiative,
        Refusal::NotARoll,
        Refusal::UnknownInstance,
        Refusal::CombatOpen,
        Refusal::NoParticipant,
        Refusal::UnknownCharacter,
        Refusal::MissingInitiative(ELOWEN.to_string()),
        Refusal::RollOfNoNpc,
        Refusal::NotAnEntry,
        Refusal::NotOfItsCombat,
        Refusal::Seated,
    ];
    let said: std::collections::BTreeSet<String> =
        refusals.iter().map(ToString::to_string).collect();
    assert_eq!(said.len(), refusals.len(), "{said:?}");
    assert!(said.iter().all(|why| why.len() > 20), "{said:?}");
    assert!(Refusal::NotAnId("rolls").to_string().starts_with("rolls "));
    assert!(
        Refusal::MissingInitiative(ELOWEN.to_string())
            .to_string()
            .contains(ELOWEN)
    );
}

#[test]
fn an_unknown_instance_alone_is_not_found() {
    assert!(Refusal::UnknownInstance.not_found());
    for refusal in [
        Refusal::NotAnId("instance"),
        Refusal::CombatOpen,
        Refusal::NoParticipant,
        Refusal::MissingInitiative(ELOWEN.to_string()),
        Refusal::NotOfItsCombat,
        Refusal::Seated,
    ] {
        assert!(!refusal.not_found(), "{refusal}");
    }
    assert!(
        Refusal::CombatOpen
            .to_string()
            .starts_with("one-active-combat")
    );
}

/// R105's start, rank 1 first, with the draws given.
fn started() -> Started {
    start(
        json!({ "initiative": { ELOWEN: 18 }, "rolls": { T_BANDIT: 13, T_VESNA: 11 } }),
        &lineup(),
        &mut Scripted::new(&[], &[5, 6, 7]),
    )
    .expect("started")
}

#[test]
fn each_entry_is_a_follow_up_of_the_start_seating_it_in_rank_order() {
    let started = started();
    let seats = follow_ups(&started.entries);
    assert_eq!(seats.len(), 3);
    for (follow_up, entry) in seats.iter().zip(&started.entries) {
        assert_eq!(follow_up.data_capability, DATA_CAPABILITY);
        assert_eq!(follow_up.payload, row(json!({ "entry": entry })));
        assert_eq!(
            follow_up.target_id.as_deref(),
            entry["initiativeEntryId"].as_str()
        );
        // What a seat reads of its entry is the entry itself.
        assert_eq!(seat(&follow_up.payload["entry"]).as_ref(), Ok(entry));
    }
    assert!(follow_ups(&[]).is_empty());
}

#[test]
fn a_seat_is_the_eight_fields_of_an_entry_read_lowercase() {
    let entry = started().entries[1].clone();
    let mut wire = json!(entry);
    wire["initiativeEntryTokenId"] = json!(T_VESNA.to_ascii_uppercase());
    wire["visible"] = json!(false);
    wire["note"] = json!("ignored");
    assert_eq!(seat(&wire), Ok(entry));
    // A PC's entry rolls nothing and may know no DEX.
    let mut pc = json!(started().entries[0]);
    pc["dexScore"] = Value::Null;
    let seated = seat(&pc).expect("a PC's entry");
    assert_eq!(seated["roll"], Value::Null);
    assert_eq!(seated["dexScore"], Value::Null);
    assert_eq!(seated.len(), 8);
}

#[test]
fn a_seat_the_tables_could_never_hold_is_refused() {
    let entry = json!(started().entries[1]);
    for (field, value) in [
        ("initiativeEntryId", json!("ee-1")),
        ("initiativeEntryCombatId", Value::Null),
        ("initiativeEntryTokenId", json!(7)),
        ("roll", json!(0)),
        ("roll", json!(21)),
        ("roll", json!("12")),
        ("initiative", Value::Null),
        ("initiative", json!(40_000)),
        ("initiative", json!(1.5)),
        ("dexScore", json!(40_000)),
        ("tiebreak", Value::Null),
        ("tiebreak", json!("0.5")),
        ("rank", json!(0)),
        ("rank", json!(1.5)),
        ("rank", json!(3_000_000_000_i64)),
    ] {
        let mut wire = entry.clone();
        wire[field] = value.clone();
        assert_eq!(seat(&wire), Err(Refusal::NotAnEntry), "{field}: {value}");
    }
    assert_eq!(seat(&json!("an entry")), Err(Refusal::NotAnEntry));
    // The edges of what the tables hold are an entry.
    let mut edges = entry.clone();
    edges["roll"] = json!(20);
    edges["initiative"] = json!(-32_768);
    edges["dexScore"] = json!(32_767);
    edges["tiebreak"] = json!(0.25);
    edges["rank"] = json!(2_147_483_647_i64);
    assert!(seat(&edges).is_ok());
    edges["roll"] = json!(1);
    edges["rank"] = json!(1);
    assert!(seat(&edges).is_ok());
}

/// The combat `COMBAT` as the start queued it, on the instance.
fn open_combat() -> Row {
    started().combat
}

/// Vesna's token on `instance`.
fn vesna_on(instance: &str) -> Row {
    row(json!({ "tokenId": T_VESNA, "tokenInstanceId": instance, "visible": false }))
}

#[test]
fn a_seat_lands_on_its_open_combat_once_and_on_its_token() {
    let entry = started().entries[1].clone();
    assert_eq!(entry["initiativeEntryTokenId"], T_VESNA);
    let combat = open_combat();
    assert_eq!(
        seatable(&entry, &combat, &vesna_on(INSTANCE), &Row::new()),
        Ok(())
    );
    // Seated once, it is seated no more.
    assert_eq!(
        seatable(&entry, &combat, &vesna_on(INSTANCE), &entry),
        Err(Refusal::Seated)
    );
}

#[test]
fn a_seat_that_is_not_its_combat_s_entry_is_refused() {
    let entry = started().entries[1].clone();
    let combat = open_combat();
    let token = vesna_on(INSTANCE);
    let refused = |entry: &Row, combat: &Row, token: &Row| {
        assert_eq!(
            seatable(entry, combat, token, &Row::new()),
            Err(Refusal::NotOfItsCombat),
            "{entry:?} {combat:?} {token:?}"
        );
    };
    // An entry id the combat does not name for the token.
    let mut forged = entry.clone();
    forged.insert(
        "initiativeEntryId".to_owned(),
        json!(entry_id(COMBAT, T_BANDIT)),
    );
    refused(&forged, &combat, &token);
    // A combat not written, of another id, ended, or of no instance.
    refused(&entry, &Row::new(), &token);
    let mut other = combat.clone();
    other.insert("combatId".to_owned(), json!(combat_id(INSTANCE, 3)));
    refused(&entry, &other, &token);
    let mut ended = combat.clone();
    ended.insert("endedAt".to_owned(), json!(NOW));
    refused(&entry, &ended, &token);
    let mut nowhere = combat.clone();
    nowhere.remove("combatInstanceId");
    refused(&entry, &nowhere, &token);
    // A token not written, another token, or one of another instance.
    refused(&entry, &combat, &Row::new());
    let mut bandit = token.clone();
    bandit.insert("tokenId".to_owned(), json!(T_BANDIT));
    refused(&entry, &combat, &bandit);
    refused(
        &entry,
        &combat,
        &vesna_on("b1000000-0000-4000-8000-000000000002"),
    );
    // An end that is null is none.
    let mut running = combat.clone();
    running.insert("endedAt".to_owned(), Value::Null);
    assert_eq!(seatable(&entry, &running, &token, &Row::new()), Ok(()));
}
