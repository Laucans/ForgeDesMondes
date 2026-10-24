//! table.setTokenVisibility as the server plants it beside its target
//! aggregate, over the in-memory store: a token of the instance is hidden or
//! revealed on its own partition, and nothing else changes; in a combat, the
//! hidden participant of rank 2 keeps its rank, and revealing it moves no
//! rank (`hidden-keeps-rank`); a token of another instance, or one the
//! engine does not hold, is refused before anything is queued
//! (`instance-isolated`). The instances, tokens, combat and entries are
//! planted by test-only DataCapabilities, as the aggregate's own tests plant
//! them.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};

const CARTE: &str = "d1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const EMBUSCADE: &str = "5c000000-0000-4000-8000-000000000002";
/// The tavern's instance, and the ambush's.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000002";
const VESNA: &str = "e1000000-0000-4000-8000-000000000003";
/// The tavern's tokens: Elowen, Bandit 1 and Vesna 1, the ambush's chief.
const JETON_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const BANDIT_1: &str = "70000000-0000-4000-8000-000000000002";
const VESNA_1: &str = "70000000-0000-4000-8000-000000000003";
/// The ambush's token of Vesna.
const VESNA_AILLEURS: &str = "70000000-0000-4000-8000-000000000004";
const NOWHERE: &str = "70000000-0000-4000-8000-0000000000ff";
/// The tavern's combat and its three entries: Elowen 1st, Vesna 2nd, the
/// bandit 3rd.
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const ENTREE_ELOWEN: &str = "ee000000-0000-4000-8000-000000000001";
const ENTREE_VESNA: &str = "ee000000-0000-4000-8000-000000000002";
const ENTREE_BANDIT: &str = "ee000000-0000-4000-8000-000000000003";

/// The test-only DataCapabilities planting the rows a change lands among.
const OPEN_INSTANCE: &str = r#"{"dataCapability":"test.openInstance","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$battlemapInstanceId"},"mode":"overwrite","callableBy":["*"]}"#;
const DROP_TOKEN: &str = r#"{"dataCapability":"test.dropToken","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"mode":"overwrite","callableBy":["*"]}"#;
const START_COMBAT: &str = r#"{"dataCapability":"test.startCombat","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$combatId"},"mode":"overwrite","callableBy":["*"]}"#;
const ENTER: &str = r#"{"dataCapability":"test.enter","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$initiativeEntryId"},"mode":"overwrite","callableBy":["*"]}"#;

fn aggregate() -> Aggregate {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../aggregates/BattlemapInstance/aggregate.json");
    Aggregate::parse(&std::fs::read_to_string(path).expect("aggregate.json")).expect("contract I")
}

fn manifest(json: &str) -> DataCapability {
    DataCapability::parse(json).expect("contract F")
}

fn plugins() -> Plugins {
    [
        aggregate_battlemap_instance::plugin(),
        set_token_visibility::plugin(),
    ]
    .into_iter()
    .collect()
}

/// The engine as the server plants this unit and its aggregate, over an
/// in-memory store, the tavern's and the ambush's instances and tokens
/// planted, and the tavern's combat with its three entries; and the store,
/// whose rows a test reads.
async fn engine() -> (Engine, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::default());
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at(1_792_000_000)),
    )
    .with_aggregate(aggregate())
    .with_data_capability(manifest(include_str!("../data-capability.json")))
    .with_data_capability(manifest(OPEN_INSTANCE))
    .with_data_capability(manifest(DROP_TOKEN))
    .with_data_capability(manifest(START_COMBAT))
    .with_data_capability(manifest(ENTER))
    .with_plugins(plugins());
    engine.load().await.expect("the in-memory engine");
    for (id, scene) in [(INSTANCE, ARRIVEE), (AUTRE_INSTANCE, EMBUSCADE)] {
        submit(
            &engine,
            "test.openInstance",
            json!({
                "battlemapInstanceId": id,
                "battlemapMediaId": CARTE,
                "sessionId": SEANCE,
                "sceneId": scene,
                "createdAt": "2026-10-21T10:00:00Z",
            }),
            None,
        )
        .await
        .expect("an instance");
    }
    for token in [
        token(JETON_ELOWEN, INSTANCE, ELOWEN, None, "Elowen"),
        token(BANDIT_1, INSTANCE, BANDIT, Some(1), "Bandit 1"),
        token(VESNA_1, INSTANCE, VESNA, Some(1), "Vesna 1"),
        token(VESNA_AILLEURS, AUTRE_INSTANCE, VESNA, Some(1), "Vesna 1"),
    ] {
        submit(&engine, "test.dropToken", token, None)
            .await
            .expect("a token");
    }
    submit(
        &engine,
        "test.startCombat",
        json!({
            "combatId": COMBAT,
            "combatInstanceId": INSTANCE,
            "round": 1,
            "currentEntryId": ENTREE_ELOWEN,
            "startedAt": "2026-10-21T10:05:00Z",
        }),
        None,
    )
    .await
    .expect("the combat");
    // Elowen's 18, the DM's; Vesna's 12 + 3 = 15; the bandit's 3 + 1 = 4.
    for entry in [
        entry(ENTREE_ELOWEN, JETON_ELOWEN, None, (18, 14), 1),
        entry(ENTREE_VESNA, VESNA_1, Some(12), (15, 16), 2),
        entry(ENTREE_BANDIT, BANDIT_1, Some(3), (4, 12), 3),
    ] {
        submit(&engine, "test.enter", entry, None)
            .await
            .expect("an entry");
    }
    let planted = engine.run_once().await.expect("planted");
    assert!(
        planted.iter().all(|result| result.status == State::Applied),
        "{planted:?}"
    );
    (engine, store)
}

/// A token as `table.placeTokens` drops it, visible: an NPC's numbered,
/// with its HP.
fn token(id: &str, instance: &str, character: &str, number: Option<i64>, name: &str) -> Value {
    let hp = number.map(|_| 11);
    json!({
        "tokenId": id,
        "tokenInstanceId": instance,
        "characterId": character,
        "number": number,
        "name": name,
        "q": 1,
        "r": 1,
        "visible": true,
        "hpCurrent": hp,
        "hpMax": hp,
        "conditions": [],
    })
}

/// A participant's place in the tavern's combat: its roll, its initiative
/// and DEX score, its rank.
fn entry(
    id: &str,
    token: &str,
    roll: Option<i64>,
    (initiative, dex): (i64, i64),
    rank: i64,
) -> Value {
    json!({
        "initiativeEntryId": id,
        "initiativeEntryCombatId": COMBAT,
        "initiativeEntryTokenId": token,
        "roll": roll,
        "initiative": initiative,
        "dexScore": dex,
        "tiebreak": 0.5,
        "rank": rank,
    })
}

/// A command as the Battlemap sends it, by the DM, on `target`.
async fn submit(
    engine: &Engine,
    data_capability: &str,
    payload: Value,
    target: Option<&str>,
) -> Result<Submitted> {
    let mut body = json!({
        "dataCapability": data_capability,
        "target": { "aggregate": "BattlemapInstance" },
        "payload": payload,
        "caller": { "by": "dm", "from": "Battlemap", "roles": ["role:approver"] },
    });
    if let Some(target) = target {
        body["target"]["id"] = json!(target);
    }
    engine
        .submit(serde_json::from_value::<Submission>(body).expect("a submission"))
        .await
}

/// `token` hidden or revealed on the tavern's instance, queued then applied.
async fn set(engine: &Engine, token: &str, visible: bool) -> CommandResult {
    submit(
        engine,
        "table.setTokenVisibility@1",
        json!({ "token": token, "visible": visible }),
        Some(INSTANCE),
    )
    .await
    .expect("queued");
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

/// The row of the partition `id`.
fn row(store: &MemoryStore, id: &str) -> Value {
    Value::Object(
        store
            .row_now(&format!("BattlemapInstance/{id}"))
            .into_iter()
            .collect(),
    )
}

/// Every row of the tavern's and the ambush's instances, by partition.
fn every_row(store: &MemoryStore) -> Vec<(&'static str, Value)> {
    [
        INSTANCE,
        AUTRE_INSTANCE,
        JETON_ELOWEN,
        BANDIT_1,
        VESNA_1,
        VESNA_AILLEURS,
        COMBAT,
        ENTREE_ELOWEN,
        ENTREE_VESNA,
        ENTREE_BANDIT,
    ]
    .into_iter()
    .map(|id| (id, row(store, id)))
    .collect()
}

/// The combat's participants by rank, as the carousel orders them.
fn order(store: &MemoryStore) -> Vec<(Value, Value)> {
    let mut entries: Vec<(Value, Value)> = [ENTREE_ELOWEN, ENTREE_VESNA, ENTREE_BANDIT]
        .into_iter()
        .map(|id| {
            let entry = row(store, id);
            (
                entry["rank"].clone(),
                entry["initiativeEntryTokenId"].clone(),
            )
        })
        .collect();
    entries.sort_by_key(|(rank, _)| rank.as_i64());
    entries
}

/// The refusal `answer` is, of `kind`: its message.
fn refused(answer: Result<Submitted>, kind: RefusalKind) -> String {
    match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a {kind:?} refusal: {other:?}"),
    }
}

#[tokio::test]
async fn a_visible_token_is_hidden_and_nothing_else_changes() {
    let (engine, store) = engine().await;
    let before = every_row(&store);

    let result = set(&engine, VESNA_1, false).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(row(&store, VESNA_1)["visible"], json!(false));
    // Every row as it was, but Vesna 1's visibility.
    let mut expected = before;
    for (id, row) in &mut expected {
        if *id == VESNA_1 {
            row["visible"] = json!(false);
        }
    }
    assert_eq!(every_row(&store), expected);
}

#[tokio::test]
async fn hidden_keeps_rank_the_hidden_participant_of_rank_2_is_revealed_and_no_rank_moves() {
    let (engine, store) = engine().await;
    let ranks = vec![
        (json!(1), json!(JETON_ELOWEN)),
        (json!(2), json!(VESNA_1)),
        (json!(3), json!(BANDIT_1)),
    ];
    assert_eq!(order(&store), ranks);
    let combat = row(&store, COMBAT);

    assert_eq!(set(&engine, VESNA_1, false).await.status, State::Applied);
    assert_eq!(row(&store, VESNA_1)["visible"], json!(false));
    assert_eq!(order(&store), ranks, "hidden");
    let hidden = every_row(&store);

    assert_eq!(set(&engine, VESNA_1, true).await.status, State::Applied);
    assert_eq!(row(&store, VESNA_1)["visible"], json!(true));
    assert_eq!(order(&store), ranks, "revealed");
    // Nothing of the combat moved: its turn, its round, every entry.
    assert_eq!(row(&store, COMBAT), combat);
    for (id, before) in hidden {
        if id != VESNA_1 {
            assert_eq!(row(&store, id), before, "{id}");
        }
    }
}

#[tokio::test]
async fn the_change_lands_on_the_token_s_partition_named_in_any_case() {
    let (engine, store) = engine().await;
    let queued = submit(
        &engine,
        "table.setTokenVisibility@1",
        json!({ "token": format!(" {} ", BANDIT_1.to_uppercase()), "visible": false }),
        Some(&INSTANCE.to_uppercase()),
    )
    .await
    .expect("queued");
    assert_eq!(queued.partition, format!("BattlemapInstance/{BANDIT_1}"));
    engine.run_once().await.expect("applied");
    assert_eq!(row(&store, BANDIT_1)["visible"], json!(false));
}

#[tokio::test]
async fn without_a_target_the_payload_s_instance_is_the_one_the_token_must_stand_on() {
    let (engine, store) = engine().await;
    submit(
        &engine,
        "table.setTokenVisibility@1",
        json!({ "instance": INSTANCE, "token": JETON_ELOWEN, "visible": false }),
        None,
    )
    .await
    .expect("queued");
    engine.run_once().await.expect("applied");
    assert_eq!(row(&store, JETON_ELOWEN)["visible"], json!(false));
}

#[tokio::test]
async fn state_persisted_reset_the_last_of_two_changes_queued_is_what_the_token_shows() {
    let (engine, store) = engine().await;
    for visible in [false, true, false] {
        let queued = submit(
            &engine,
            "table.setTokenVisibility@1",
            json!({ "token": VESNA_1, "visible": visible }),
            Some(INSTANCE),
        )
        .await
        .expect("queued");
        // A toggle is the DM's latest: never held for a confirmation.
        assert_eq!(queued.result.status, State::Queued, "{queued:?}");
    }
    let settled = engine.run_once().await.expect("applied");
    assert_eq!(settled.len(), 3, "{settled:?}");
    assert!(settled.iter().all(|result| result.status == State::Applied));
    assert_eq!(row(&store, VESNA_1)["visible"], json!(false));
}

#[tokio::test]
async fn instance_isolated_a_token_of_another_instance_is_refused_and_stays_as_it_was() {
    let (engine, store) = engine().await;
    let message = refused(
        submit(
            &engine,
            "table.setTokenVisibility@1",
            json!({ "token": VESNA_AILLEURS, "visible": false }),
            Some(INSTANCE),
        )
        .await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("instance-isolated"), "{message}");
    assert!(engine.run_once().await.expect("ran").is_empty(), "queued");
    assert_eq!(row(&store, VESNA_AILLEURS)["visible"], json!(true));
}

#[tokio::test]
async fn a_token_the_engine_does_not_hold_is_not_found_nor_is_an_instance_or_an_entry() {
    let (engine, store) = engine().await;
    for token in [NOWHERE, INSTANCE, ENTREE_VESNA] {
        let message = refused(
            submit(
                &engine,
                "table.setTokenVisibility@1",
                json!({ "token": token, "visible": false }),
                Some(INSTANCE),
            )
            .await,
            RefusalKind::NotFound,
        );
        assert!(message.contains("token"), "{token}: {message}");
    }
    assert!(engine.run_once().await.expect("ran").is_empty(), "queued");
    assert!(
        store
            .row_now(&format!("BattlemapInstance/{NOWHERE}"))
            .is_empty()
    );
    assert!(row(&store, ENTREE_VESNA).get("visible").is_none());
}

#[tokio::test]
async fn a_change_naming_no_instance_no_token_or_no_visibility_is_refused() {
    let (engine, _) = engine().await;
    for (payload, target, words) in [
        (
            json!({ "token": VESNA_1, "visible": false }),
            None,
            "instance",
        ),
        (json!({ "visible": false }), Some(INSTANCE), "token"),
        (json!({ "token": VESNA_1 }), Some(INSTANCE), "visible"),
        (
            json!({ "token": VESNA_1, "visible": "non" }),
            Some(INSTANCE),
            "visible",
        ),
    ] {
        let message = refused(
            submit(
                &engine,
                "table.setTokenVisibility@1",
                payload.clone(),
                target,
            )
            .await,
            RefusalKind::Unprocessable,
        );
        assert!(message.starts_with(words), "{payload}: {message}");
    }
    assert!(engine.run_once().await.expect("ran").is_empty(), "queued");
}
