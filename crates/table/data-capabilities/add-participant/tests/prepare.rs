//! What `table.addParticipant` makes of a request, without a database: the
//! request read off the wire, the token's character, the newcomer's entry
//! and the participants it lands ahead of (`npc-initiative-bounds`,
//! `initiative-tiebreak`), the moves its follow-ups are, and when a move
//! makes room.

use std::collections::VecDeque;

use add_participant::{
    Character, DATA_CAPABILITY, DEFAULT_DEX, Dice, Joined, Refusal, Request, Row, Seated, Seating,
    Standing, SystemDice, character_of, entry_id, follow_ups, order, partition, placed, prepare,
    request, shiftable,
};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const TOKEN: &str = "70000000-0000-4000-8000-0000000000aa";
const CHARACTER: &str = "e1000000-0000-4000-8000-0000000000aa";

/// Dice that throw what the test scripts, in order.
#[derive(Default)]
struct Scripted {
    d20s: VecDeque<i64>,
    draws: VecDeque<i64>,
}

impl Scripted {
    fn new(d20s: &[i64], draws: &[i64]) -> Self {
        Self {
            d20s: d20s.iter().copied().collect(),
            draws: draws.iter().copied().collect(),
        }
    }
}

impl Dice for Scripted {
    fn d20(&mut self) -> i64 {
        self.d20s.pop_front().expect("a d20 scripted")
    }

    fn draw(&mut self) -> i64 {
        self.draws.pop_front().expect("a draw scripted")
    }
}

fn row(value: Value) -> Row {
    serde_json::from_value(value).expect("an object")
}

fn joining(initiative: Option<i64>) -> Request {
    Request {
        instance: INSTANCE.to_owned(),
        token: TOKEN.to_owned(),
        initiative,
    }
}

/// A participant `id`, of token `token`, at `rank`.
fn seated(id: &str, initiative: i64, dex: Option<i64>, draw: f64, rank: i64) -> Seated {
    Seated {
        id: id.to_owned(),
        token: format!("token-of-{id}"),
        initiative,
        dex,
        tiebreak: draw,
        rank,
    }
}

/// R105's combat: Elowen at 18, Vesna at 14 DEX 16, the Bandit at 14 DEX 12.
fn r105() -> Vec<Seated> {
    vec![
        seated("elowen", 18, Some(14), 1.0, 1),
        seated("vesna", 14, Some(16), 99.0, 2),
        seated("bandit", 14, Some(12), 2.0, 3),
    ]
}

fn seating(kind: &str, dex: Option<i64>) -> Seating {
    Seating {
        combat: Some(COMBAT.to_owned()),
        entries: r105(),
        character: Some(Character {
            kind: kind.to_owned(),
            dex,
        }),
    }
}

fn standing(initiative: i64, dex: Option<i64>, draw: f64) -> Standing {
    Standing {
        initiative,
        dex,
        draw,
    }
}

// --- request ---

#[test]
fn request_reads_the_instance_off_the_target_else_the_payload_ids_lowercase() {
    let wire = row(json!({
        "token": format!("  {}  ", TOKEN.to_uppercase()),
        "instance": "b2000000-0000-4000-8000-000000000002",
    }));
    let by_target = request(&wire, Some(&INSTANCE.to_uppercase())).expect("a request");
    assert_eq!(by_target, joining(None));
    let by_payload = request(&wire, None).expect("a request");
    assert_eq!(by_payload.instance, "b2000000-0000-4000-8000-000000000002");
    assert_eq!(by_payload.token, TOKEN);
}

#[test]
fn request_takes_a_pc_s_initiative_and_null_as_none() {
    let wire = row(json!({ "token": TOKEN, "initiative": 17 }));
    assert_eq!(
        request(&wire, Some(INSTANCE))
            .expect("a request")
            .initiative,
        Some(17)
    );
    let wire = row(json!({ "token": TOKEN, "initiative": null }));
    assert_eq!(
        request(&wire, Some(INSTANCE))
            .expect("a request")
            .initiative,
        None
    );
    // The smallint's bounds hold.
    for held in [-32768, 32767] {
        let wire = row(json!({ "token": TOKEN, "initiative": held }));
        assert_eq!(
            request(&wire, Some(INSTANCE)).expect("held").initiative,
            Some(held)
        );
    }
}

#[test]
fn request_refuses_what_names_no_id_or_no_whole_initiative() {
    let no_instance = row(json!({ "token": TOKEN }));
    assert_eq!(
        request(&no_instance, None),
        Err(Refusal::NotAnId("instance"))
    );
    assert_eq!(
        request(&no_instance, Some("une taverne")),
        Err(Refusal::NotAnId("instance"))
    );
    for token in [json!(null), json!("Bandit"), json!(7)] {
        assert_eq!(
            request(&row(json!({ "token": token })), Some(INSTANCE)),
            Err(Refusal::NotAnId("token"))
        );
    }
    assert_eq!(
        request(&row(json!({})), Some(INSTANCE)),
        Err(Refusal::NotAnId("token"))
    );
    for initiative in [json!(12.5), json!("12"), json!(32768), json!(-32769)] {
        assert_eq!(
            request(
                &row(json!({ "token": TOKEN, "initiative": initiative })),
                Some(INSTANCE)
            ),
            Err(Refusal::NotAnInitiative)
        );
    }
}

// --- the token ---

#[test]
fn character_of_a_token_on_the_instance_is_its_character_lowercase() {
    let token = row(json!({
        "tokenId": TOKEN.to_uppercase(),
        "tokenInstanceId": INSTANCE,
        "characterId": CHARACTER.to_uppercase(),
    }));
    assert_eq!(
        character_of(&joining(None), &token),
        Ok(CHARACTER.to_owned())
    );
}

#[test]
fn character_of_no_token_of_the_instance_is_an_unknown_token_and_no_character_unknown() {
    let on = |instance: &str, character: Value| {
        row(json!({
            "tokenId": TOKEN,
            "tokenInstanceId": instance,
            "characterId": character,
        }))
    };
    // On the instance, but of no character.
    assert_eq!(
        character_of(&joining(None), &on(INSTANCE, Value::Null)),
        Err(Refusal::UnknownCharacter)
    );
    for token in [
        Row::new(),
        on("b2000000-0000-4000-8000-000000000002", json!(CHARACTER)),
        row(json!({
            "tokenId": "70000000-0000-4000-8000-0000000000bb",
            "tokenInstanceId": INSTANCE,
            "characterId": CHARACTER,
        })),
    ] {
        assert_eq!(
            character_of(&joining(None), &token),
            Err(Refusal::UnknownToken),
            "{token:?}"
        );
    }
}

// --- the newcomer ---

#[test]
fn npc_initiative_bounds_an_npc_rolls_a_d20_plus_its_dex_modifier_and_lands_by_rank() {
    // DEX 14: +2. A 13 makes 15, between Elowen (18) and Vesna (14).
    let Joined { entry, moves } = prepare(
        &joining(None),
        &seating("pnj", Some(14)),
        &mut Scripted::new(&[13], &[42]),
    )
    .expect("joined");
    assert_eq!(
        entry,
        row(json!({
            "initiativeEntryId": entry_id(COMBAT, TOKEN),
            "initiativeEntryCombatId": COMBAT,
            "initiativeEntryTokenId": TOKEN,
            "roll": 13,
            "initiative": 15,
            "dexScore": 14,
            "tiebreak": 42,
            "rank": 2,
        }))
    );
    assert_eq!(moves, ["vesna", "bandit"]);
}

#[test]
fn npc_initiative_bounds_an_npc_with_no_dex_rolls_with_the_default_one() {
    let Joined { entry, .. } = prepare(
        &joining(None),
        &seating("pnj", None),
        &mut Scripted::new(&[1], &[0]),
    )
    .expect("joined");
    assert_eq!(DEFAULT_DEX, 10);
    assert_eq!(entry["roll"], 1);
    assert_eq!(entry["initiative"], 1);
    assert_eq!(entry["dexScore"], DEFAULT_DEX);
    // Last: nobody moves.
    assert_eq!(entry["rank"], 4);
}

#[test]
fn npc_initiative_bounds_a_pc_rolls_nothing_and_takes_the_dm_s_figure() {
    // 14 like Vesna and the Bandit, DEX 10 below both: last, no draw needed.
    let Joined { entry, moves } = prepare(
        &joining(Some(14)),
        &seating("pj", Some(10)),
        &mut Scripted::new(&[], &[7]),
    )
    .expect("joined");
    assert_eq!(entry["roll"], Value::Null);
    assert_eq!(entry["initiative"], 14);
    assert_eq!(entry["dexScore"], 10);
    assert_eq!(entry["tiebreak"], 7);
    assert_eq!(entry["rank"], 4);
    assert!(moves.is_empty());
    // A PC whose sheet gives no DEX has none.
    let Joined { entry, .. } = prepare(
        &joining(Some(30)),
        &seating("pj", None),
        &mut Scripted::new(&[], &[7]),
    )
    .expect("joined");
    assert_eq!(entry["dexScore"], Value::Null);
    assert_eq!(entry["rank"], 1);
}

#[test]
fn npc_initiative_bounds_a_pc_needs_the_dm_s_figure_and_an_npc_takes_none() {
    let mut dice = Scripted::new(&[10], &[1]);
    assert_eq!(
        prepare(&joining(None), &seating("pj", Some(12)), &mut dice),
        Err(Refusal::PcInitiative)
    );
    assert_eq!(
        prepare(&joining(Some(12)), &seating("pnj", Some(12)), &mut dice),
        Err(Refusal::NpcRolls)
    );
}

#[test]
fn a_joining_with_no_open_combat_is_refused() {
    let mut no_combat = seating("pnj", Some(12));
    no_combat.combat = None;
    assert_eq!(
        prepare(&joining(None), &no_combat, &mut Scripted::new(&[10], &[1])),
        Err(Refusal::NoOpenCombat)
    );
}

#[test]
fn a_token_already_in_the_combat_is_refused() {
    let mut seating = seating("pnj", Some(12));
    seating.entries[1].token = TOKEN.to_owned();
    assert_eq!(
        prepare(&joining(None), &seating, &mut Scripted::new(&[10], &[1])),
        Err(Refusal::AlreadyInCombat)
    );
}

#[test]
fn a_token_of_no_pc_nor_npc_is_refused() {
    let mut nobody = seating("pnj", Some(12));
    nobody.character = None;
    let mut place = seating("lieu", None);
    place.entries.clear();
    for seating in [nobody, place] {
        assert_eq!(
            prepare(&joining(Some(3)), &seating, &mut Scripted::new(&[10], &[1])),
            Err(Refusal::UnknownCharacter)
        );
    }
}

// --- the order ---

#[test]
fn initiative_tiebreak_order_reads_initiative_then_dex_unknown_last_then_draw() {
    use std::cmp::Ordering::{Equal, Greater, Less};
    let base = standing(14, Some(12), 5.0);
    assert_eq!(order(&standing(15, Some(1), 0.0), &base), Less);
    assert_eq!(order(&standing(13, Some(20), 99.0), &base), Greater);
    assert_eq!(order(&standing(14, Some(13), 0.0), &base), Less);
    assert_eq!(order(&standing(14, Some(11), 99.0), &base), Greater);
    assert_eq!(order(&standing(14, None, 99.0), &base), Greater);
    assert_eq!(order(&base, &standing(14, None, 99.0)), Less);
    assert_eq!(order(&standing(14, Some(12), 6.0), &base), Less);
    assert_eq!(order(&standing(14, Some(12), 4.0), &base), Greater);
    assert_eq!(order(&base, &base), Equal);
}

#[test]
fn initiative_tiebreak_a_newcomer_tied_on_initiative_is_settled_by_dex_then_draw() {
    // At 14 with DEX 14: after Vesna (16), before the Bandit (12).
    assert_eq!(
        placed(&r105(), &standing(14, Some(14), 0.0)),
        (3, vec!["bandit".to_owned()])
    );
    // At 14 with DEX 16 like Vesna: the draw, 99 against Vesna's.
    assert_eq!(
        placed(&r105(), &standing(14, Some(16), 100.0)),
        (2, vec!["vesna".to_owned(), "bandit".to_owned()])
    );
    assert_eq!(
        placed(&r105(), &standing(14, Some(16), 98.0)),
        (3, vec!["bandit".to_owned()])
    );
    // A full tie goes after.
    assert_eq!(
        placed(&r105(), &standing(14, Some(16), 99.0)),
        (3, vec!["bandit".to_owned()])
    );
}

#[test]
fn initiative_tiebreak_first_and_last_and_alone() {
    assert_eq!(
        placed(&r105(), &standing(25, None, 0.0)),
        (
            1,
            vec!["elowen".to_owned(), "vesna".to_owned(), "bandit".to_owned()]
        )
    );
    assert_eq!(placed(&r105(), &standing(-4, Some(1), 0.0)), (4, vec![]));
    assert_eq!(placed(&[], &standing(10, Some(10), 0.0)), (1, vec![]));
}

#[test]
fn initiative_tiebreak_the_order_read_is_the_ranks_not_the_standings() {
    // The DM moved the Bandit first: the carousel reads Bandit, Elowen,
    // Vesna. At 16 the newcomer goes after the last that ranks before it —
    // Elowen —, so before Vesna alone.
    let reordered = vec![
        seated("bandit", 14, Some(12), 2.0, 1),
        seated("elowen", 18, Some(14), 1.0, 2),
        seated("vesna", 14, Some(16), 99.0, 3),
    ];
    assert_eq!(
        placed(&reordered, &standing(16, None, 0.0)),
        (3, vec!["vesna".to_owned()])
    );
    // A rank shared by two is read by id: the newcomer goes after `a`, the
    // last that ranks before it, and the rank after `a`'s is free.
    let shared = vec![
        seated("b", 10, None, 0.0, 1),
        seated("a", 20, None, 0.0, 1),
        seated("c", 5, None, 0.0, 2),
    ];
    assert_eq!(
        placed(&shared, &standing(25, None, 0.0)),
        (1, vec!["a".to_owned(), "b".to_owned(), "c".to_owned()])
    );
    assert_eq!(
        placed(&shared, &standing(15, None, 0.0)),
        (2, vec!["c".to_owned()])
    );
}

#[test]
fn initiative_tiebreak_the_moves_stop_at_a_gap_in_the_ranks() {
    let gapped = vec![
        seated("first", 20, None, 0.0, 1),
        seated("second", 10, None, 0.0, 2),
        seated("third", 5, None, 0.0, 4),
    ];
    assert_eq!(
        placed(&gapped, &standing(15, None, 0.0)),
        (2, vec!["second".to_owned()])
    );
    // The first at rank 3: rank 1 is free, nobody moves.
    let late = vec![seated("only", 10, None, 0.0, 3)];
    assert_eq!(placed(&late, &standing(15, None, 0.0)), (1, vec![]));
    // Two at one rank both move, and so does the one after them.
    let doubled = vec![
        seated("first", 20, None, 0.0, 1),
        seated("x", 10, None, 0.0, 2),
        seated("y", 9, None, 0.0, 2),
        seated("z", 8, None, 0.0, 3),
    ];
    assert_eq!(
        placed(&doubled, &standing(15, None, 0.0)),
        (2, vec!["x".to_owned(), "y".to_owned(), "z".to_owned()])
    );
    // One sharing the rank of the participant before the newcomer stays.
    let shared = vec![
        seated("first", 20, None, 0.0, 1),
        seated("second", 10, None, 0.0, 1),
        seated("third", 5, None, 0.0, 2),
    ];
    assert_eq!(
        placed(&shared, &standing(15, None, 0.0)),
        (2, vec!["third".to_owned()])
    );
}

// --- the moves ---

#[test]
fn follow_ups_move_each_entry_for_the_newcomer_in_rank_order() {
    let newcomer = entry_id(COMBAT, TOKEN);
    let moves = vec!["vesna".to_owned(), "bandit".to_owned()];
    let submissions = follow_ups(&newcomer, &moves);
    assert_eq!(submissions.len(), 2);
    for (submission, moved) in submissions.iter().zip(&moves) {
        assert_eq!(submission.data_capability, DATA_CAPABILITY);
        assert_eq!(submission.target_id.as_deref(), Some(moved.as_str()));
        assert_eq!(
            submission.payload,
            row(json!({ "shift": moved, "newcomer": newcomer }))
        );
    }
    assert!(follow_ups(&newcomer, &[]).is_empty());
}

/// An entry of `combat` for `id`, at `rank`.
fn entry(id: &str, combat: &str, rank: i64) -> Row {
    row(json!({
        "initiativeEntryId": id,
        "initiativeEntryCombatId": combat,
        "initiativeEntryTokenId": "70000000-0000-4000-8000-000000000001",
        "roll": null,
        "initiative": 12,
        "dexScore": null,
        "tiebreak": 3,
        "rank": rank,
    }))
}

const MOVED: &str = "a0000000-0000-4000-8000-000000000001";
const NEWCOMER: &str = "a0000000-0000-4000-8000-000000000002";

fn combat() -> Row {
    row(json!({ "combatId": COMBAT, "combatInstanceId": INSTANCE, "round": 1 }))
}

#[test]
fn initiative_tiebreak_a_move_makes_room_for_the_newcomer_holding_its_rank() {
    let moved = entry(MOVED, COMBAT, 2);
    let newcomer = entry(NEWCOMER, COMBAT, 2);
    let others = [Seated::of(&newcomer).expect("an entry")];
    assert_eq!(shiftable(&moved, &newcomer, &combat(), &others), Ok(()));
    // Or for the one moved before it, the newcomer above.
    let pushed =
        Seated::of(&entry("a0000000-0000-4000-8000-000000000003", COMBAT, 3)).expect("an entry");
    assert_eq!(
        shiftable(&entry(MOVED, COMBAT, 3), &newcomer, &combat(), &[pushed]),
        Ok(())
    );
    // An open combat whose end is null.
    let mut open = combat();
    open.insert("endedAt".to_owned(), Value::Null);
    assert_eq!(shiftable(&moved, &newcomer, &open, &others), Ok(()));
}

#[test]
fn initiative_tiebreak_a_move_that_makes_no_room_is_refused() {
    let moved = entry(MOVED, COMBAT, 2);
    let newcomer = entry(NEWCOMER, COMBAT, 2);
    let others = [Seated::of(&newcomer).expect("an entry")];
    let refused = |shifted: &Row, newcomer: &Row, combat: &Row, others: &[Seated]| {
        assert_eq!(
            shiftable(shifted, newcomer, combat, others),
            Err(Refusal::NotAShift),
            "{shifted:?} {newcomer:?} {combat:?} {others:?}"
        );
    };
    // Nobody else at its rank: the newcomer already has its place.
    refused(&moved, &newcomer, &combat(), &[]);
    let lower = [Seated::of(&entry(NEWCOMER, COMBAT, 1)).expect("an entry")];
    refused(&moved, &newcomer, &combat(), &lower);
    // Itself at its rank is no one else.
    let itself = [Seated::of(&moved).expect("an entry")];
    refused(&moved, &newcomer, &combat(), &itself);
    // The newcomer ranked below it.
    refused(&moved, &entry(NEWCOMER, COMBAT, 3), &combat(), &others);
    // The newcomer is the entry moved.
    refused(&moved, &entry(MOVED, COMBAT, 2), &combat(), &others);
    // The newcomer of another combat.
    let other_combat = "c0000000-0000-4000-8000-000000000002";
    refused(
        &moved,
        &entry(NEWCOMER, other_combat, 2),
        &combat(),
        &others,
    );
    // The combat row is another's, or ended.
    let mut another = combat();
    another.insert("combatId".to_owned(), json!(other_combat));
    refused(&moved, &newcomer, &another, &others);
    let mut ended = combat();
    ended.insert("endedAt".to_owned(), json!("2026-10-22T10:00:00Z"));
    refused(&moved, &newcomer, &ended, &others);
    // Rows that are no entries.
    refused(&Row::new(), &newcomer, &combat(), &others);
    refused(&moved, &Row::new(), &combat(), &others);
    let mut no_combat = moved.clone();
    no_combat.insert("initiativeEntryCombatId".to_owned(), json!("aucun"));
    refused(&no_combat, &entry(NEWCOMER, "aucun", 2), &combat(), &others);
}

#[test]
fn seated_of_reads_an_entry_and_nothing_else() {
    let mut held = entry(&MOVED.to_uppercase(), COMBAT, 4);
    held.insert("dexScore".to_owned(), json!(13));
    held.insert("tiebreak".to_owned(), json!(2.5));
    assert_eq!(
        Seated::of(&held),
        Some(Seated {
            id: MOVED.to_owned(),
            token: "70000000-0000-4000-8000-000000000001".to_owned(),
            initiative: 12,
            dex: Some(13),
            tiebreak: 2.5,
            rank: 4,
        })
    );
    for field in [
        "initiativeEntryId",
        "initiativeEntryTokenId",
        "initiative",
        "tiebreak",
        "rank",
    ] {
        let mut missing = held.clone();
        missing.remove(field);
        assert_eq!(Seated::of(&missing), None, "{field}");
    }
}

// --- the rest ---

#[test]
fn partition_and_entry_id_are_the_ones_table_start_combat_writes() {
    assert_eq!(partition(MOVED), format!("BattlemapInstance/{MOVED}"));
    // The id table.startCombat derives for this token in this combat.
    assert_eq!(
        entry_id(COMBAT, TOKEN),
        "0d3bf4d5-7a69-595c-ac4c-04148402b851"
    );
    assert_ne!(entry_id(COMBAT, TOKEN), entry_id(COMBAT, MOVED));
}

#[test]
fn the_system_dice_throw_a_d20_and_draw_32_bits() {
    let mut dice = SystemDice;
    let throws: Vec<i64> = (0..4000).map(|_| dice.d20()).collect();
    assert!(throws.iter().all(|roll| (1..=20).contains(roll)));
    assert!(throws.contains(&1) && throws.contains(&20));
    let draws: Vec<i64> = (0..64).map(|_| dice.draw()).collect();
    assert!(
        draws
            .iter()
            .all(|draw| (0..=i64::from(u32::MAX)).contains(draw))
    );
    assert!(draws.iter().any(|draw| *draw != draws[0]));
}

#[test]
fn a_refusal_says_why_and_only_an_unknown_token_is_not_found() {
    let all = [
        Refusal::NotAnId("token"),
        Refusal::NotAnInitiative,
        Refusal::UnknownToken,
        Refusal::NoOpenCombat,
        Refusal::AlreadyInCombat,
        Refusal::UnknownCharacter,
        Refusal::PcInitiative,
        Refusal::NpcRolls,
        Refusal::NotAShift,
    ];
    let messages: Vec<String> = all.iter().map(ToString::to_string).collect();
    for (refusal, message) in all.iter().zip(&messages) {
        assert!(!message.is_empty(), "{refusal:?}");
        assert_eq!(refusal.not_found(), *refusal == Refusal::UnknownToken);
    }
    let mut distinct = messages.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), messages.len());
    assert!(messages[0].contains("token"));
    assert!(messages[6].starts_with("npc-initiative-bounds"));
    assert!(messages[7].starts_with("npc-initiative-bounds"));
}
