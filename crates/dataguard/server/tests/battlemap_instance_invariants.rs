//! The BattlemapInstance aggregate's invariants, judged by the DataGuard as
//! `plant()` binds it, over the in-memory store: a command that breaks one
//! is rejected under its id, and the row stays as it was. The
//! DataCapabilities here are test-only — no DataCapability of the product
//! writes an instance, a token, a combat or an initiative yet. Each targets
//! the row by the id of its own table.

use dataguard_server::{Engine, MEMORY, Rows, planted};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};

/// The test-only DataCapabilities: their name, their effect, the payload
/// field naming the row, the fields they touch, their mode.
const MANIFESTS: [(&str, &str, &str, &[&str], &str); 13] = [
    (
        "test.openInstance",
        "insert",
        "battlemapInstanceId",
        &[],
        "overwrite",
    ),
    (
        "test.updateInstance",
        "update",
        "battlemapInstanceId",
        &[],
        "overwrite",
    ),
    ("test.dropToken", "insert", "tokenId", &[], "overwrite"),
    ("test.updateToken", "update", "tokenId", &[], "overwrite"),
    ("test.removeToken", "delete", "tokenId", &[], "overwrite"),
    ("test.startCombat", "insert", "combatId", &[], "overwrite"),
    ("test.updateCombat", "update", "combatId", &[], "overwrite"),
    (
        "test.addEntry",
        "insert",
        "initiativeEntryId",
        &[],
        "overwrite",
    ),
    (
        "test.updateEntry",
        "update",
        "initiativeEntryId",
        &[],
        "overwrite",
    ),
    // A change of HP that says it settles the death too.
    (
        "test.woundToken",
        "update",
        "tokenId",
        &["BattlemapInstance.hpCurrent", "BattlemapInstance.dead"],
        "overwrite",
    ),
    // A reveal that says it reorders too.
    (
        "test.revealAndRerank",
        "update",
        "tokenId",
        &["BattlemapInstance.visible", "BattlemapInstance.rank"],
        "overwrite",
    ),
    // A reset that says it redraws the battlemap's grid.
    (
        "test.resetGrid",
        "update",
        "battlemapInstanceId",
        &["BattlemapInstance.resetAt", "Media.hexSizePx"],
        "overwrite",
    ),
    // « Suivant »: a step of the round, relative.
    ("test.nextTurn", "update", "combatId", &[], "relative"),
];

/// A campaign's fields changed: another aggregate, with fields of the same
/// names.
const UPDATE_CAMPAIGN: &str = r#"{"dataCapability":"test.updateCampaign","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's PC.
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
/// Valombre's NPC.
const MORLOX: &str = "e1000000-0000-4000-8000-000000000002";
/// The tavern's battlemap.
const CARTE: &str = "d1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
/// The tavern scene.
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
/// The ambush scene.
const EMBUSCADE: &str = "5c000000-0000-4000-8000-000000000002";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
/// The ambush's instance.
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const JETON_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const MORLOX_1: &str = "70000000-0000-4000-8000-000000000002";
const MORLOX_2: &str = "70000000-0000-4000-8000-000000000003";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const ENTREE_MORLOX_1: &str = "1e000000-0000-4000-8000-000000000001";
const ENTREE_ELOWEN: &str = "1e000000-0000-4000-8000-000000000002";

/// The engine as the server plants it, with the test-only DataCapabilities,
/// and the rows it holds.
async fn engine() -> (Engine, Rows) {
    let planted = planted(MEMORY).await.expect("the in-memory engine");
    let mut engine = planted.engine;
    for (name, effect, id, touches, mode) in MANIFESTS {
        let manifest = json!({
            "dataCapability": name,
            "owner": "dataguard",
            "version": 1,
            "effect": effect,
            "target": { "aggregate": "BattlemapInstance", "id": format!("${id}") },
            "touches": touches,
            "mode": mode,
            "callableBy": ["*"],
        });
        engine = engine.with_data_capability(
            DataCapability::parse(&manifest.to_string()).expect("contract F"),
        );
    }
    let engine =
        engine.with_data_capability(DataCapability::parse(UPDATE_CAMPAIGN).expect("contract F"));
    (engine, planted.rows)
}

/// What the single applier made of one command.
async fn written(engine: &Engine, data_capability: &str, payload: Value) -> CommandResult {
    queued(engine, data_capability, payload).await;
    let mut applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    applied.remove(0)
}

/// One command queued, the applier not run yet.
async fn queued(engine: &Engine, data_capability: &str, payload: Value) {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    engine
        .submit(Submission {
            data_capability: data_capability.to_string(),
            payload: payload.into_iter().collect(),
            target_id: None,
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "table.battlemap".to_string(),
                permissions: Vec::new(),
                // The critical fields' review is not what these tests judge.
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
}

fn rejected_by(result: &CommandResult, ids: &[&str]) {
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ids, "{result:?}");
}

/// Rejected, `id` among the invariants it breaks.
fn rejected_among(result: &CommandResult, id: &str) {
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert!(result.violations.iter().any(|v| v == id), "{result:?}");
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

fn with(mut base: Value, field: &str, value: Value) -> Value {
    base[field] = value;
    base
}

fn without(mut base: Value, field: &str) -> Value {
    base.as_object_mut().expect("an object").remove(field);
    base
}

/// The tavern scene's instance of its battlemap.
fn instance() -> Value {
    json!({
        "battlemapInstanceId": INSTANCE,
        "battlemapMediaId": CARTE,
        "sessionId": SEANCE,
        "sceneId": ARRIVEE,
        "createdAt": "2026-10-18T20:00:00Z",
    })
}

/// Elowen's token: a PC's, HP not tracked, dropped once.
fn elowen() -> Value {
    json!({
        "tokenId": JETON_ELOWEN,
        "tokenInstanceId": INSTANCE,
        "characterId": ELOWEN,
        "name": "Elowen",
        "q": 0,
        "r": 0,
        "visible": true,
        "conditions": [],
    })
}

/// Morlox `n` of a drop « × 2 »: 7 HP of 7, copied from the sheet.
fn morlox(id: &str, n: u8) -> Value {
    json!({
        "tokenId": id,
        "tokenInstanceId": INSTANCE,
        "characterId": MORLOX,
        "number": n,
        "name": format!("Morlox {n}"),
        "q": 3,
        "r": -1,
        "visible": true,
        "hpCurrent": 7,
        "hpMax": 7,
        "conditions": [],
    })
}

/// The tavern's combat, at its first round.
fn combat() -> Value {
    json!({
        "combatId": COMBAT,
        "combatInstanceId": INSTANCE,
        "round": 1,
        "currentEntryId": ENTREE_ELOWEN,
        "startedAt": "2026-10-18T20:05:00Z",
    })
}

/// Morlox 1's entry: 14 rolled, DEX 12 (+1), initiative 15.
fn morlox_entry() -> Value {
    json!({
        "initiativeEntryId": ENTREE_MORLOX_1,
        "initiativeEntryCombatId": COMBAT,
        "initiativeEntryTokenId": MORLOX_1,
        "roll": 14,
        "initiative": 15,
        "dexScore": 12,
        "tiebreak": 0.5,
        "rank": 2,
    })
}

/// Elowen's entry: no roll, the 17 the DM entered.
fn elowen_entry() -> Value {
    json!({
        "initiativeEntryId": ENTREE_ELOWEN,
        "initiativeEntryCombatId": COMBAT,
        "initiativeEntryTokenId": JETON_ELOWEN,
        "roll": null,
        "initiative": 17,
        "dexScore": 16,
        "tiebreak": 0.25,
        "rank": 1,
    })
}

/// An engine holding the tavern's instance, Elowen and Morlox 1 and 2 on
/// it, and the combat with their two entries.
async fn tavern() -> (Engine, Rows) {
    let (engine, rows) = engine().await;
    for (data_capability, row) in [
        ("test.openInstance", instance()),
        ("test.dropToken", elowen()),
        ("test.dropToken", morlox(MORLOX_1, 1)),
        ("test.dropToken", morlox(MORLOX_2, 2)),
        ("test.startCombat", combat()),
        ("test.addEntry", elowen_entry()),
        ("test.addEntry", morlox_entry()),
    ] {
        applied(&written(&engine, data_capability, row).await);
    }
    (engine, rows)
}

/// `field` of the row `id` names, as the engine holds it.
async fn held(rows: &Rows, id: &str, field: &str) -> Value {
    rows.projected(&format!("BattlemapInstance/{id}"))
        .await
        .expect("the row read")
        .get(field)
        .cloned()
        .unwrap_or(Value::Null)
}

#[tokio::test]
async fn instance_isolated_never_writes_the_battlemap_nor_its_grid() {
    let (engine, _) = tavern().await;
    // The grid's hex size and the battlemap's title are the Media
    // aggregate's: an instance changes neither.
    for (field, value) in [("hexSizePx", json!(80)), ("title", json!("Taverne en feu"))] {
        rejected_by(
            &written(
                &engine,
                "test.updateInstance",
                json!({ "battlemapInstanceId": INSTANCE, field: value }),
            )
            .await,
            &["instance-isolated"],
        );
    }
    // Nor does it say it touches them.
    rejected_by(
        &written(
            &engine,
            "test.resetGrid",
            json!({ "battlemapInstanceId": INSTANCE, "resetAt": "2026-10-18T21:00:00Z" }),
        )
        .await,
        &["instance-isolated"],
    );
    // Its own fields, it writes.
    applied(
        &written(
            &engine,
            "test.updateInstance",
            json!({ "battlemapInstanceId": INSTANCE, "resetAt": "2026-10-18T21:00:00Z" }),
        )
        .await,
    );
}

#[tokio::test]
async fn instance_isolated_keeps_tokens_combat_and_initiative_on_their_own_instance() {
    let (engine, rows) = tavern().await;
    for (data_capability, row) in [
        (
            "test.updateToken",
            json!({ "tokenId": MORLOX_1, "tokenInstanceId": AUTRE_INSTANCE }),
        ),
        (
            "test.updateCombat",
            json!({ "combatId": COMBAT, "combatInstanceId": AUTRE_INSTANCE }),
        ),
        (
            "test.updateEntry",
            json!({ "initiativeEntryId": ENTREE_MORLOX_1, "initiativeEntryTokenId": MORLOX_2 }),
        ),
        (
            "test.updateEntry",
            json!({ "initiativeEntryId": ENTREE_MORLOX_1, "initiativeEntryCombatId": INSTANCE }),
        ),
    ] {
        rejected_by(
            &written(&engine, data_capability, row).await,
            &["instance-isolated"],
        );
    }
    // An entry is a place in a combat, for a token: it names both.
    for field in ["initiativeEntryCombatId", "initiativeEntryTokenId"] {
        rejected_by(
            &written(
                &engine,
                "test.addEntry",
                without(
                    with(
                        morlox_entry(),
                        "initiativeEntryId",
                        json!("1e000000-0000-4000-8000-000000000009"),
                    ),
                    field,
                ),
            )
            .await,
            &["instance-isolated"],
        );
    }
    assert_eq!(held(&rows, MORLOX_1, "tokenInstanceId").await, INSTANCE);
    assert_eq!(held(&rows, COMBAT, "combatInstanceId").await, INSTANCE);
    assert_eq!(
        held(&rows, ENTREE_MORLOX_1, "initiativeEntryTokenId").await,
        MORLOX_1
    );
}

#[tokio::test]
async fn one_instance_per_scene_names_its_battlemap_session_and_scene_and_keeps_them() {
    let (engine, _) = engine().await;
    for field in ["battlemapMediaId", "sessionId", "sceneId"] {
        rejected_by(
            &written(&engine, "test.openInstance", without(instance(), field)).await,
            &["one-instance-per-scene"],
        );
    }
    applied(&written(&engine, "test.openInstance", instance()).await);
    // The instance is the tavern scene's: it does not move to the ambush.
    rejected_by(
        &written(
            &engine,
            "test.updateInstance",
            json!({ "battlemapInstanceId": INSTANCE, "sceneId": EMBUSCADE }),
        )
        .await,
        &["one-instance-per-scene"],
    );
    // An id in another case is the same scene.
    applied(
        &written(
            &engine,
            "test.updateInstance",
            json!({ "battlemapInstanceId": INSTANCE, "sceneId": ARRIVEE.to_uppercase() }),
        )
        .await,
    );
}

#[tokio::test]
async fn token_roster_drops_a_token_for_one_character_on_one_instance() {
    let (engine, rows) = tavern().await;
    for field in ["characterId", "tokenInstanceId"] {
        rejected_among(
            &written(
                &engine,
                "test.dropToken",
                without(morlox("70000000-0000-4000-8000-000000000009", 3), field),
            )
            .await,
            "token-roster",
        );
    }
    // Elowen's token stays Elowen's: it does not become Morlox's.
    rejected_by(
        &written(
            &engine,
            "test.updateToken",
            json!({ "tokenId": JETON_ELOWEN, "characterId": MORLOX }),
        )
        .await,
        &["token-roster"],
    );
    assert_eq!(held(&rows, JETON_ELOWEN, "characterId").await, ELOWEN);
}

#[tokio::test]
async fn multi_drop_numbering_counts_from_1_and_a_rename_keeps_the_number() {
    let (engine, rows) = tavern().await;
    // A third Morlox, numbered after the two taken.
    applied(
        &written(
            &engine,
            "test.dropToken",
            morlox("70000000-0000-4000-8000-000000000004", 3),
        )
        .await,
    );
    for number in [json!(0), json!(-1), json!(1.5), json!("4"), json!(40_000)] {
        rejected_by(
            &written(
                &engine,
                "test.dropToken",
                with(
                    morlox("70000000-0000-4000-8000-000000000005", 4),
                    "number",
                    number,
                ),
            )
            .await,
            &["multi-drop-numbering"],
        );
    }
    // A token has a name.
    for name in [Value::Null, json!(" "), json!(2)] {
        rejected_by(
            &written(
                &engine,
                "test.updateToken",
                json!({ "tokenId": MORLOX_2, "name": name }),
            )
            .await,
            &["multi-drop-numbering"],
        );
    }
    // Renaming Morlox 2 touches its name alone.
    applied(
        &written(
            &engine,
            "test.updateToken",
            json!({ "tokenId": MORLOX_2, "name": "Chef bandit" }),
        )
        .await,
    );
    assert_eq!(held(&rows, MORLOX_2, "number").await, 2);
    assert_eq!(held(&rows, MORLOX_1, "name").await, "Morlox 1");
    // Its number is its drop's; its initial, the table's.
    for (field, value) in [("number", json!(5)), ("labelInitial", json!("C"))] {
        rejected_by(
            &written(
                &engine,
                "test.updateToken",
                json!({ "tokenId": MORLOX_2, field: value }),
            )
            .await,
            &["multi-drop-numbering"],
        );
    }
}

#[tokio::test]
async fn pj_hp_not_tracked_leaves_a_pc_without_hp_and_an_npc_within_its_maximum() {
    let (engine, _) = tavern().await;
    // A PC's token tracks no HP, not one of the two.
    for (field, value) in [("hpCurrent", json!(12)), ("hpMax", json!(12))] {
        rejected_by(
            &written(
                &engine,
                "test.updateToken",
                json!({ "tokenId": JETON_ELOWEN, field: value }),
            )
            .await,
            &["pj-hp-not-tracked"],
        );
    }
    // An NPC's, from 0 to its maximum, whole.
    for hp in [json!(8), json!(-1), json!(3.5), json!("3")] {
        rejected_by(
            &written(
                &engine,
                "test.updateToken",
                json!({ "tokenId": MORLOX_1, "hpCurrent": hp }),
            )
            .await,
            &["pj-hp-not-tracked"],
        );
    }
    for hp in [0, 7, 3] {
        applied(
            &written(
                &engine,
                "test.updateToken",
                json!({ "tokenId": MORLOX_1, "hpCurrent": hp }),
            )
            .await,
        );
    }
}

#[tokio::test]
async fn hp_change_one_token_touches_the_targeted_token_alone() {
    let (engine, rows) = tavern().await;
    applied(
        &written(
            &engine,
            "test.updateToken",
            json!({ "tokenId": MORLOX_1, "hpCurrent": 2 }),
        )
        .await,
    );
    // The other Morlox keeps its HP.
    assert_eq!(held(&rows, MORLOX_1, "hpCurrent").await, 2);
    assert_eq!(held(&rows, MORLOX_2, "hpCurrent").await, 7);
    // HP are a token's: not the instance's, nor the combat's.
    rejected_among(
        &written(
            &engine,
            "test.updateInstance",
            json!({ "battlemapInstanceId": INSTANCE, "hpCurrent": 2, "hpMax": 7 }),
        )
        .await,
        "hp-change-one-token",
    );
}

#[tokio::test]
async fn dead_derived_leaves_death_to_the_hp_and_writes_it_never() {
    let (engine, rows) = tavern().await;
    // Down to 0: dead, which the table derives.
    applied(
        &written(
            &engine,
            "test.updateToken",
            json!({ "tokenId": MORLOX_1, "hpCurrent": 0 }),
        )
        .await,
    );
    assert_eq!(held(&rows, MORLOX_1, "dead").await, Value::Null);
    for (field, value) in [("dead", json!(true)), ("skipped", json!(false))] {
        rejected_by(
            &written(
                &engine,
                "test.updateToken",
                json!({ "tokenId": MORLOX_2, field: value }),
            )
            .await,
            &["dead-derived"],
        );
    }
    // Nor by a touch.
    rejected_by(
        &written(
            &engine,
            "test.woundToken",
            json!({ "tokenId": MORLOX_2, "hpCurrent": 0 }),
        )
        .await,
        &["dead-derived"],
    );
}

#[tokio::test]
async fn players_see_band_only_leaves_the_band_and_what_is_shown_to_the_tables() {
    let (engine, _) = tavern().await;
    for (field, value) in [
        ("healthBand", json!("indemne")),
        ("healthBand", json!(7)),
        ("shownToPlayers", json!(true)),
    ] {
        rejected_by(
            &written(
                &engine,
                "test.updateToken",
                json!({ "tokenId": MORLOX_2, field: value }),
            )
            .await,
            &["players-see-band-only"],
        );
    }
    // Every token says whether the players see it.
    for visible in [Value::Null, json!("oui")] {
        rejected_by(
            &written(
                &engine,
                "test.dropToken",
                with(
                    morlox("70000000-0000-4000-8000-000000000009", 3),
                    "visible",
                    visible,
                ),
            )
            .await,
            &["players-see-band-only"],
        );
    }
    // Hiding the token is the DM's; what the players see follows.
    applied(
        &written(
            &engine,
            "test.updateToken",
            json!({ "tokenId": MORLOX_2, "visible": false }),
        )
        .await,
    );
}

#[tokio::test]
async fn npc_initiative_bounds_rolls_a_d20_plus_the_dex_modifier() {
    let (engine, _) = tavern().await;
    let entry = |roll: Value, dex: Value, initiative: Value| {
        json!({
            "initiativeEntryId": "1e000000-0000-4000-8000-000000000003",
            "initiativeEntryCombatId": COMBAT,
            "initiativeEntryTokenId": MORLOX_2,
            "roll": roll,
            "initiative": initiative,
            "dexScore": dex,
            "tiebreak": 0.75,
            "rank": 3,
        })
    };
    for row in [
        // Off the die.
        entry(json!(21), json!(12), json!(22)),
        entry(json!(0), json!(12), json!(1)),
        // Not the roll plus +1.
        entry(json!(14), json!(12), json!(16)),
        // DEX 9 is -1, floored: 10 rolled is 9, not 10.
        entry(json!(10), json!(9), json!(10)),
        // A roll with no DEX to add.
        entry(json!(10), Value::Null, json!(10)),
        // No initiative, or not a whole one.
        entry(Value::Null, json!(12), Value::Null),
        entry(Value::Null, json!(12), json!(15.5)),
    ] {
        rejected_by(
            &written(&engine, "test.addEntry", row).await,
            &["npc-initiative-bounds"],
        );
    }
    applied(
        &written(
            &engine,
            "test.addEntry",
            entry(json!(10), json!(9), json!(9)),
        )
        .await,
    );
    // A PC rolls nothing: the DM's figure stands.
    applied(
        &written(
            &engine,
            "test.updateEntry",
            json!({ "initiativeEntryId": ENTREE_ELOWEN, "initiative": 4 }),
        )
        .await,
    );
}

#[tokio::test]
async fn initiative_tiebreak_ranks_from_1_and_keeps_the_draw_of_the_launch() {
    let (engine, rows) = tavern().await;
    for (field, value) in [
        ("tiebreak", Value::Null),
        ("tiebreak", json!("pile")),
        ("rank", json!(0)),
        ("rank", Value::Null),
    ] {
        rejected_by(
            &written(
                &engine,
                "test.addEntry",
                with(
                    with(
                        morlox_entry(),
                        "initiativeEntryId",
                        json!("1e000000-0000-4000-8000-000000000004"),
                    ),
                    field,
                    value,
                ),
            )
            .await,
            &["initiative-tiebreak"],
        );
    }
    // Drawn again, the tie could break the other way.
    rejected_by(
        &written(
            &engine,
            "test.updateEntry",
            json!({ "initiativeEntryId": ENTREE_MORLOX_1, "tiebreak": 0.1 }),
        )
        .await,
        &["initiative-tiebreak"],
    );
    assert_eq!(held(&rows, ENTREE_MORLOX_1, "tiebreak").await, 0.5);
    // The DM moves a participant up.
    applied(
        &written(
            &engine,
            "test.updateEntry",
            json!({ "initiativeEntryId": ENTREE_MORLOX_1, "rank": 1 }),
        )
        .await,
    );
}

#[tokio::test]
async fn hidden_keeps_rank_moves_no_rank_when_a_token_is_hidden_or_shown() {
    let (engine, rows) = tavern().await;
    // Hidden, then shown: the token's alone.
    for visible in [false, true] {
        applied(
            &written(
                &engine,
                "test.updateToken",
                json!({ "tokenId": MORLOX_1, "visible": visible }),
            )
            .await,
        );
    }
    assert_eq!(held(&rows, ENTREE_MORLOX_1, "rank").await, 2);
    // A reveal that reorders, by its payload or by its touches.
    rejected_among(
        &written(
            &engine,
            "test.updateToken",
            json!({ "tokenId": MORLOX_1, "visible": false, "rank": 1 }),
        )
        .await,
        "hidden-keeps-rank",
    );
    rejected_by(
        &written(
            &engine,
            "test.revealAndRerank",
            json!({ "tokenId": MORLOX_1, "visible": true }),
        )
        .await,
        &["hidden-keeps-rank"],
    );
    // The DM reorders on the entry, the token's visibility untouched.
    applied(
        &written(
            &engine,
            "test.updateEntry",
            json!({ "initiativeEntryId": ENTREE_MORLOX_1, "rank": 3 }),
        )
        .await,
    );
    // The order is the view's.
    for (field, value) in [
        ("orderIndex", json!(1)),
        ("entryId", json!(ENTREE_MORLOX_1)),
        ("initiativeOrderViewCombatId", json!(COMBAT)),
    ] {
        rejected_by(
            &written(
                &engine,
                "test.updateToken",
                json!({ "tokenId": MORLOX_1, field: value }),
            )
            .await,
            &["hidden-keeps-rank"],
        );
    }
}

#[tokio::test]
async fn next_turn_starts_at_round_1_and_steps_one_round_at_most() {
    let (engine, rows) = tavern().await;
    for round in [json!(0), json!(1.5)] {
        rejected_by(
            &written(
                &engine,
                "test.updateCombat",
                json!({ "combatId": COMBAT, "round": round }),
            )
            .await,
            &["next-turn"],
        );
    }
    rejected_by(
        &written(
            &engine,
            "test.startCombat",
            with(
                with(
                    combat(),
                    "combatId",
                    json!("c0000000-0000-4000-8000-000000000002"),
                ),
                "round",
                json!(7),
            ),
        )
        .await,
        &["next-turn"],
    );
    // « Suivant » within the round, then past the last rank.
    for (step, current) in [(0, ENTREE_MORLOX_1), (1, ENTREE_ELOWEN)] {
        applied(
            &written(
                &engine,
                "test.nextTurn",
                json!({ "combatId": COMBAT, "round": step, "currentEntryId": current }),
            )
            .await,
        );
    }
    assert_eq!(held(&rows, COMBAT, "round").await, 2);
    // Two rounds at once, or one back.
    for step in [2, -1] {
        rejected_by(
            &written(
                &engine,
                "test.nextTurn",
                json!({ "combatId": COMBAT, "round": step, "currentEntryId": ENTREE_ELOWEN }),
            )
            .await,
            &["next-turn"],
        );
    }
    // Written whole: never below the round seen, never up on no one.
    for row in [
        json!({ "combatId": COMBAT, "round": 1 }),
        json!({ "combatId": COMBAT, "round": 3, "currentEntryId": null }),
    ] {
        rejected_by(
            &written(&engine, "test.updateCombat", row).await,
            &["next-turn"],
        );
    }
    assert_eq!(held(&rows, COMBAT, "round").await, 2);
}

#[tokio::test]
async fn next_turn_queued_twice_steps_twice_and_never_past_an_end_queued_ahead() {
    let (engine, rows) = tavern().await;
    // The DM clicks « Suivant » twice at the wrap: both wait, both land.
    for current in [ENTREE_MORLOX_1, ENTREE_ELOWEN] {
        queued(
            &engine,
            "test.nextTurn",
            json!({ "combatId": COMBAT, "round": 1, "currentEntryId": current }),
        )
        .await;
    }
    let results = engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), 2, "{results:?}");
    results.iter().for_each(applied);
    assert_eq!(held(&rows, COMBAT, "round").await, 3);
    // The combat ends, then « Suivant » arrives: the end, queued ahead, wins.
    queued(
        &engine,
        "test.updateCombat",
        json!({ "combatId": COMBAT, "endedAt": "2026-10-18T20:45:00Z" }),
    )
    .await;
    queued(
        &engine,
        "test.nextTurn",
        json!({ "combatId": COMBAT, "round": 0, "currentEntryId": ENTREE_MORLOX_1 }),
    )
    .await;
    let results = engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), 2, "{results:?}");
    applied(&results[0]);
    rejected_by(&results[1], &["next-turn"]);
    // Ended, the combat turns no more.
    for row in [
        json!({ "combatId": COMBAT, "round": 4, "currentEntryId": ENTREE_MORLOX_1 }),
        json!({ "combatId": COMBAT, "currentEntryId": ENTREE_MORLOX_1 }),
    ] {
        rejected_by(
            &written(&engine, "test.updateCombat", row).await,
            &["next-turn"],
        );
    }
    assert_eq!(held(&rows, COMBAT, "currentEntryId").await, ENTREE_ELOWEN);
}

#[tokio::test]
async fn one_active_combat_starts_on_an_instance_and_stays_ended() {
    let (engine, _) = tavern().await;
    for field in ["combatInstanceId", "startedAt"] {
        rejected_by(
            &written(
                &engine,
                "test.startCombat",
                with(
                    without(combat(), field),
                    "combatId",
                    json!("c0000000-0000-4000-8000-000000000002"),
                ),
            )
            .await,
            &["one-active-combat"],
        );
    }
    applied(
        &written(
            &engine,
            "test.updateCombat",
            json!({ "combatId": COMBAT, "endedAt": "2026-10-18T20:45:00Z" }),
        )
        .await,
    );
    // Reopened, or ended at another time, or started again.
    for (field, value) in [
        ("endedAt", Value::Null),
        ("endedAt", json!("2026-10-18T22:00:00Z")),
        ("startedAt", json!("2026-10-18T21:00:00Z")),
    ] {
        rejected_by(
            &written(
                &engine,
                "test.updateCombat",
                json!({ "combatId": COMBAT, field: value }),
            )
            .await,
            &["one-active-combat"],
        );
    }
    // The next one is a combat of its own.
    applied(
        &written(
            &engine,
            "test.startCombat",
            with(
                with(
                    combat(),
                    "combatId",
                    json!("c0000000-0000-4000-8000-000000000002"),
                ),
                "startedAt",
                json!("2026-10-18T21:00:00Z"),
            ),
        )
        .await,
    );
}

#[tokio::test]
async fn state_persisted_reset_keeps_the_last_reset_and_removes_what_it_clears() {
    let (engine, rows) = tavern().await;
    applied(
        &written(
            &engine,
            "test.updateInstance",
            json!({ "battlemapInstanceId": INSTANCE, "resetAt": "2026-10-18T21:00:00Z" }),
        )
        .await,
    );
    for (field, value) in [
        ("resetAt", Value::Null),
        ("createdAt", json!("2026-10-19T20:00:00Z")),
    ] {
        rejected_by(
            &written(
                &engine,
                "test.updateInstance",
                json!({ "battlemapInstanceId": INSTANCE, field: value }),
            )
            .await,
            &["state-persisted-reset"],
        );
    }
    // A later reset replaces it.
    applied(
        &written(
            &engine,
            "test.updateInstance",
            json!({ "battlemapInstanceId": INSTANCE, "resetAt": "2026-10-18T22:00:00Z" }),
        )
        .await,
    );
    // A token's conditions are a list of names, persisted as given.
    for conditions in [json!("À terre"), json!([""]), json!([3]), Value::Null] {
        rejected_by(
            &written(
                &engine,
                "test.updateToken",
                json!({ "tokenId": MORLOX_1, "conditions": conditions }),
            )
            .await,
            &["state-persisted-reset"],
        );
    }
    applied(
        &written(
            &engine,
            "test.updateToken",
            json!({ "tokenId": MORLOX_1, "conditions": ["À terre", "Empoisonné"] }),
        )
        .await,
    );
    assert_eq!(
        held(&rows, MORLOX_1, "conditions").await,
        json!(["À terre", "Empoisonné"])
    );
    // « Réinitialiser la carte » removes the token: a tombstone, whatever
    // the row held.
    applied(
        &written(
            &engine,
            "test.removeToken",
            json!({ "tokenId": MORLOX_1, "hpCurrent": 99, "dead": true }),
        )
        .await,
    );
}

#[tokio::test]
async fn single_position_source_stands_a_token_on_one_whole_hex() {
    let (engine, rows) = tavern().await;
    for (field, value) in [
        ("q", Value::Null),
        ("r", Value::Null),
        ("q", json!(1.5)),
        ("r", json!("2")),
        ("q", json!(3_000_000_000_i64)),
    ] {
        rejected_by(
            &written(
                &engine,
                "test.updateToken",
                json!({ "tokenId": MORLOX_1, field: value }),
            )
            .await,
            &["single-position-source"],
        );
    }
    applied(
        &written(
            &engine,
            "test.updateToken",
            json!({ "tokenId": MORLOX_1, "q": -2, "r": 4 }),
        )
        .await,
    );
    assert_eq!(held(&rows, MORLOX_1, "q").await, -2);
    // A position of another kind is not the aggregate's.
    rejected_by(
        &written(
            &engine,
            "test.updateToken",
            json!({ "tokenId": MORLOX_1, "x": 120.5 }),
        )
        .await,
        &["instance-isolated"],
    );
}

#[tokio::test]
async fn another_aggregate_s_hp_and_position_are_not_the_battlemap_s() {
    let (engine, _) = engine().await;
    applied(
        &written(
            &engine,
            "test.updateCampaign",
            json!({
                "id": VALOMBRE,
                "kind": "campagne",
                "hpCurrent": 99,
                "dead": true,
                "q": 1.5,
                "round": 0,
            }),
        )
        .await,
    );
}
