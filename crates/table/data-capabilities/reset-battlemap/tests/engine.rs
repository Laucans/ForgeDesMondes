//! table.resetBattlemap as the server plants it beside its target
//! aggregate, over the engine's store in memory: an instance and its
//! Bandit. With no tables to list the instance's rows, no reset is queued;
//! one naming no uuid is refused before anything else.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::{Error, Result};
use reset_battlemap::{Row, partition};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "70000000-0000-4000-8000-000000000001";

fn row(value: Value) -> Row {
    let Value::Object(map) = value else {
        unreachable!("a row is an object");
    };
    map.into_iter().collect()
}

async fn planted() -> (Engine, Arc<MemoryStore>) {
    let store = Arc::new(
        MemoryStore::default()
            .with_row(
                &partition(INSTANCE),
                row(json!({ "battlemapInstanceId": INSTANCE, "createdAt": "2026-10-21T10:00:00Z" })),
            )
            .with_row(
                &partition(BANDIT),
                row(json!({ "tokenId": BANDIT, "tokenInstanceId": INSTANCE, "name": "Bandit 1", "q": 0, "r": 0, "conditions": [] })),
            ),
    );
    let aggregate = Aggregate::parse(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../aggregates/BattlemapInstance/aggregate.json"),
        )
        .expect("aggregate.json"),
    )
    .expect("contract I");
    let plugins: Plugins = [
        aggregate_battlemap_instance::plugin(),
        reset_battlemap::plugin(),
    ]
    .into_iter()
    .collect();
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at(1_792_000_000)),
    )
    .with_aggregate(aggregate)
    .with_data_capability(
        DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
    )
    .with_plugins(plugins);
    engine.load().await.expect("the engine loads");
    (engine, store)
}

async fn reset(engine: &Engine, target: &str, payload: Value) -> Result<Submitted> {
    let body = json!({
        "dataCapability": "table.resetBattlemap@1",
        "target": { "aggregate": "BattlemapInstance", "id": target },
        "payload": payload,
        "caller": { "by": "dm", "from": "Battlemap", "roles": ["role:approver"] },
    });
    engine
        .submit(serde_json::from_value::<Submission>(body).expect("a submission"))
        .await
}

fn refused(answer: Result<Submitted>, kind: RefusalKind) -> String {
    match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a {kind:?} refusal: {other:?}"),
    }
}

#[tokio::test]
async fn over_memory_no_map_is_listed_so_none_is_reset() {
    let (engine, store) = planted().await;
    let message = refused(
        reset(&engine, INSTANCE, json!({})).await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("n'a pas pu être lue"), "{message}");
    // A deletion of the reset is listed alike.
    refused(
        reset(
            &engine,
            BANDIT,
            json!({ "of": INSTANCE, "resetAt": "2026-10-21T11:00:00.000Z" }),
        )
        .await,
        RefusalKind::Unprocessable,
    );
    assert!(engine.run_once().await.expect("ran").is_empty());
    assert!(!store.is_tombstoned(&partition(BANDIT)));
}

#[tokio::test]
async fn a_reset_naming_no_uuid_is_refused_before_anything_is_read() {
    let (engine, _) = planted().await;
    for (target, payload, field) in [
        ("pas-un-id", json!({}), "instance"),
        (BANDIT, json!({ "of": "pas-un-id" }), "of"),
        ("pas-un-id", json!({ "of": INSTANCE }), "row"),
    ] {
        let message = refused(
            reset(&engine, target, payload).await,
            RefusalKind::Unprocessable,
        );
        assert_eq!(message, format!("{field} n'est pas un identifiant"));
    }
}
