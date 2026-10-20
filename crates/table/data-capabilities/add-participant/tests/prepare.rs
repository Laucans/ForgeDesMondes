//! `request`, `token` and `prepare`: what the DM sends read, the token
//! checked against the instance, and the joining made its entry — an NPC's
//! roll and DEX modifier, a PC's figure — and the moves of the participants
//! it lands ahead of; what a combat forbids refused before anything is
//! queued.

use std::collections::VecDeque;

use add_participant::{
    Character, Dice, Refusal, Request, Row, Seated, Seating, Token, names, prepare, request, token,
};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000003";
const RENFORT: &str = "70000000-0000-4000-8000-000000000009";
const ENTREE: &str = "1e000000-0000-4000-8000-000000000009";

/// A die that rolls what it was told, in order; nothing once it is out.
struct Loaded(VecDeque<i64>);

impl Dice for Loaded {
    fn roll(&mut self, _sides: u32) -> Option<i64> {
        self.0.pop_front()
    }
}

fn dice(rolls: &[i64]) -> Loaded {
    Loaded(rolls.iter().copied().collect())
}

fn wire(value: Value) -> Row {
    let Value::Object(fields) = value else {
        panic!("a payload is an object");
    };
    fields.into_iter().collect()
}

fn asked(initiative: Option<i64>) -> Request {
    Request {
        token: RENFORT.to_string(),
        instance: Some(INSTANCE.to_string()),
        initiative,
    }
}

fn renfort() -> Token {
    Token {
        id: RENFORT.to_string(),
        instance: INSTANCE.to_string(),
        character: BANDIT.to_string(),
    }
}

/// A participant seated at `rank`, its entry held by the engine.
fn seated(name: &str, initiative: i64, dex: Option<i64>, draw: f64, rank: i64) -> Seated {
    Seated {
        id: format!("entree-{name}"),
        token: format!("jeton-{name}"),
        roll: None,
        initiative,
        dex,
        tiebreak: draw,
        rank,
        held: true,
    }
}

fn combat(kind: &str, dex: Option<i64>, entries: Vec<Seated>) -> Seating {
    Seating {
        combat: Some(COMBAT.to_string()),
        entries,
        character: Some(Character {
            kind: kind.to_string(),
            dex,
        }),
    }
}

/// Each move: the entry, and the step of its rank.
fn steps(moves: &[(String, Row)]) -> Vec<(&str, i64)> {
    moves
        .iter()
        .map(|(id, row)| (id.as_str(), row["rank"].as_i64().expect("a rank")))
        .collect()
}

#[test]
fn the_id_is_named_with_or_without_its_version() {
    assert!(names("table.addParticipant"));
    assert!(names("table.addParticipant@1"));
    assert!(!names("table.removeParticipant"));
}

#[test]
fn a_token_is_named_on_the_instance_targeted_or_as_the_target_itself() {
    let upper = RENFORT.to_uppercase();
    let on = request(&wire(json!({ "token": upper })), Some(INSTANCE)).expect("a request");
    assert_eq!(on, asked(None));
    let itself = request(&wire(json!({ "initiative": 17 })), Some(RENFORT)).expect("a request");
    assert_eq!(itself.token, RENFORT);
    assert_eq!(itself.instance, None);
    assert_eq!(itself.initiative, Some(17));
    let null = request(&wire(json!({ "token": RENFORT, "initiative": null })), None).expect("ok");
    assert_eq!(null.initiative, None);
}

#[test]
fn what_is_no_uuid_or_no_whole_number_is_refused() {
    let refused = |payload: Value, target: Option<&str>| {
        request(&wire(payload), target).expect_err("refused")
    };
    assert_eq!(refused(json!({}), None), Refusal::NotAnId("token"));
    assert_eq!(
        refused(json!({ "token": "bandit" }), Some(INSTANCE)),
        Refusal::NotAnId("token")
    );
    assert_eq!(
        refused(json!({ "token": RENFORT }), Some("instance")),
        Refusal::NotAnId("instance")
    );
    for initiative in [json!(12.5), json!("12"), json!(40_000)] {
        assert_eq!(
            refused(json!({ "token": RENFORT, "initiative": initiative }), None),
            Refusal::NotAnInitiative
        );
    }
}

#[test]
fn the_token_must_be_one_the_engine_holds_on_the_instance_named() {
    let row = wire(json!({
        "tokenId": RENFORT,
        "tokenInstanceId": INSTANCE.to_uppercase(),
        "characterId": BANDIT,
    }));
    assert_eq!(token(&asked(None), &row), Ok(renfort()));
    assert_eq!(token(&asked(None), &Row::new()), Err(Refusal::UnknownToken));
    let elsewhere = Request {
        instance: Some(AUTRE_INSTANCE.to_string()),
        ..asked(None)
    };
    assert_eq!(token(&elsewhere, &row), Err(Refusal::UnknownToken));
    let itself = Request {
        instance: None,
        ..asked(None)
    };
    assert_eq!(
        token(&itself, &row).map(|token| token.instance),
        Ok(INSTANCE.to_string())
    );
}

#[test]
fn an_npc_rolls_a_d20_and_adds_its_floored_dex_modifier() {
    let seating = combat("pnj", Some(9), Vec::new());
    let prepared = prepare(
        &asked(None),
        &renfort(),
        &seating,
        &mut dice(&[7, 42]),
        ENTREE,
    )
    .expect("joins");
    assert_eq!(prepared.entry.0, ENTREE);
    assert_eq!(
        prepared.entry.1,
        wire(json!({
            "initiativeEntryId": ENTREE,
            "initiativeEntryCombatId": COMBAT,
            "initiativeEntryTokenId": RENFORT,
            "roll": 7,
            "initiative": 6,
            "dexScore": 9,
            "tiebreak": 41.0,
            "rank": 1,
        }))
    );
    assert!(prepared.moves.is_empty());
}

/// A die that records the faces it was asked for and rolls the highest.
struct Highest(Vec<u32>);

impl Dice for Highest {
    fn roll(&mut self, sides: u32) -> Option<i64> {
        self.0.push(sides);
        Some(i64::from(sides))
    }
}

#[test]
fn the_draw_is_on_the_scale_start_combat_draws_on() {
    let mut highest = Highest(Vec::new());
    let prepared = prepare(
        &asked(Some(10)),
        &renfort(),
        &combat("pj", Some(12), Vec::new()),
        &mut highest,
        ENTREE,
    )
    .expect("joins");
    // A PC rolls nothing but its draw: 0 to 2^31 - 1, as a start draws.
    assert_eq!(highest.0, vec![1 << 31]);
    assert_eq!(prepared.entry.1["tiebreak"], 2_147_483_647.0);
}

#[test]
fn a_pc_rolls_nothing_and_takes_the_dm_s_figure() {
    let seating = combat("pj", Some(18), Vec::new());
    let prepared = prepare(
        &asked(Some(17)),
        &renfort(),
        &seating,
        &mut dice(&[3]),
        ENTREE,
    )
    .expect("joins");
    let row = prepared.entry.1;
    assert_eq!(row["roll"], Value::Null);
    assert_eq!(row["initiative"], 17);
    assert_eq!(row["dexScore"], 18);
    assert_eq!(row["tiebreak"], 2.0);
}

#[test]
fn what_the_combat_or_the_sheet_forbids_is_refused() {
    let refused = |request: &Request, seating: &Seating, rolls: &[i64]| {
        prepare(request, &renfort(), seating, &mut dice(rolls), ENTREE).expect_err("refused")
    };
    let none = Seating {
        combat: None,
        ..combat("pnj", Some(12), Vec::new())
    };
    assert_eq!(
        refused(&asked(None), &none, &[10, 10]),
        Refusal::NoOpenCombat
    );
    let mut already = seated("renfort", 12, Some(12), 1.0, 1);
    already.token = RENFORT.to_uppercase();
    assert_eq!(
        refused(
            &asked(None),
            &combat("pnj", Some(12), vec![already]),
            &[10, 10]
        ),
        Refusal::AlreadyInCombat
    );
    assert_eq!(
        refused(&asked(None), &combat("lieu", None, Vec::new()), &[10, 10]),
        Refusal::UnknownCharacter
    );
    let no_sheet = Seating {
        character: None,
        ..combat("pnj", Some(12), Vec::new())
    };
    assert_eq!(
        refused(&asked(None), &no_sheet, &[10, 10]),
        Refusal::UnknownCharacter
    );
    assert_eq!(
        refused(&asked(None), &combat("pj", Some(12), Vec::new()), &[10]),
        Refusal::PcInitiative
    );
    assert_eq!(
        refused(
            &asked(Some(15)),
            &combat("pnj", Some(12), Vec::new()),
            &[10, 10]
        ),
        Refusal::NpcRolls
    );
    assert_eq!(
        refused(&asked(None), &combat("pnj", None, Vec::new()), &[10, 10]),
        Refusal::NoDex
    );
    assert_eq!(
        refused(&asked(None), &combat("pnj", Some(12), Vec::new()), &[10]),
        Refusal::NoRandomness
    );
}

#[test]
fn it_lands_by_initiative_and_moves_down_the_ones_it_lands_ahead_of() {
    let entries = vec![
        seated("elowen", 18, Some(18), 1.0, 1),
        seated("bandit-1", 12, Some(12), 1.0, 2),
        seated("kael", 8, Some(14), 1.0, 3),
    ];
    // 12 + 2 (DEX 14) = 14: after Elowen, ahead of Bandit 1 and Kael.
    let prepared = prepare(
        &asked(None),
        &renfort(),
        &combat("pnj", Some(14), entries),
        &mut dice(&[12, 50]),
        ENTREE,
    )
    .expect("joins");
    assert_eq!(prepared.entry.1["rank"], 2);
    assert_eq!(
        steps(&prepared.moves),
        [("entree-kael", 1), ("entree-bandit-1", 1)]
    );
    // A step names its entry, its combat and its token, and nothing else.
    assert_eq!(
        prepared.moves[1].1,
        wire(json!({
            "initiativeEntryId": "entree-bandit-1",
            "initiativeEntryCombatId": COMBAT,
            "initiativeEntryTokenId": "jeton-bandit-1",
            "rank": 1,
        }))
    );
}

#[test]
fn at_equal_initiative_dex_then_the_draw_decide_and_a_full_tie_goes_after() {
    let entries = vec![
        seated("dex-16", 14, Some(16), 5.0, 1),
        seated("dex-12-draw-9", 14, Some(12), 9.0, 2),
        seated("dex-12-draw-2", 14, Some(12), 2.0, 3),
        seated("no-dex", 14, None, 99.0, 4),
    ];
    let joining = |draw: i64| {
        let prepared = prepare(
            &asked(Some(14)),
            &renfort(),
            &combat("pj", Some(12), entries.clone()),
            // The draw is the roll less one.
            &mut dice(&[draw + 1]),
            ENTREE,
        )
        .expect("joins");
        prepared.entry.1["rank"].as_i64().expect("a rank")
    };
    // DEX 12 at 14: after DEX 16, and among the DEX 12 by its draw — before
    // every one with no DEX.
    assert_eq!(joining(10), 2);
    assert_eq!(joining(5), 3);
    assert_eq!(joining(1), 4);
    // A tie on all three keeps the order given: the newcomer last of them.
    assert_eq!(joining(9), 3);
    assert_eq!(joining(2), 4);
    // With no DEX itself, it goes after every known one, whatever its draw.
    let no_dex = prepare(
        &asked(Some(14)),
        &renfort(),
        &combat("pj", None, entries.clone()),
        &mut dice(&[101]),
        ENTREE,
    )
    .expect("joins");
    assert_eq!(no_dex.entry.1["rank"], 4);
    assert_eq!(no_dex.entry.1["dexScore"], Value::Null);
}

#[test]
fn a_gap_in_the_ranks_takes_it_in_and_the_last_place_moves_no_one() {
    let entries = vec![
        seated("premier", 20, Some(10), 1.0, 1),
        seated("deuxieme", 5, Some(10), 1.0, 3),
    ];
    let seating = combat("pj", Some(10), entries);
    let middle = prepare(
        &asked(Some(10)),
        &renfort(),
        &seating,
        &mut dice(&[1]),
        ENTREE,
    )
    .expect("joins");
    assert_eq!(middle.entry.1["rank"], 2);
    assert!(middle.moves.is_empty());
    let last = prepare(
        &asked(Some(1)),
        &renfort(),
        &seating,
        &mut dice(&[1]),
        ENTREE,
    )
    .expect("joins");
    assert_eq!(last.entry.1["rank"], 4);
    assert!(last.moves.is_empty());
}

#[test]
fn a_tie_in_ranks_is_read_by_id_as_the_carousel_reads_it_and_parted() {
    let entries = vec![
        seated("x", 30, Some(10), 1.0, 1),
        seated("b", 10, Some(10), 1.0, 2),
        seated("a", 10, Some(10), 1.0, 2),
        seated("c", 5, Some(10), 1.0, 3),
    ];
    let prepared = prepare(
        &asked(Some(20)),
        &renfort(),
        &combat("pj", Some(10), entries),
        &mut dice(&[1]),
        ENTREE,
    )
    .expect("joins");
    assert_eq!(prepared.entry.1["rank"], 2);
    // a before b, by id, as the carousel orders them: a to 3, b to 4, c to 5.
    assert_eq!(
        steps(&prepared.moves),
        [("entree-c", 2), ("entree-b", 2), ("entree-a", 1)]
    );
}

#[test]
fn after_the_dm_reordered_it_lands_after_the_last_one_ranked_before_it() {
    // The DM moved the slow one up: the newcomer at 15 goes after the one at
    // 18, not ahead of everyone because it beats the first.
    let entries = vec![
        seated("lent", 5, Some(10), 1.0, 1),
        seated("rapide", 18, Some(10), 1.0, 2),
        seated("moyen", 12, Some(10), 1.0, 3),
    ];
    let prepared = prepare(
        &asked(Some(15)),
        &renfort(),
        &combat("pj", Some(10), entries),
        &mut dice(&[1]),
        ENTREE,
    )
    .expect("joins");
    assert_eq!(prepared.entry.1["rank"], 3);
    assert_eq!(steps(&prepared.moves), [("entree-moyen", 1)]);
}

#[test]
fn an_entry_only_the_tables_hold_is_moved_whole_to_its_new_rank() {
    let mut tabled = seated("bandit-2", 9, Some(12), 4.0, 2);
    tabled.held = false;
    tabled.roll = Some(8);
    let entries = vec![seated("elowen", 18, Some(18), 1.0, 1), tabled];
    let prepared = prepare(
        &asked(Some(10)),
        &renfort(),
        &combat("pj", Some(10), entries),
        &mut dice(&[1]),
        ENTREE,
    )
    .expect("joins");
    assert_eq!(prepared.entry.1["rank"], 2);
    assert_eq!(
        prepared.moves,
        [(
            "entree-bandit-2".to_string(),
            wire(json!({
                "initiativeEntryId": "entree-bandit-2",
                "initiativeEntryCombatId": COMBAT,
                "initiativeEntryTokenId": "jeton-bandit-2",
                "roll": 8,
                "initiative": 9,
                "dexScore": 12,
                "tiebreak": 4.0,
                "rank": 3,
            }))
        )]
    );
}

#[test]
fn the_moves_never_leave_two_entries_at_one_rank_whichever_one_fails_after() {
    // No gap in the ranks: the whole order shifts down.
    let entries = vec![
        seated("a", 10, Some(10), 1.0, 1),
        seated("b", 9, Some(10), 1.0, 2),
        seated("c", 8, Some(10), 1.0, 3),
        seated("d", 7, Some(10), 1.0, 4),
    ];
    let prepared = prepare(
        &asked(Some(20)),
        &renfort(),
        &combat("pj", Some(10), entries.clone()),
        &mut dice(&[1]),
        ENTREE,
    )
    .expect("joins");
    assert_eq!(prepared.moves.len(), 4);
    let mut ranks: Vec<(String, i64)> = entries.iter().map(|e| (e.id.clone(), e.rank)).collect();
    for (moved, step) in &prepared.moves {
        let entry = ranks
            .iter_mut()
            .find(|(id, _)| id == moved)
            .expect("seated");
        entry.1 += step["rank"].as_i64().expect("a step");
        let mut seen: Vec<i64> = ranks.iter().map(|(_, rank)| *rank).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), ranks.len(), "two at one rank after {moved}");
    }
}
