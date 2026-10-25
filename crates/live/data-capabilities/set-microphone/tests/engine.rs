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
