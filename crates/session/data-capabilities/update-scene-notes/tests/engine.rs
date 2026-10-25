//! `session.updateSceneNotes` as the server plants it beside Session, over
//! the in-memory store (`platform_test_kit::Bench`): the DM's notes land on
//! a scene of the session and the scene keeps them; a scene of another
//! session, one the store does not hold, or notes that are no text are
//! refused before anything is queued; the same command replayed with its
//! key is not applied twice. What it writes passes `text-only-no-audio` —
//! text, and links that are ids — which the engine judges on every write.

use harness_dataguard::Result;
use harness_dataguard::api::Submission;
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::plugins::RefusalKind;
use platform_test_kit::{Bench, Served, file, planter, refused, uuid};
use serde_json::{Value, json};

const PLANT: &str = "test.plantScene";

fn session() -> String {
    uuid(0xb0, 1)
}
fn other_session() -> String {
    uuid(0xb0, 2)
}
fn scene() -> String {
    uuid(0xc0, 1)
}
fn place() -> String {
    uuid(0xe0, 1)
}
fn event() -> String {
    uuid(0xe0, 2)
}

/// The engine as the server plants this unit and its aggregate.
async fn served() -> Served {
    Bench::new(
        include_str!("../data-capability.json"),
        &file("crates/session/aggregates/Session/aggregate.json"),
        [aggregate_session::plugin(), update_scene_notes::plugin()],
    )
    .planting(&planter(PLANT, "Session", "$sceneId", "insert"))
    .memory()
    .await
}

/// A scene of `session`, as the engine holds it once it is added.
fn planted(id: &str, session: &str, links: &Value) -> Value {
    json!({
        "sceneId": id,
        "sceneSessionId": session,
        "sceneTitle": "La taverne",
        "notes": "avant",
        "entityId": links,
    })
}

/// The command the DM sends.
fn command(session: &str, scene: &str, notes: Value) -> Value {
    json!({ "session": session, "scene": scene, "notes": notes })
}

/// The command, submitted with `key`.
async fn submit_keyed(
    served: &Served,
    payload: Value,
    key: &str,
) -> Result<harness_dataguard::api::Submitted> {
    let Value::Object(fields) = payload else {
        panic!("an object")
    };
    served
        .engine
        .submit(Submission {
            data_capability: "session.updateSceneNotes".into(),
            payload: fields.into_iter().collect(),
            idempotency_key: Some(key.into()),
            caller: Engine::caller("dm", "test", &[], &["role:approver"]),
            ..Submission::default()
        })
        .await
}

#[tokio::test]
async fn the_unit_is_planted_beside_its_aggregate() {
    let served = served().await;
    assert_eq!(served.settled().await.len(), 0, "nothing queued yet");
}

#[tokio::test]
async fn the_dm_writes_a_scene_s_notes_and_the_scene_keeps_them() {
    let served = served().await;
    let links = json!([place(), event()]);
    served
        .plant(PLANT, [planted(&scene(), &session(), &links)])
        .await;
    let result = served
        .applied(
            "session.updateSceneNotes",
            command(
                &session(),
                &scene(),
                json!("Le gué est tenu par des bandits."),
            ),
            None,
        )
        .await;
    assert_eq!(result.status, State::Applied);
    let row = served.row(&format!("Session/{}", scene()));
    assert_eq!(row["notes"], json!("Le gué est tenu par des bandits."));
    assert_eq!(row["sceneTitle"], json!("La taverne"), "the title is kept");
    assert_eq!(row["sceneSessionId"], json!(session()));
    assert_eq!(row["entityId"], links, "the links are kept");
}

#[tokio::test]
async fn a_blank_text_is_written_as_a_blank_text() {
    let served = served().await;
    served
        .plant(PLANT, [planted(&scene(), &session(), &json!([]))])
        .await;
    served
        .applied(
            "session.updateSceneNotes",
            command(&session(), &scene(), json!("")),
            None,
        )
        .await;
    assert_eq!(
        served.row(&format!("Session/{}", scene()))["notes"],
        json!("")
    );
}

#[tokio::test]
async fn a_scene_of_another_session_is_refused_422_and_no_notes_change() {
    let served = served().await;
    served
        .plant(PLANT, [planted(&scene(), &other_session(), &json!([]))])
        .await;
    let answer = served
        .submit(
            "session.updateSceneNotes",
            command(&session(), &scene(), json!("intrus")),
            None,
        )
        .await;
    let why = refused(answer, RefusalKind::Unprocessable);
    assert!(why.contains("pas de la séance"), "{why}");
    assert_eq!(served.settled().await.len(), 0, "nothing was queued");
    assert_eq!(
        served.row(&format!("Session/{}", scene()))["notes"],
        json!("avant")
    );
}

#[tokio::test]
async fn a_scene_the_store_does_not_hold_is_a_404() {
    let served = served().await;
    let answer = served
        .submit(
            "session.updateSceneNotes",
            command(&session(), &scene(), json!("x")),
            None,
        )
        .await;
    assert!(!refused(answer, RefusalKind::NotFound).is_empty());
    assert_eq!(served.settled().await.len(), 0);
}

#[tokio::test]
async fn the_row_of_a_session_is_no_scene() {
    let served = served().await;
    served
        .plant(
            PLANT,
            [json!({ "sceneId": scene(), "sceneSessionId": session(), "sceneTitle": "T" })],
        )
        .await;
    let answer = served
        .submit(
            "session.updateSceneNotes",
            command(&session(), &session(), json!("x")),
            None,
        )
        .await;
    assert!(!refused(answer, RefusalKind::NotFound).is_empty());
}

#[tokio::test]
async fn notes_that_are_no_text_are_refused_422() {
    let served = served().await;
    served
        .plant(PLANT, [planted(&scene(), &session(), &json!([]))])
        .await;
    for notes in [json!(null), json!(3), json!({ "audio": "…" })] {
        let answer = served
            .submit(
                "session.updateSceneNotes",
                command(&session(), &scene(), notes),
                None,
            )
            .await;
        assert!(!refused(answer, RefusalKind::Unprocessable).is_empty());
    }
    assert_eq!(served.settled().await.len(), 0);
}

#[tokio::test]
async fn the_same_command_replayed_with_its_key_is_not_applied_twice() {
    let served = served().await;
    served
        .plant(PLANT, [planted(&scene(), &session(), &json!([]))])
        .await;
    let notes = command(&session(), &scene(), json!("une fois"));
    let first = submit_keyed(&served, notes.clone(), "k1")
        .await
        .expect("queued");
    assert!(!first.replayed);
    let again = submit_keyed(&served, notes, "k1").await.expect("answered");
    assert!(again.replayed, "the key is known");
    assert_eq!(again.command_id, first.command_id);
    let settled = served.settled().await;
    assert_eq!(settled.len(), 1, "applied once: {settled:?}");
    assert_eq!(settled[0].status, State::Applied);
}
