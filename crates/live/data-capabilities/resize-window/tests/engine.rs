//! live.resizeWindow through the engine over its memory, as the server plants
//! it with no database (`DATABASE_URL=memory`): the engine has no table, so
//! no live session is known and every resize is refused before it is queued.
//! What the tables answer is `postgres.rs`'s.

use std::sync::Arc;

use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::postgres::SystemClock;
use serde_json::{Value, json};

async fn engine() -> Engine {
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(aggregate_diffusion::DOCUMENT).expect("contract I"))
    .with_data_capability(
        DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
    )
    .with_plugins(
        [aggregate_diffusion::plugin(), resize_window::plugin()]
            .into_iter()
            .collect(),
    );
    engine.load().await.expect("the engine loads");
    engine
}

fn submission(payload: Value) -> Submission {
    serde_json::from_value(json!({
        "dataCapability": "live.resizeWindow@1",
        "payload": payload,
        "caller": { "by": "dm", "from": "PlayerStage", "roles": [] },
    }))
    .expect("a submission")
}

#[tokio::test]
async fn over_memory_no_live_session_shows_the_window_and_a_resize_is_refused() {
    let engine = engine().await;
    for (payload, why) in [
        (
            json!({ "campaign": "c1000000-0000-4000-8000-000000000001", "w": 0.4, "h": 0.6 }),
            "aucune séance n'est en cours",
        ),
        (json!({ "w": 0.4, "h": 0.6 }), "campaign manque"),
        (
            json!({ "campaign": "c1000000-0000-4000-8000-000000000001" }),
            "w manque",
        ),
    ] {
        match engine.submit(submission(payload)).await {
            Err(Error::Refused(refusal)) => {
                assert_eq!(refusal.kind, RefusalKind::Unprocessable, "{why}");
                assert_eq!(refusal.message, why);
            }
            other => panic!("{why}: {other:?}"),
        }
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
}
