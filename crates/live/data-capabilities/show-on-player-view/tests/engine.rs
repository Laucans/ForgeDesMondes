//! live.showOnPlayerView through the engine over its memory, as the server
//! plants it with no database (`DATABASE_URL=memory`): the engine has no
//! table, so no live session is known and every send is refused before it
//! is queued. What the tables answer is `postgres.rs`'s.

use std::sync::Arc;

use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::postgres::SystemClock;
use serde_json::json;

#[tokio::test]
async fn over_memory_no_live_session_shows_a_send_and_it_is_refused() {
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
        [aggregate_diffusion::plugin(), show_on_player_view::plugin()]
            .into_iter()
            .collect(),
    );
    engine.load().await.expect("the engine loads");
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "live.showOnPlayerView@1",
        "payload": {
            "campaign": "c1000000-0000-4000-8000-000000000001",
            "kind": "illustration",
            "media": "d1000000-0000-4000-8000-000000000002",
        },
        "caller": { "by": "dm", "from": "table.Battlemap", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    match engine.submit(submission).await {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::Unprocessable);
            assert_eq!(refusal.message, "aucune séance n'est en cours");
        }
        other => panic!("{other:?}"),
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
}
