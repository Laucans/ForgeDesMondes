//! table.nextTurn reads who is skipped from `initiative_order_view`, the view
//! the initiative carousel shows, over PostgreSQL (`DATABASE_URL`): a scratch
//! database of its own, migrated as the server migrates it, dropped when the
//! test ends (`platform_test_kit::Scratch`). A combat of five participants on
//! the tavern's instance — two Bandits alive, a hidden one alive, a dead one
//! and Valombre's PC, who tracks no HP — is pressed on again and again: the
//! turn lands exactly where the view's `skipped` column lets it, and the round
//! goes up at each lap.

use harness_dataguard::plugins::RefusalKind;
use platform_test_kit::{Bench, Scratch, Served, file, planter, refused, uuid};
use serde_json::{Value, json};

const NEXT: &str = "table.nextTurn";
const DROP_INSTANCE: &str = "test.dropInstance";
const DROP_TOKEN: &str = "test.dropToken";
const OPEN_COMBAT: &str = "test.openCombat";
const SEAT: &str = "test.seat";
const WOUND: &str = "test.wound";
const HIDE: &str = "test.hide";

fn instance() -> String {
    uuid(0xb1, 1)
}
fn campaign() -> String {
    uuid(0xc1, 1)
}
fn session() -> String {
    uuid(0x5e, 1)
}
fn scene() -> String {
    uuid(0x5c, 1)
}
fn media() -> String {
    uuid(0xd1, 1)
}
fn bandit() -> String {
    uuid(0xe1, 1)
}
fn hero() -> String {
    uuid(0xe1, 2)
}
fn combat() -> String {
    uuid(0xc0, 1)
}
fn token(number: u64) -> String {
    uuid(0x70, number)
}
fn entry(number: u64) -> String {
    uuid(0xee, number)
}

/// A database holding Valombre, its session, scene and battlemap that an
/// instance stands on, a Bandit (an NPC) and a hero (a PC).
async fn seeded(label: &str) -> Scratch {
    let scratch = Scratch::new(label).await;
    scratch
        .seed(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES
                 ('{campaign}', 'Les Cendres de Valombre', 'campagne');
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                 ('{session}', '{campaign}', 1, 'L''embuscade du Gué', 'live', false);
             INSERT INTO scene (id, session_id, position, title) VALUES
                 ('{scene}', '{session}', 1, 'Le gué');
             INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES
                 ('{media}', '{campaign}', 'battlemap', 'Le Gué des Corbeaux', 'media/gue.png', 960, 640);
             INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) VALUES
                 ('{media}', 30, 12, 8, 'pointe', 'text', 0.4);
             INSERT INTO entity (id, campaign_id, type, name) VALUES
                 ('{bandit}', '{campaign}', 'pnj', 'Bandit'),
                 ('{hero}', '{campaign}', 'pj', 'Aldric');
             INSERT INTO character (entity_id, token_color) VALUES
                 ('{bandit}', '#888'), ('{hero}', '#38a');
             INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                 SELECT id, 1, 'dm', '{{}}', '[]' FROM entity;",
            campaign = campaign(),
            session = session(),
            scene = scene(),
            media = media(),
            bandit = bandit(),
            hero = hero(),
        ))
        .await;
    scratch
}

/// The engine as the server plants this unit and its aggregate over
/// `scratch`'s tables, with the rows a combat is made of planted by tests.
async fn served(scratch: &Scratch) -> Served {
    Bench::new(
        include_str!("../data-capability.json"),
        &file("crates/table/aggregates/BattlemapInstance/aggregate.json"),
        [aggregate_battlemap_instance::plugin(), next_turn::plugin()],
    )
    .planting(&planter(
        DROP_INSTANCE,
        "BattlemapInstance",
        "$battlemapInstanceId",
        "insert",
    ))
    .planting(&planter(
        DROP_TOKEN,
        "BattlemapInstance",
        "$tokenId",
        "insert",
    ))
    .planting(&planter(
        OPEN_COMBAT,
        "BattlemapInstance",
        "$combatId",
        "insert",
    ))
    .planting(&planter(
        SEAT,
        "BattlemapInstance",
        "$initiativeEntryId",
        "insert",
    ))
    .planting(&planter(WOUND, "BattlemapInstance", "$tokenId", "update"))
    .planting(&planter(HIDE, "BattlemapInstance", "$tokenId", "update"))
    .postgres(scratch)
    .await
}

/// An NPC token, number `number`, at `hp` of 11.
fn npc(number: u64, hp: i64) -> Value {
    json!({
        "tokenId": token(number), "tokenInstanceId": instance(), "characterId": bandit(),
        "characterType": "pnj", "number": number, "name": format!("Bandit {number}"),
        "q": 5, "r": number, "visible": true, "hpCurrent": hp, "hpMax": 11, "conditions": [],
    })
}

/// The hero's token, which tracks no HP.
fn pc(number: u64) -> Value {
    json!({
        "tokenId": token(number), "tokenInstanceId": instance(), "characterId": hero(),
        "characterType": "pj", "name": "Aldric",
        "q": 5, "r": number, "visible": true, "hpCurrent": null, "hpMax": null, "conditions": [],
    })
}

/// The token `number`'s place in the combat: rank `number`.
fn seat(number: u64) -> Value {
    json!({
        "initiativeEntryId": entry(number), "initiativeEntryCombatId": combat(),
        "initiativeEntryTokenId": token(number), "roll": 10, "initiative": 11,
        "dexScore": 12, "tiebreak": 0.5, "rank": number,
    })
}

/// The ford's instance, which the tables hold their tokens and combat under.
fn instance_row(id: &str) -> Value {
    json!({
        "battlemapInstanceId": id, "battlemapMediaId": media(), "sessionId": session(),
        "sceneId": scene(), "createdAt": "2026-10-21T10:00:00Z",
    })
}

/// A combat on the tavern's instance, round 1, the first entry's turn, over
/// `tokens`, ranked in the order given.
async fn plant(bench: &Served, tokens: Vec<Value>) {
    let count = tokens.len() as u64;
    bench
        .plant(DROP_INSTANCE, [instance_row(&instance())])
        .await;
    bench.plant(DROP_TOKEN, tokens).await;
    bench
        .plant(
            OPEN_COMBAT,
            [json!({
                "combatId": combat(), "combatInstanceId": instance(), "round": 1,
                "currentEntryId": entry(1), "startedAt": "2026-10-21T10:00:00Z",
            })],
        )
        .await;
    bench.plant(SEAT, (1..=count).map(seat)).await;
}

/// The combat's turn and round, as its table holds them.
async fn turn(scratch: &Scratch) -> (String, i64) {
    let rows = scratch
        .rows(
            "SELECT current_entry_id::text AS entry, round FROM combat WHERE id = $1::text::uuid",
            &[json!(combat())],
        )
        .await;
    (
        rows[0]["entry"].as_str().expect("a turn").to_owned(),
        rows[0]["round"].as_i64().expect("a round"),
    )
}

/// The view's rows for the combat, in rank order: entry and `skipped`.
async fn view(scratch: &Scratch) -> Vec<(String, bool)> {
    scratch
        .rows(
            "SELECT entry_id::text AS entry, skipped, shown_to_players AS shown
             FROM initiative_order_view WHERE combat_id = $1::text::uuid ORDER BY order_index",
            &[json!(combat())],
        )
        .await
        .iter()
        .map(|row| {
            (
                row["entry"].as_str().expect("an entry").to_owned(),
                row["skipped"].as_bool().expect("skipped"),
            )
        })
        .collect()
}

/// « Suivant » pressed and applied: the turn and the round it left.
async fn pressed(scratch: &Scratch, bench: &Served) -> (String, i64) {
    bench.applied(NEXT, json!({}), Some(&instance())).await;
    turn(scratch).await
}

/// What Initiative@1's NEXT gives from `(current, round)` over `view`, worked
/// out here, by hand, from the column alone: the next unskipped row after the
/// current one, the first unskipped one on a wrap, a round more.
fn expected(view: &[(String, bool)], (current, round): &(String, i64)) -> (String, i64) {
    let at = view
        .iter()
        .position(|(entry, _)| entry == current)
        .expect("current is listed");
    let later = view.iter().skip(at + 1).find(|(_, skipped)| !skipped);
    match later {
        Some((entry, _)) => (entry.clone(), *round),
        None => {
            let (entry, _) = view
                .iter()
                .find(|(_, skipped)| !skipped)
                .expect("one not skipped");
            (entry.clone(), round + 1)
        }
    }
}

// AC1, AC2, AC3, AC5: next-turn follows the view's skipped column, press after press
#[tokio::test]
async fn next_turn_follows_the_view_s_skipped_column_through_two_laps() {
    let scratch = seeded("next_turn_laps").await;
    let bench = served(&scratch).await;
    // 1 Bandit alive, 2 Bandit dead, 3 the PC, 4 Bandit hidden and alive, 5 Bandit dead.
    plant(
        &bench,
        vec![npc(1, 11), npc(2, 0), pc(3), npc(4, 7), npc(5, 0)],
    )
    .await;
    bench
        .applied(HIDE, json!({ "tokenId": token(4), "visible": false }), None)
        .await;

    let order = view(&scratch).await;
    let skipped: Vec<bool> = order.iter().map(|(_, skipped)| *skipped).collect();
    assert_eq!(skipped, [false, true, false, false, true], "{order:?}");

    let mut here = (entry(1), 1);
    assert_eq!(turn(&scratch).await, here);
    let mut seen = Vec::new();
    for _ in 0..7 {
        let next = pressed(&scratch, &bench).await;
        assert_eq!(next, expected(&order, &here), "from {here:?}");
        seen.push(next.clone());
        here = next;
    }
    // AC1: the dead second is passed over; AC3: the hidden fourth takes its
    // turn; AC2: after the last alive one, the first, a round more.
    let literal: Vec<(String, i64)> = [(3, 1), (4, 1), (1, 2), (3, 2), (4, 2), (1, 3), (3, 3)]
        .iter()
        .map(|(number, round)| (entry(*number), *round))
        .collect();
    assert_eq!(seen, literal);
}

// AC5: next-turn reads the view as it is at each press, a wound applied in between included
#[tokio::test]
async fn next_turn_reads_a_wound_applied_between_two_presses() {
    let scratch = seeded("next_turn_wound").await;
    let bench = served(&scratch).await;
    plant(&bench, vec![npc(1, 11), npc(2, 11), npc(3, 11)]).await;

    assert_eq!(pressed(&scratch, &bench).await, (entry(2), 1));
    bench
        .applied(WOUND, json!({ "tokenId": token(3), "hpCurrent": 0 }), None)
        .await;
    // The third is dead now: the turn wraps to the first and counts the round.
    assert_eq!(pressed(&scratch, &bench).await, (entry(1), 2));
    // Healed, the third is back.
    bench
        .applied(WOUND, json!({ "tokenId": token(3), "hpCurrent": 4 }), None)
        .await;
    assert_eq!(pressed(&scratch, &bench).await, (entry(2), 2));
    assert_eq!(pressed(&scratch, &bench).await, (entry(3), 2));
}

// AC2: next-turn gives the round to the only participant left, again and again
#[tokio::test]
async fn next_turn_with_one_participant_not_skipped_wraps_to_it_and_counts_a_round_each_press() {
    let scratch = seeded("next_turn_alone").await;
    let bench = served(&scratch).await;
    plant(&bench, vec![pc(1), npc(2, 0)]).await;

    assert_eq!(pressed(&scratch, &bench).await, (entry(1), 2));
    assert_eq!(pressed(&scratch, &bench).await, (entry(1), 3));
}

// AC4: dead-derived refuses a combat in which every participant is skipped
#[tokio::test]
async fn dead_derived_a_combat_whose_every_participant_is_skipped_is_refused() {
    let scratch = seeded("next_turn_all_skipped").await;
    let bench = served(&scratch).await;
    plant(&bench, vec![npc(1, 0), npc(2, 0), npc(3, 0)]).await;
    assert!(view(&scratch).await.iter().all(|(_, skipped)| *skipped));

    let message = refused(
        bench.submit(NEXT, json!({}), Some(&instance())).await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("dead-derived"), "{message}");
    assert!(bench.settled().await.is_empty(), "nothing was queued");
    assert_eq!(turn(&scratch).await, (entry(1), 1));
}

// AC4: next-turn refuses a combat the view lists no participant of
#[tokio::test]
async fn next_turn_with_no_participant_listed_is_refused() {
    let scratch = seeded("next_turn_empty").await;
    let bench = served(&scratch).await;
    bench
        .plant(DROP_INSTANCE, [instance_row(&instance())])
        .await;
    bench
        .plant(
            OPEN_COMBAT,
            [json!({
                "combatId": combat(), "combatInstanceId": instance(),
                "round": 1, "currentEntryId": null, "startedAt": "2026-10-21T10:00:00Z",
            })],
        )
        .await;
    assert!(view(&scratch).await.is_empty());

    let message = refused(
        bench.submit(NEXT, json!({}), Some(&instance())).await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("dead-derived"), "{message}");
}

// AC6: deadness is no longer computed from hpCurrent in src/lib.rs
#[test]
fn the_unit_s_source_never_reads_hp_current() {
    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("the unit's own source is beside its tests");
    assert!(!source.contains("hpCurrent"), "src/lib.rs reads hpCurrent");
}
