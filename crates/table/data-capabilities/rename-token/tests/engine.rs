//! table.renameToken as the server plants it beside its target aggregate,
//! over the engine's store in memory: three Bandit tokens numbered 1 to 3,
//! the second renamed — only its name changes, and the three numbers stay
//! (`multi-drop-numbering`); an empty name, a token of another instance or
//! one the engine does not hold is refused before anything is queued.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::{Error, Result};
use rename_token::{Row, partition};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
const NOWHERE: &str = "70000000-0000-4000-8000-0000000000ff";

/// The id of Bandit `number`.
fn bandit_id(number: u8) -> String {
    format!("70000000-0000-4000-8000-00000000000{number}")
}

/// Bandit `number` as table.placeTokens left it on the instance.
fn bandit(number: u8) -> Row {
    let Value::Object(map) = json!({
        "tokenId": bandit_id(number),
        "tokenInstanceId": INSTANCE,
        "characterId": BANDIT,
        "number": number,
        "name": format!("Bandit {number}"),
        "q": 5,
        "r": i64::from(number),
        "visible": true,
        "hpCurrent": 11,
        "hpMax": 11,
        "conditions": [],
    }) else {
        unreachable!("a row is an object");
    };
    map.into_iter().collect()
}

fn aggregate() -> Aggregate {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../aggregates/BattlemapInstance/aggregate.json");
    Aggregate::parse(&std::fs::read_to_string(path).expect("aggregate.json")).expect("contract I")
}

fn manifest() -> DataCapability {
    DataCapability::parse(include_str!("../data-capability.json")).expect("contract F")
}

/// The engine as the server plants this unit and its aggregate, over a
/// store holding Bandit 1 to 3; and that store.
async fn planted() -> (Engine, Arc<MemoryStore>) {
    let store = Arc::new((1..=3).fold(MemoryStore::default(), |store, number| {
        store.with_row(&partition(&bandit_id(number)), bandit(number))
    }));
    let plugins: Plugins = [
        aggregate_battlemap_instance::plugin(),
        rename_token::plugin(),
    ]
    .into_iter()
    .collect();
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at(1_792_000_000)),
    )
    .with_aggregate(aggregate())
    .with_data_capability(manifest())
    .with_plugins(plugins);
    engine.load().await.expect("the engine loads");
    (engine, store)
}

/// A rename as the Battlemap sends it, by the DM, to `target`.
async fn rename(engine: &Engine, target: &str, payload: Value) -> Result<Submitted> {
    let body = json!({
        "dataCapability": "table.renameToken@1",
        "target": { "aggregate": "BattlemapInstance", "id": target },
        "payload": payload,
        "caller": { "by": "dm", "from": "Battlemap" },
    });
    engine
        .submit(serde_json::from_value::<Submission>(body).expect("a submission"))
        .await
}

/// Every command queued, applied.
async fn settle(engine: &Engine) {
    for result in engine.run_once().await.expect("the applier ran") {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
}

/// The refusal `answer` is, of `kind`: its message.
fn refused(answer: Result<Submitted>, kind: RefusalKind) -> String {
    match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a {kind:?} refusal: {other:?}"),
    }
}

#[tokio::test]
async fn multi_drop_numbering_renaming_bandit_2_touches_it_alone() {
    let (engine, store) = planted().await;
    let submitted = rename(
        &engine,
        INSTANCE,
        json!({ "token": bandit_id(2), "name": " Bandit blessé " }),
    )
    .await
    .expect("queued");
    // The command is written on the token's partition, not the instance's.
    assert_eq!(submitted.partition, partition(&bandit_id(2)));
    settle(&engine).await;

    let mut renamed = bandit(2);
    renamed.insert("name".to_owned(), json!("Bandit blessé"));
    assert_eq!(store.row_now(&partition(&bandit_id(2))), renamed);
    for number in [1, 3] {
        assert_eq!(
            store.row_now(&partition(&bandit_id(number))),
            bandit(number)
        );
    }
    let numbers: Vec<Value> = (1..=3)
        .map(|number| store.row_now(&partition(&bandit_id(number)))["number"].clone())
        .collect();
    assert_eq!(numbers, [json!(1), json!(2), json!(3)]);
}

#[tokio::test]
async fn a_token_targeted_itself_is_renamed_too() {
    let (engine, store) = planted().await;
    rename(&engine, &bandit_id(3), json!({ "name": "Bandit borgne" }))
        .await
        .expect("queued");
    settle(&engine).await;
    assert_eq!(
        store.row_now(&partition(&bandit_id(3)))["name"],
        "Bandit borgne"
    );
}

#[tokio::test]
async fn an_empty_name_is_refused_and_nothing_is_queued() {
    let (engine, store) = planted().await;
    for name in [json!(""), json!("  "), json!(null)] {
        let message = refused(
            rename(
                &engine,
                INSTANCE,
                json!({ "token": bandit_id(2), "name": name }),
            )
            .await,
            RefusalKind::Unprocessable,
        );
        assert!(message.contains("multi-drop-numbering"), "{message}");
    }
    assert!(engine.run_once().await.expect("ran").is_empty());
    assert_eq!(store.row_now(&partition(&bandit_id(2))), bandit(2));
}

#[tokio::test]
async fn a_token_of_another_instance_or_not_held_is_not_found() {
    let (engine, store) = planted().await;
    for (target, token) in [
        (AUTRE_INSTANCE, bandit_id(2)),
        (INSTANCE, NOWHERE.to_owned()),
    ] {
        refused(
            rename(
                &engine,
                target,
                json!({ "token": token, "name": "Bandit blessé" }),
            )
            .await,
            RefusalKind::NotFound,
        );
    }
    assert!(engine.run_once().await.expect("ran").is_empty());
    assert_eq!(store.row_now(&partition(&bandit_id(2))), bandit(2));
}

#[tokio::test]
async fn a_second_rename_queued_behind_the_first_renames_the_token_as_it_will_be() {
    let (engine, store) = planted().await;
    rename(
        &engine,
        INSTANCE,
        json!({ "token": bandit_id(1), "name": "Bandit blessé" }),
    )
    .await
    .expect("queued");
    let second = rename(
        &engine,
        INSTANCE,
        json!({ "token": bandit_id(1), "name": "Bandit mort" }),
    )
    .await
    .expect("queued");
    // A value is declared ahead of it (confirm_on_stale): never rejected,
    // its author confirms the overwrite.
    assert_eq!(second.result.status, State::AwaitingConfirmation);
    engine
        .confirm(&second.command_id, "dm")
        .await
        .expect("confirmed");
    settle(&engine).await;
    let row = store.row_now(&partition(&bandit_id(1)));
    assert_eq!(row["name"], "Bandit mort");
    assert_eq!(row["number"], 1);
}
