//! live.showWindowOnPlayerView through the engine over its memory, as the
//! server plants it with no database (`DATABASE_URL=memory`): the engine has
//! no table, so no live session is known and every window is refused before
//! it is queued. What the tables answer is `postgres.rs`'s.

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

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";

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
        [
            aggregate_diffusion::plugin(),
            show_window_on_player_view::plugin(),
        ]
        .into_iter()
        .collect(),
    );
    engine.load().await.expect("the engine loads");
    engine
}

fn submission(payload: Value) -> Submission {
    serde_json::from_value(json!({
        "dataCapability": "live.showWindowOnPlayerView@1",
        "payload": payload,
        "caller": { "by": "dm", "from": "table.TableSheet", "roles": [] },
    }))
    .expect("a submission")
}

#[tokio::test]
async fn over_memory_no_live_session_opens_a_window_and_it_is_refused() {
    let engine = engine().await;
    for (payload, why) in [
        (
            json!({ "campaign": VALOMBRE, "media": PORTRAIT }),
            "aucune séance n'est en cours",
        ),
        (json!({ "media": PORTRAIT }), "campaign manque"),
        (json!({ "campaign": VALOMBRE }), "media manque"),
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
