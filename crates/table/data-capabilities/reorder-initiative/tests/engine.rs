//! table.reorderInitiative over the in-memory store, which holds no tables:
//! the combat's participants cannot be listed, so a reorder is refused
//! (422) before anything is queued, never guessed from the queue alone.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{Plugins, RefusalKind};
use serde_json::json;

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const T_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";

#[tokio::test(flavor = "current_thread")]
async fn over_memory_the_participants_cannot_be_listed_and_the_reorder_is_refused() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../aggregates/BattlemapInstance/aggregate.json");
    let aggregate = Aggregate::parse(&std::fs::read_to_string(path).expect("aggregate.json"))
        .expect("contract I");
    let plugins: Plugins = [
        aggregate_battlemap_instance::plugin(),
        reorder_initiative::plugin(),
    ]
    .into_iter()
    .collect();
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at(1_792_000_000)),
    )
    .with_aggregate(aggregate)
    .with_data_capability(
        DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
    )
    .with_plugins(plugins);
    engine.load().await.expect("the in-memory engine");

    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "table.reorderInitiative@1",
        "target": { "aggregate": "BattlemapInstance", "id": INSTANCE },
        "payload": { "order": [T_ELOWEN] },
        "caller": { "by": "dm", "from": "Battlemap", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    match engine.submit(submission).await {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::Unprocessable);
            assert!(
                refusal.message.contains("n'ont pas pu être lus"),
                "{}",
                refusal.message
            );
        }
        other => panic!("refused, not {other:?}"),
    }
    assert!(engine.run_once().await.expect("a run").is_empty());
}
