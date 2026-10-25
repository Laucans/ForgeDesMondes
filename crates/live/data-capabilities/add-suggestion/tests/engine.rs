//! `live.addSuggestion` as the server plants it beside Suggestion, over the
//! tables of a scratch PostgreSQL (`platform_test_kit::Bench`): the query
//! that reads the session and its campaign's targets runs for real. A
//! recognised element or sound becomes a Suggestion at the engine's clock,
//! only for a live session with the microphone on (live-only-local-audio) and
//! only for a target of the session's campaign (suggestion-target); the row
//! holds the suggestion and nothing else (pile-and-history-derived); every
//! refusal writes nothing.

use std::sync::Arc;

use harness_dataguard::command::State;
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::store::Ids;
use platform_test_kit::{Bench, Scratch, Served, file, manifest, refused, uuid};
use serde_json::{Value, json};

const MANIFEST: &str = include_str!("../data-capability.json");

/// The campaign the sessions belong to, and another one.
fn campaign() -> String {
    uuid(0xca, 1)
}
fn elsewhere() -> String {
    uuid(0xca, 2)
}

/// A session of the campaign: live with the microphone on, in prep, closed,
/// live with the microphone off; and one the tables do not have.
fn live() -> String {
    uuid(0x5e, 1)
}
fn prep() -> String {
    uuid(0x5e, 2)
}
fn closed() -> String {
    uuid(0x5e, 3)
}
fn mute() -> String {
    uuid(0x5e, 4)
}
fn unknown() -> String {
    uuid(0x5e, 9)
}

/// An element and a sound of the campaign, and of the other one.
fn entity() -> String {
    uuid(0xe1, 1)
}
fn foreign_entity() -> String {
    uuid(0xe1, 2)
}
fn sound() -> String {
    uuid(0x50, 1)
}
fn foreign_sound() -> String {
    uuid(0x50, 2)
}

/// The id the engine gives the next row.
const SUGGESTION: &str = "5a000000-0000-4000-8000-000000000001";

struct One;

impl Ids for One {
    fn fresh(&self) -> String {
        SUGGESTION.to_string()
    }
}

/// The tables the foreign keys ask for, over a scratch database.
async fn seeded(label: &str) -> (Scratch, Served) {
    let scratch = Scratch::new(label).await;
    scratch
        .seed(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES
                 ('{campaign}', 'Les Brumes', 'campagne'),
                 ('{elsewhere}', 'Ailleurs', 'campagne');
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                 ('{live}', '{campaign}', 1, 'Live', 'live', true),
                 ('{prep}', '{campaign}', 2, 'Prep', 'prep', false),
                 ('{closed}', '{campaign}', 3, 'Closed', 'closed', false),
                 ('{mute}', '{campaign}', 4, 'Mute', 'live', false);
             INSERT INTO sound (id, campaign_id, name, file_path) VALUES
                 ('{sound}', '{campaign}', 'Porte qui grince', '/sons/porte.ogg'),
                 ('{foreign_sound}', '{elsewhere}', 'Tonnerre', '/sons/tonnerre.ogg');",
            campaign = campaign(),
            elsewhere = elsewhere(),
            live = live(),
            prep = prep(),
            closed = closed(),
            mute = mute(),
            sound = sound(),
            foreign_sound = foreign_sound(),
        ))
        .await;
    // An element's versions are written in the transaction that saves it
    // (every-change-versioned).
    scratch
        .seed(&format!(
            "BEGIN;
             INSERT INTO entity (id, campaign_id, type, name) VALUES
                 ('{entity}', '{campaign}', 'lieu', 'La Taverne du Sanglier'),
                 ('{foreign_entity}', '{elsewhere}', 'pnj', 'Morlox');
             INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) VALUES
                 ('{entity}', 1, 'dm', '{{}}', '[]'),
                 ('{foreign_entity}', 1, 'dm', '{{}}', '[]');
             COMMIT;",
            entity = entity(),
            foreign_entity = foreign_entity(),
            campaign = campaign(),
            elsewhere = elsewhere(),
        ))
        .await;
    let served = Bench::new(
        MANIFEST,
        &file("crates/live/aggregates/Suggestion/aggregate.json"),
        [aggregate_suggestion::plugin(), add_suggestion::plugin()],
    )
    .ids(Arc::new(One))
    .postgres(&scratch)
    .await;
    (scratch, served)
}

/// The rows the engine holds, by partition.
async fn written(scratch: &Scratch) -> Vec<Value> {
    scratch
        .rows(
            "SELECT partition, fields FROM dataguard_rows ORDER BY partition",
            &[],
        )
        .await
}

/// Asserts that `answer` is a 422 and that nothing was queued or written.
async fn refused_422(scratch: &Scratch, served: &Served, answer: Value) -> String {
    let message = refused(
        served.submit("live.addSuggestion", answer, None).await,
        RefusalKind::Unprocessable,
    );
    assert!(served.settled().await.is_empty(), "nothing queued");
    assert!(written(scratch).await.is_empty(), "nothing written");
    message
}

#[test]
fn the_manifest_is_an_insert_touching_nothing() {
    let manifest = manifest(MANIFEST);
    assert_eq!(manifest.data_capability, "live.addSuggestion");
    assert_eq!(manifest.target.aggregate, "Suggestion");
    assert_eq!(format!("{:?}", manifest.effect), "Insert");
    assert!(manifest.touches.is_empty());
    for id in [
        "suggestion-target",
        "pile-and-history-derived",
        "live-only-local-audio",
    ] {
        assert!(manifest.invariants.iter().any(|i| i == id), "{id}");
    }
}

#[tokio::test]
async fn a_recognised_element_becomes_a_suggestion_at_the_engine_clock() {
    let (scratch, served) = seeded("add_suggestion_entity").await;
    let applied = served
        .applied(
            "live.addSuggestion",
            json!({ "sessionId": live(), "entityId": entity(), "matchedText": "la taverne" }),
            None,
        )
        .await;
    assert_eq!(applied.status, State::Applied);
    assert!(applied.violations.is_empty(), "{:?}", applied.violations);
    assert_eq!(
        written(&scratch).await,
        [json!({
            "partition": format!("Suggestion/{SUGGESTION}"),
            "fields": {
                "id": SUGGESTION,
                "sessionId": live(),
                "entityId": entity(),
                "matchedText": "la taverne",
                // The bench's clock stands at 1_792_000_000 s.
                "detectedAt": "2026-10-14T17:46:40.000Z",
            },
        })]
    );
}

#[tokio::test]
async fn a_recognised_sound_becomes_a_suggestion() {
    let (scratch, served) = seeded("add_suggestion_sound").await;
    served.clock.set_millis(1_792_000_123_456);
    served
        .applied(
            "live.addSuggestion",
            json!({ "sessionId": live(), "soundId": sound(), "matchedText": "la porte grince" }),
            None,
        )
        .await;
    assert_eq!(
        written(&scratch).await,
        [json!({
            "partition": format!("Suggestion/{SUGGESTION}"),
            "fields": {
                "id": SUGGESTION,
                "sessionId": live(),
                "soundId": sound(),
                "matchedText": "la porte grince",
                "detectedAt": "2026-10-14T17:48:43.456Z",
            },
        })]
    );
}

#[tokio::test]
async fn live_only_local_audio_a_session_in_prep_closed_or_muted_refuses() {
    let (scratch, served) = seeded("add_suggestion_not_live").await;
    for session in [prep(), closed(), mute()] {
        refused_422(
            &scratch,
            &served,
            json!({ "sessionId": session, "entityId": entity(), "matchedText": "la taverne" }),
        )
        .await;
    }
    // The words say which: not live, or live but mute.
    let in_prep = refused_422(
        &scratch,
        &served,
        json!({ "sessionId": prep(), "soundId": sound(), "matchedText": "porte" }),
    )
    .await;
    let muted = refused_422(
        &scratch,
        &served,
        json!({ "sessionId": mute(), "soundId": sound(), "matchedText": "porte" }),
    )
    .await;
    assert!(in_prep.contains("not live"), "{in_prep}");
    assert!(muted.contains("microphone is off"), "{muted}");
}

#[tokio::test]
async fn suggestion_target_an_element_or_a_sound_of_another_campaign_refuses() {
    let (scratch, served) = seeded("add_suggestion_foreign").await;
    for target in [
        json!({ "entityId": foreign_entity() }),
        json!({ "soundId": foreign_sound() }),
        // Rows that exist nowhere.
        json!({ "entityId": uuid(0xe1, 9) }),
        json!({ "soundId": uuid(0x50, 9) }),
    ] {
        let mut payload = json!({ "sessionId": live(), "matchedText": "Morlox" });
        payload
            .as_object_mut()
            .expect("an object")
            .extend(target.as_object().expect("an object").clone());
        let message = refused_422(&scratch, &served, payload).await;
        assert!(
            message.contains("not one of the session's campaign"),
            "{message}"
        );
    }
}

#[tokio::test]
async fn an_element_where_the_sound_is_asked_for_is_not_a_sound() {
    // The element and the sound are looked up in their own tables.
    let (scratch, served) = seeded("add_suggestion_crossed").await;
    refused_422(
        &scratch,
        &served,
        json!({ "sessionId": live(), "soundId": entity(), "matchedText": "la taverne" }),
    )
    .await;
    refused_422(
        &scratch,
        &served,
        json!({ "sessionId": live(), "entityId": sound(), "matchedText": "la porte" }),
    )
    .await;
}

#[tokio::test]
async fn both_neither_or_a_blank_text_refuse() {
    let (scratch, served) = seeded("add_suggestion_shape").await;
    for payload in [
        json!({ "sessionId": live(), "entityId": entity(), "soundId": sound(), "matchedText": "x" }),
        json!({ "sessionId": live(), "matchedText": "x" }),
        json!({ "sessionId": live(), "entityId": entity(), "matchedText": "" }),
        json!({ "sessionId": live(), "entityId": entity(), "matchedText": "  " }),
        json!({ "sessionId": live(), "entityId": entity() }),
        json!({ "sessionId": "x", "entityId": entity(), "matchedText": "x" }),
    ] {
        refused_422(&scratch, &served, payload).await;
    }
}

#[tokio::test]
async fn an_unknown_session_is_refused_404_and_nothing_is_written() {
    let (scratch, served) = seeded("add_suggestion_unknown").await;
    let message = refused(
        served
            .submit(
                "live.addSuggestion",
                json!({ "sessionId": unknown(), "entityId": entity(), "matchedText": "la taverne" }),
                None,
            )
            .await,
        RefusalKind::NotFound,
    );
    assert!(message.contains("session"), "{message}");
    assert!(served.settled().await.is_empty(), "nothing queued");
    assert!(written(&scratch).await.is_empty(), "nothing written");
}

#[tokio::test]
async fn a_suggestion_is_always_new() {
    // Naming a row would merge into an existing suggestion.
    let (scratch, served) = seeded("add_suggestion_named").await;
    let payload = json!({ "sessionId": live(), "entityId": entity(), "matchedText": "la taverne" });
    served
        .applied("live.addSuggestion", payload.clone(), None)
        .await;
    let message = refused(
        served
            .submit("live.addSuggestion", payload, Some(SUGGESTION))
            .await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("always new"), "{message}");
    assert!(served.settled().await.is_empty(), "nothing queued");
    assert_eq!(written(&scratch).await.len(), 1);
}
