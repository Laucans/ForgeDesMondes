//! table.getBattlemapInstance over PostgreSQL (`DATABASE_URL`): the real SQL of
//! the Capability, run by the test kit's scratch database on tables the real
//! DataCapabilities wrote — `table.startCombat` seating the PCs at the
//! initiative the DM entered, `table.endCombat` closing the combat,
//! `table.resetBattlemap` tombstoning the map —, and answered by `answer`.
//! `pjInitiative` is each PC's last entered initiative: it survives the end of
//! the combat and a reopen (a second read), is replaced by a later combat, is
//! the same in both views, never names an NPC, and is emptied by a reset.
//! Each test works in a database of its own, dropped when it ends.

use std::collections::BTreeMap;

use get_battlemap_instance::{SQL, VARIABLES, answer};
use platform_test_kit::{Bench, Scratch, Served, planter, read, uuid};
use serde_json::{Value, json};

fn campaign() -> String {
    uuid(0xc1, 1)
}
fn battlemap() -> String {
    uuid(0xd1, 1)
}
fn session() -> String {
    uuid(0x5e, 1)
}
fn scene() -> String {
    uuid(0x5c, 1)
}
/// The ford's instance of the scene (bmi-gue-s4).
fn instance() -> String {
    uuid(0xb1, 1)
}
/// The four PCs of the table, and the two NPCs facing them.
fn elowen() -> String {
    uuid(0xe1, 1)
}
fn brannoc() -> String {
    uuid(0xe1, 2)
}
fn sirielle() -> String {
    uuid(0xe1, 3)
}
fn kael() -> String {
    uuid(0xe1, 4)
}
fn bandit() -> String {
    uuid(0xe1, 0xb1)
}
fn chef() -> String {
    uuid(0xe1, 0xb2)
}
/// A token's id: the how-many-th of the instance.
fn token(n: u64) -> String {
    uuid(0x70, n)
}

const START_COMBAT: &str = "table.startCombat@1";
const END_COMBAT: &str = "table.endCombat@1";
const RESET: &str = "table.resetBattlemap@1";

/// A token of `character` on the instance, as `table.placeTokens` drops it; an
/// NPC's tracks 11 HP.
fn placed(id: &str, character: &str, name: &str, npc: bool) -> Value {
    json!({
        "tokenId": id,
        "tokenInstanceId": instance(),
        "characterId": character,
        "number": if npc { json!(1) } else { Value::Null },
        "name": name,
        "q": 5,
        "r": 5,
        "visible": true,
        "hpCurrent": if npc { json!(11) } else { Value::Null },
        "hpMax": if npc { json!(11) } else { Value::Null },
        "conditions": [],
    })
}

/// A scratch database holding the campaign, the ford with its grid, the live
/// session and its scene, the four PCs and the two NPCs with their DEX.
async fn seeded(label: &str) -> Scratch {
    let db = Scratch::new(&format!("get_battlemap_instance_{label}")).await;
    db.seed(&format!(
        "INSERT INTO campaign (id, name, kind) VALUES ('{campaign}', 'Les Cendres de Valombre', 'campagne');
         INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES
             ('{battlemap}', '{campaign}', 'battlemap', 'Le Gué des Corbeaux', 'media/gue.png', 960, 640);
         INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) VALUES
             ('{battlemap}', 30, 12, 8, 'pointe', 'text', 0.4);
         INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
             ('{session}', '{campaign}', 4, 'L’embuscade du Gué', 'live', true);
         INSERT INTO scene (id, session_id, position, title) VALUES ('{scene}', '{session}', 1, 'Le Gué');
         INSERT INTO entity (id, campaign_id, type, name) VALUES
             ('{elowen}', '{campaign}', 'pj', 'Elowen'), ('{brannoc}', '{campaign}', 'pj', 'Brannoc'),
             ('{sirielle}', '{campaign}', 'pj', 'Sirielle'), ('{kael}', '{campaign}', 'pj', 'Kael'),
             ('{bandit}', '{campaign}', 'pnj', 'Bandit'), ('{chef}', '{campaign}', 'pnj', 'Chef bandit');
         INSERT INTO character (entity_id, hp_max, dexterity, token_color) VALUES
             ('{elowen}', 44, 14, 'success'), ('{brannoc}', 52, 10, 'success'),
             ('{sirielle}', 31, 16, 'success'), ('{kael}', 38, 12, 'success'),
             ('{bandit}', 11, 12, 'danger'), ('{chef}', 65, 16, 'danger');
         INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
             SELECT id, 1, 'dm', '{{}}', '[]' FROM entity;",
        campaign = campaign(),
        battlemap = battlemap(),
        session = session(),
        scene = scene(),
        elowen = elowen(),
        brannoc = brannoc(),
        sirielle = sirielle(),
        kael = kael(),
        bandit = bandit(),
        chef = chef(),
    ))
    .await;
    db
}

/// The engine as the server plants the three DataCapabilities beside their
/// aggregate, and on it the ford's instance with its six tokens.
async fn table(scratch: &Scratch) -> Served {
    let bench = Bench::new(
        &platform_test_kit::file(
            "crates/table/data-capabilities/start-combat/data-capability.json",
        ),
        &platform_test_kit::file("crates/table/aggregates/BattlemapInstance/aggregate.json"),
        [
            aggregate_battlemap_instance::plugin(),
            start_combat::plugin(),
            end_combat::plugin(),
            reset_battlemap::plugin(),
        ],
    )
    .planting(&platform_test_kit::file(
        "crates/table/data-capabilities/end-combat/data-capability.json",
    ))
    .planting(&platform_test_kit::file(
        "crates/table/data-capabilities/reset-battlemap/data-capability.json",
    ))
    .planting(&planter(
        "test.openInstance",
        "BattlemapInstance",
        "$battlemapInstanceId",
        "insert",
    ))
    .planting(&planter(
        "test.dropToken",
        "BattlemapInstance",
        "$tokenId",
        "insert",
    ));
    let served = bench.postgres(scratch).await;
    served
        .plant(
            "test.openInstance",
            [json!({
                "battlemapInstanceId": instance(),
                "battlemapMediaId": battlemap(),
                "sessionId": session(),
                "sceneId": scene(),
                "createdAt": "2026-10-21T10:00:00Z",
            })],
        )
        .await;
    served
        .plant(
            "test.dropToken",
            [
                placed(&token(1), &elowen(), "Elowen", false),
                placed(&token(2), &brannoc(), "Brannoc", false),
                placed(&token(3), &sirielle(), "Sirielle", false),
                placed(&token(4), &kael(), "Kael", false),
                placed(&token(5), &bandit(), "Bandit 1", true),
                placed(&token(6), &chef(), "Chef bandit 1", true),
            ],
        )
        .await;
    served
}

/// The table's four PCs at the initiative the DM entered, and the NPCs'
/// rolls, as the Battlemap sends a start.
fn lineup(elowen_at: i64) -> Value {
    json!({
        "initiative": {
            elowen(): elowen_at, brannoc(): 9, sirielle(): 14, kael(): 12,
        },
        "rolls": { token(5): 13, token(6): 11 },
    })
}

/// Starts a combat on the instance, every seat applied.
async fn start(served: &Served, payload: Value) {
    submit_all(served, START_COMBAT, payload).await;
}

/// Submits `data_capability` on the instance and runs the queue until it is
/// drained, every command applied — a reset queues follow-ups.
async fn submit_all(served: &Served, data_capability: &str, payload: Value) {
    served
        .submit(data_capability, payload, Some(&instance()))
        .await
        .unwrap_or_else(|e| panic!("{data_capability} was refused: {e}"));
    loop {
        let settled = served.settled().await;
        if settled.is_empty() {
            return;
        }
        assert!(
            settled
                .iter()
                .all(|result| result.status == harness_dataguard::command::State::Applied),
            "{settled:?}"
        );
    }
}

/// What the registry serves for the instance in `view`: the rows of [`SQL`]
/// bound to it, then `answer`.
async fn read_in(scratch: &Scratch, view: &str) -> Value {
    let rows = scratch.rows(SQL, &[json!(instance())]).await;
    let variables = BTreeMap::from([
        ("instance".to_string(), json!(instance())),
        ("view".to_string(), json!(view)),
    ]);
    answer(&Value::Array(rows), &variables)
}

/// The initiatives the DM entered at the table's start, by character.
fn entered(elowen_at: i64) -> Value {
    json!({
        elowen(): elowen_at, brannoc(): 9, sirielle(): 14, kael(): 12,
    })
}

// AC1: a fresh instance answers `pjInitiative: {}` in either view.
#[tokio::test(flavor = "current_thread")]
async fn a_fresh_instance_has_no_initiative_to_prefill() {
    let scratch = seeded("fresh").await;
    let _served = table(&scratch).await;
    for view in ["dm", "player"] {
        let got = read_in(&scratch, view).await;
        assert_eq!(got["pjInitiative"], json!({}), "{view}");
        assert_eq!(got["state"]["combat"], Value::Null, "{view}");
    }
}

// AC2: after a start, `pjInitiative` is the entered map in both views, no NPC in it.
#[tokio::test(flavor = "current_thread")]
async fn a_started_combat_gives_each_pcs_initiative_in_both_views_and_no_npcs() {
    let scratch = seeded("started").await;
    let served = table(&scratch).await;
    start(&served, lineup(18)).await;
    for view in ["dm", "player"] {
        let got = read_in(&scratch, view).await;
        assert_eq!(got["pjInitiative"], entered(18), "{view}");
        for npc in [bandit(), chef()] {
            assert!(got["pjInitiative"].get(&npc).is_none(), "{view}: {npc}");
        }
    }
}

// AC3: after the combat ends, `pjInitiative` is unchanged and `combat` is null.
#[tokio::test(flavor = "current_thread")]
async fn the_initiatives_outlive_the_combat_that_took_them() {
    let scratch = seeded("ended").await;
    let served = table(&scratch).await;
    start(&served, lineup(18)).await;
    assert_ne!(
        read_in(&scratch, "dm").await["state"]["combat"],
        Value::Null
    );
    submit_all(&served, END_COMBAT, json!({})).await;
    for view in ["dm", "player"] {
        let got = read_in(&scratch, view).await;
        assert_eq!(got["state"]["combat"], Value::Null, "{view}");
        assert_eq!(got["pjInitiative"], entered(18), "{view}");
    }
}

// AC4: a later combat's figure for a character replaces the old one.
#[tokio::test(flavor = "current_thread")]
async fn a_later_combat_replaces_the_initiative_per_character() {
    let scratch = seeded("replaced").await;
    let served = table(&scratch).await;
    start(&served, lineup(18)).await;
    submit_all(&served, END_COMBAT, json!({})).await;
    // The second combat starts later, and gives Elowen another figure.
    served.clock.set(platform_test_kit::NOW + 3600);
    start(&served, lineup(7)).await;
    for view in ["dm", "player"] {
        let got = read_in(&scratch, view).await;
        assert_eq!(got["pjInitiative"], entered(7), "{view}");
    }
    // Ended too, the latest combat stays the one read.
    submit_all(&served, END_COMBAT, json!({})).await;
    assert_eq!(read_in(&scratch, "dm").await["pjInitiative"], entered(7));
}

// AC5: reading again, as after a reopen, answers the same initiatives,
// positions, hit points and combat order.
#[tokio::test(flavor = "current_thread")]
async fn a_second_read_answers_the_same_map_positions_hit_points_and_order() {
    let scratch = seeded("reopened").await;
    let served = table(&scratch).await;
    start(&served, lineup(18)).await;
    for view in ["dm", "player"] {
        let first = read_in(&scratch, view).await;
        let second = read_in(&scratch, view).await;
        assert_eq!(first, second, "{view}");
        assert_eq!(first["pjInitiative"], entered(18), "{view}");
        assert_eq!(
            first["state"]["combat"]["order"].as_array().map(Vec::len),
            Some(6)
        );
    }
    let dm = read_in(&scratch, "dm").await;
    let bandit = dm["state"]["tokens"]
        .as_array()
        .and_then(|tokens| tokens.iter().find(|t| t["id"] == token(5)))
        .expect("the Bandit");
    assert_eq!((&bandit["q"], &bandit["r"]), (&json!(5), &json!(5)));
    assert_eq!((&bandit["hp"], &bandit["hpMax"]), (&json!(11), &json!(11)));
}

// AC6: after a reset, `pjInitiative` is `{}`.
#[tokio::test(flavor = "current_thread")]
async fn a_reset_empties_the_initiatives() {
    let scratch = seeded("reset").await;
    let served = table(&scratch).await;
    start(&served, lineup(18)).await;
    submit_all(&served, END_COMBAT, json!({})).await;
    start(&served, lineup(18)).await;
    submit_all(&served, RESET, json!({})).await;
    for view in ["dm", "player"] {
        let got = read_in(&scratch, view).await;
        assert_eq!(got["pjInitiative"], json!({}), "{view}");
        assert_eq!(got["state"]["combat"], Value::Null, "{view}");
    }
}

// AC7: every v1 field of the answer is still there, under its v1 name and type,
// beside `pjInitiative`.
#[tokio::test(flavor = "current_thread")]
async fn the_answer_keeps_every_v1_field_beside_the_new_one() {
    let scratch = seeded("v1").await;
    let served = table(&scratch).await;
    start(&served, lineup(18)).await;
    let got = read_in(&scratch, "dm").await;
    let keys = |value: &Value| -> Vec<String> {
        value
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect()
    };
    assert_eq!(
        keys(&got),
        [
            "battlemap",
            "id",
            "pjInitiative",
            "roster",
            "scene",
            "session",
            "state"
        ]
    );
    assert_eq!(keys(&got["state"]), ["combat", "tokens"]);
    assert_eq!(keys(&got["state"]["combat"]), ["current", "order", "round"]);
    let mut token_fields = vec![
        "char", "color", "crop", "dead", "hp", "hpMax", "id", "initials", "kind", "name", "num",
        "portrait", "q", "r", "states", "status", "visible",
    ];
    token_fields.sort_unstable();
    assert_eq!(keys(&got["state"]["tokens"][0]), token_fields);
    assert_eq!(got["id"], instance());
    assert_eq!(got["battlemap"], battlemap());
    assert_eq!(got["session"], session());
    assert_eq!(got["scene"], scene());
    assert_eq!(got["roster"].as_array().map(Vec::len), Some(6));
    assert_eq!(got["state"]["combat"]["round"], 1);
}

#[test]
fn the_sql_keeps_the_guards_of_a_capability() {
    read::one_read_statement(SQL);
    read::binds_in_order(SQL, VARIABLES);
    read::query_file_matches(
        SQL,
        &read::query("table", "get-battlemap-instance"),
        VARIABLES,
    );
}
