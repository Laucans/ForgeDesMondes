//! `table.startCombat`'s judgement before a command is queued: what the DM
//! sends, read ([`request`]), and made the combat's row and its entries'
//! against the instance's lineup ([`prepare`]), with scripted dice.

use std::collections::VecDeque;

use serde_json::{Value, json};
use start_combat::{
    DEFAULT_DEX, Dice, Fresh, Lineup, Participant, Prepared, Refusal, Request, Row, names,
    partition, prepare, request,
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

fn ids(n: usize) -> Vec<String> {
    (1..=n)
        .map(|i| format!("ee000000-0000-4000-8000-{i:012}"))
        .collect()
}

/// The start `payload` asks for on the instance, against `lineup`.
fn start(payload: Value, lineup: &Lineup, dice: &mut Scripted) -> Result<Prepared, Refusal> {
    let request = request(&row(payload), Some(INSTANCE)).expect("a request");
    let entries = ids(lineup.tokens.len());
    let fresh = Fresh {
        combat: COMBAT,
        entries: &entries,
        now: NOW,
    };
    prepare(&request, lineup, fresh, dice)
}

/// The entries' tokens, rank 1 first.
fn order(prepared: &Prepared) -> Vec<&str> {
    prepared
        .entries
        .iter()
        .map(|(_, entry)| entry["initiativeEntryTokenId"].as_str().expect("a token"))
        .collect()
}

fn entry_of<'a>(prepared: &'a Prepared, token: &str) -> &'a Row {
    &prepared
        .entries
        .iter()
        .find(|(_, entry)| entry["initiativeEntryTokenId"] == token)
        .expect("an entry")
        .1
}

#[test]
fn names_this_data_capability_at_any_version() {
    assert!(names("table.startCombat"));
    assert!(names("table.startCombat@1"));
    assert!(!names("table.nextTurn"));
    assert_eq!(partition(COMBAT), format!("BattlemapInstance/{COMBAT}"));
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
        .flat_map(|(_, entry)| entry.keys())
        .chain(prepared.combat.1.keys())
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
    let ranks: Vec<&Value> = prepared.entries.iter().map(|(_, e)| &e["rank"]).collect();
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
    assert!(
        prepared
            .entries
            .iter()
            .all(|(_, e)| !e.contains_key("visible"))
    );
}

#[test]
fn next_turn_the_combat_opens_at_round_1_on_rank_1() {
    let prepared = start(
        json!({ "initiative": { ELOWEN: 18 }, "rolls": { T_BANDIT: 13, T_VESNA: 11 } }),
        &lineup(),
        &mut Scripted::default(),
    )
    .expect("started");
    let (id, combat) = &prepared.combat;
    assert_eq!(id, COMBAT);
    let (first, _) = &prepared.entries[0];
    assert_eq!(
        combat,
        &row(json!({
            "combatId": COMBAT,
            "combatInstanceId": INSTANCE,
            "round": 1,
            "currentEntryId": first,
            "startedAt": NOW,
        }))
    );
    for (_, entry) in &prepared.entries {
        assert_eq!(entry["initiativeEntryCombatId"], COMBAT);
    }
    assert_eq!(prepared.entries[0].1["initiativeEntryTokenId"], T_ELOWEN);
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
