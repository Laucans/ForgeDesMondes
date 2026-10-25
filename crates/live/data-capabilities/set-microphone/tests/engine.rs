//! `live.setMicrophone` as the server plants it beside Session, over the in-memory
//! store (`platform_test_kit::Bench`): what the engine refuses before it reads a single
//! table — an `on` that is not a boolean, a target that is no uuid, a server with no
//! database to find the session in — and that nothing is queued then. The tables it
//! writes are `tests/postgres.rs`'s.

use std::sync::Arc;

use harness_dataguard::plugins::{RefusalKind, Shared};
use platform_test_kit::{Bench, Served, file, refused};
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::json;

use set_microphone::Listening;

/// The engine as the server plants this unit and its aggregate, the speech-to-text
/// port and the streams shared with it.
async fn served() -> Served {
    let mut shared = Shared::default();
    shared.insert(Arc::new(SharedSpeechToText::new(FakeSpeechToText::new(""))));
    shared.insert(Arc::new(Listening::new()));
    Bench::new(
        include_str!("../data-capability.json"),
        &file("crates/session/aggregates/Session/aggregate.json"),
        [aggregate_session::plugin(), set_microphone::plugin()],
    )
    .sharing(shared)
    .memory()
    .await
}

#[tokio::test]
async fn the_unit_is_planted_beside_its_aggregate() {
    let served = served().await;
    assert_eq!(served.settled().await.len(), 0, "nothing queued yet");
}

#[tokio::test]
async fn an_on_that_is_not_a_boolean_is_refused_and_nothing_is_queued() {
    let served = served().await;
    for payload in [json!({}), json!({ "on": "true" }), json!({ "on": 1 })] {
        let message = refused(
            served.submit("live.setMicrophone", payload, None).await,
            RefusalKind::Unprocessable,
        );
        assert!(message.contains("on:"), "{message}");
    }
    assert!(served.settled().await.is_empty(), "nothing queued");
}

#[tokio::test]
async fn a_target_that_is_no_uuid_is_refused_and_nothing_is_queued() {
    let served = served().await;
    let message = refused(
        served
            .submit("live.setMicrophone", json!({ "on": true }), Some("s4"))
            .await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("target"), "{message}");
    assert!(served.settled().await.is_empty(), "nothing queued");
}

#[tokio::test]
async fn a_server_with_no_database_cannot_find_the_session_and_queues_nothing() {
    let served = served().await;
    let message = refused(
        served
            .submit("live.setMicrophone", json!({ "on": true }), None)
            .await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("no database"), "{message}");
    assert!(served.settled().await.is_empty(), "nothing queued");
}

#[tokio::test]
async fn the_suggestions_a_stream_hears_are_queued_in_the_engine_it_is_bound_to() {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use harness_dataguard::guard::Caller;
    use harness_dataguard::plugins::Project;
    use harness_dataguard::store::Write;
    use platform_test_kit::planter;
    use set_microphone::detect::Vocabulary;
    use set_microphone::listening::{Observer, Plan, Plans};
    use tokio::sync::Notify;

    const SESSION: &str = "5e000000-0000-4000-8000-000000000001";
    let speech = FakeSpeechToText::new("").listening(["La Balafrée"], Duration::from_millis(5));
    let listening = Arc::new(Listening::new());
    let mut shared = Shared::default();
    shared.insert(Arc::new(SharedSpeechToText::new(speech.clone())));
    shared.insert(Arc::clone(&listening));
    // A test-only stand-in for live.addSuggestion, writing a row the engine holds.
    let served = Bench::new(
        include_str!("../data-capability.json"),
        &file("crates/session/aggregates/Session/aggregate.json"),
        [aggregate_session::plugin(), set_microphone::plugin()],
    )
    .planting(&planter(
        "live.addSuggestion",
        "Session",
        "$sessionId",
        "insert",
    ))
    .sharing(shared)
    .memory()
    .await;
    let engine = Arc::new(served.engine);
    let wake = Arc::new(Notify::new());
    listening.bind(&engine, Arc::clone(&wake));

    let plans = Arc::new(Plans::default());
    plans.stage(
        SESSION,
        &listening,
        Some(Plan {
            vocabulary: Vocabulary::from_rows(&json!([
                { "term": "la Balafrée", "kind": "entity", "id": "e1000000-0000-4000-8000-000000000001", "alias": true },
            ])),
            speech: SharedSpeechToText::new(speech),
            caller: Caller {
                by: "dm".to_string(),
                roles: vec!["role:approver".to_string()],
                ..Caller::default()
            },
        }),
    );
    Observer(plans).observe(&Write {
        partition: format!("Session/{SESSION}"),
        values: BTreeMap::from([("microphoneOn".to_string(), json!(true))]),
        tombstone: false,
    });
    // The suggestion is queued in the engine, and the applier woken.
    let woken = tokio::time::timeout(Duration::from_secs(2), wake.notified()).await;
    assert!(
        woken.is_ok(),
        "the applier is woken once a suggestion is queued"
    );
    let queued = engine.run_once().await.expect("the applier");
    assert_eq!(queued.len(), 1, "one suggestion queued: {queued:?}");
    listening.shutdown();
}
