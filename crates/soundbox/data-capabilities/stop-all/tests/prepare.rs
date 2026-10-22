//! What « Tout arrêter » turns a request into, before the engine queues it:
//! [`scope`] reads which playbacks it names — a campaign, from its payload
//! or its target, and one of its sessions only when it names one — and
//! refuses one naming no campaign, or an id that is no uuid; [`stops`]
//! stops each playback still playing at that instant, and leaves one that
//! stopped, or will, as it is; [`timestamp`] writes the instant as the
//! table takes it. Over the engine's memory, which holds no table, nothing
//! plays: the request is applied and writes nothing.

use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::store::Store;
use serde_json::{Value, json};
use stop_all::{
    DATA_CAPABILITY, Refusal, Row, STOPPED, Scope, Stop, plays, scope, stops, timestamp,
};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const FIRST: &str = "5e000000-0000-4000-8000-000000000001";

fn row(value: Value) -> Row {
    serde_json::from_value(value).expect("an object")
}

fn valombre(session: Option<&str>) -> Scope {
    Scope {
        campaign: VALOMBRE.to_owned(),
        session: session.map(str::to_owned),
    }
}

#[test]
fn a_stop_all_names_its_campaign_in_its_payload_or_as_its_target() {
    assert_eq!(
        scope(&row(json!({ "campaign": VALOMBRE })), None),
        Ok(valombre(None))
    );
    assert_eq!(scope(&Row::new(), Some(VALOMBRE)), Ok(valombre(None)));
    // The payload's campaign wins over the target, and either is read
    // trimmed and lowercase, as the tables write ids.
    assert_eq!(
        scope(
            &row(json!({ "campaign": format!(" {} ", VALOMBRE.to_uppercase()) })),
            Some("c1000000-0000-4000-8000-0000000000ff")
        ),
        Ok(valombre(None))
    );
    // A campaign left empty or null is no campaign: the target's is read.
    for empty in [json!(null), json!("  ")] {
        assert_eq!(
            scope(&row(json!({ "campaign": empty })), Some(VALOMBRE)),
            Ok(valombre(None)),
            "{empty}"
        );
    }
}

#[test]
fn a_session_named_narrows_the_stop_all_to_it() {
    assert_eq!(
        scope(
            &row(json!({ "campaign": VALOMBRE, "session": FIRST.to_uppercase() })),
            None
        ),
        Ok(valombre(Some(FIRST)))
    );
    for none in [json!(null), json!("")] {
        assert_eq!(
            scope(&row(json!({ "campaign": VALOMBRE, "session": none })), None),
            Ok(valombre(None)),
            "{none}"
        );
    }
}

#[test]
fn a_stop_all_naming_no_campaign_or_no_uuid_is_refused() {
    assert_eq!(scope(&Row::new(), None), Err(Refusal::NoCampaign));
    assert_eq!(scope(&Row::new(), Some("  ")), Err(Refusal::NoCampaign));
    for campaign in [
        json!("valombre"),
        json!(42),
        json!("c1000000-0000-4000-8000-00000000000"),
        json!("c1000000-0000-4000-8000-0000000000001"),
        json!("c1000000-0000-4000-8000-00000000000g"),
        json!("c1000000-0000-4000-8000-000000000001-0"),
        json!("c1000000000040008000000000000001"),
    ] {
        assert_eq!(
            scope(&row(json!({ "campaign": campaign })), None),
            Err(Refusal::NoCampaign),
            "{campaign}"
        );
    }
    for session in [json!("la-taverne"), json!(1)] {
        assert_eq!(
            scope(
                &row(json!({ "campaign": VALOMBRE, "session": session })),
                None
            ),
            Err(Refusal::NotSession),
            "{session}"
        );
    }
}

#[test]
fn a_refusal_says_why_and_is_unprocessable() {
    use harness_dataguard::plugins::{self, RefusalKind};
    assert_eq!(
        Refusal::NoCampaign.to_string(),
        "« Tout arrêter » nomme sa campagne"
    );
    assert_eq!(
        Refusal::NotSession.to_string(),
        "« Tout arrêter » ne reconnaît pas cette séance"
    );
    let refusal = plugins::Refusal::from(Refusal::NotSession);
    assert_eq!(refusal.kind, RefusalKind::Unprocessable);
    assert_eq!(
        refusal.message,
        "« Tout arrêter » ne reconnaît pas cette séance"
    );
}

#[test]
fn every_playback_still_playing_gets_the_instant_and_one_stopped_keeps_its_own() {
    const NOW: &str = "2026-10-22T08:00:00.123Z";
    let running = [
        ("P1".to_owned(), Row::new()),
        (
            "p2".to_owned(),
            row(json!({ STOPPED: null, "origin": "soundbox" })),
        ),
        (
            "p3".to_owned(),
            row(json!({ STOPPED: "2026-10-01T20:00:00.000Z" })),
        ),
    ];
    let stop = |id: &str| Stop {
        target_id: id.to_owned(),
        payload: row(json!({ STOPPED: NOW })),
    };

    assert_eq!(stops(&running, NOW), [stop("p1"), stop("p2")]);
    assert!(stops(&running[2..], NOW).is_empty(), "nothing plays");
    assert!(stops(&[], NOW).is_empty());
}

#[test]
fn a_playback_plays_until_it_has_a_stop_time() {
    assert!(plays(&Row::new()));
    assert!(plays(&row(json!({ STOPPED: null }))));
    assert!(!plays(&row(json!({ STOPPED: "2026-10-01T20:00:00.000Z" }))));
}

#[test]
fn the_instant_is_written_in_utc_to_the_millisecond() {
    assert_eq!(timestamp(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(timestamp(1_792_656_000_123), "2026-10-22T08:00:00.123Z");
    assert_eq!(timestamp(951_782_400_000), "2000-02-29T00:00:00.000Z");
    assert_eq!(timestamp(4_107_542_399_999), "2100-02-28T23:59:59.999Z");
    // The day after: a century year, no leap day, March 1st.
    assert_eq!(timestamp(4_107_542_400_000), "2100-03-01T00:00:00.000Z");
    assert_eq!(timestamp(1_704_067_199_001), "2023-12-31T23:59:59.001Z");
}

#[tokio::test]
async fn over_the_engine_s_memory_nothing_plays_and_a_stop_all_writes_nothing() {
    let store = Arc::new(MemoryStore::default());
    let aggregate = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../aggregates/SoundPlayback/aggregate.json"),
    )
    .expect("the aggregate's manifest");
    let plugins = [stop_all::plugin(), aggregate_sound_playback::plugin()]
        .into_iter()
        .collect();
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at_millis(1_792_656_000_123)),
    )
    .with_aggregate(Aggregate::parse(&aggregate).expect("contract I"))
    .with_data_capability(
        DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
    )
    .with_plugins(plugins);
    engine.load().await.expect("the engine loaded");

    let submitted = engine
        .submit(Submission {
            data_capability: DATA_CAPABILITY.to_owned(),
            payload: row(json!({ "campaign": VALOMBRE })),
            ..Submission::default()
        })
        .await
        .expect("queued");
    let results = engine.run_once().await.expect("the applier ran");

    assert_eq!(submitted.partition, format!("SoundPlayback/{VALOMBRE}"));
    assert!(submitted.then.is_empty());
    assert_eq!(results.len(), 1, "{results:?}");
    assert_eq!(results[0].status, State::Applied, "{results:?}");
    let written = store
        .row(&format!("SoundPlayback/{VALOMBRE}"))
        .await
        .expect("read");
    assert!(written.is_empty(), "{written:?}");
}
