//! `session.updateSceneNotes` over PostgreSQL (`DATABASE_URL`), as the server
//! plants it beside Session, whose projection writes its tables: each test
//! works in a scratch database of its own, migrated as the server migrates
//! it, dropped when it ends (`platform_test_kit::Scratch`). A scene the
//! server loaded into `scene` — which the engine holds no row of — takes the
//! DM's notes in its table and keeps its rank, title and links; a scene of
//! another session changes nothing; one the table does not hold is a 404.

use harness_dataguard::command::State;
use harness_dataguard::plugins::RefusalKind;
use platform_test_kit::{Bench, Scratch, Served, file, planter, refused, uuid};
use serde_json::{Value, json};

const PLANT: &str = "test.plantScene";

fn campaign() -> String {
    uuid(0xca, 1)
}
fn session() -> String {
    uuid(0xb0, 1)
}
fn other_session() -> String {
    uuid(0xb0, 2)
}
fn scene() -> String {
    uuid(0xc0, 1)
}
fn second_scene() -> String {
    uuid(0xc0, 2)
}
fn foreign_scene() -> String {
    uuid(0xc0, 3)
}
fn place() -> String {
    uuid(0xe0, 1)
}
fn event() -> String {
    uuid(0xe0, 2)
}

/// The engine as the server plants this unit and its aggregate over
/// `scratch`'s tables.
async fn served(scratch: &Scratch) -> Served {
    Bench::new(
        include_str!("../data-capability.json"),
        &file("crates/session/aggregates/Session/aggregate.json"),
        [aggregate_session::plugin(), update_scene_notes::plugin()],
    )
    .planting(&planter(PLANT, "Session", "$sceneId", "insert"))
    .postgres(scratch)
    .await
}

/// A campaign, two sessions, two elements, and the scenes the server's
/// fixtures load straight into the tables: two of the first session — the
/// first links the place then the event, in that order — one of the second.
async fn seeded(label: &str) -> Scratch {
    let scratch = Scratch::new(label).await;
    scratch
        .seed(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES ('{c}', 'Valombre', 'campagne');
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                ('{s1}', '{c}', 1, 'Séance 1', 'prep', false),
                ('{s2}', '{c}', 2, 'Séance 2', 'prep', false);
             INSERT INTO entity (id, campaign_id, type, name) VALUES
                ('{p}', '{c}', 'lieu', 'La taverne'), ('{e}', '{c}', 'evenement', 'Le marché');
             INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) VALUES
                ('{p}', 1, 'dm', '{{}}', '[]'), ('{e}', 1, 'dm', '{{}}', '[]');
             INSERT INTO scene (id, session_id, position, title, notes) VALUES
                ('{a}', '{s1}', 1, 'La taverne', 'avant'),
                ('{b}', '{s1}', 2, 'Le gué', NULL),
                ('{x}', '{s2}', 1, 'Ailleurs', 'à eux');
             INSERT INTO scene_entity (scene_id, entity_id, position) VALUES
                ('{a}', '{e}', 2), ('{a}', '{p}', 1);",
            c = campaign(),
            s1 = session(),
            s2 = other_session(),
            p = place(),
            e = event(),
            a = scene(),
            b = second_scene(),
            x = foreign_scene(),
        ))
        .await;
    scratch
}

/// The command the DM sends.
fn command(session: &str, scene: &str, notes: Value) -> Value {
    json!({ "session": session, "scene": scene, "notes": notes })
}

/// The scenes as the table holds them: id, rank, title, notes, by rank of
/// session then scene.
async fn scenes(scratch: &Scratch) -> Vec<Value> {
    scratch
        .rows(
            "SELECT id::text AS id, session_id::text AS session, position, title, notes
             FROM scene ORDER BY session_id, position",
            &[],
        )
        .await
}

/// The ids a scene links, in rank.
async fn links(scratch: &Scratch, scene: &str) -> Vec<Value> {
    scratch
        .rows(
            "SELECT entity_id::text AS id FROM scene_entity
             WHERE scene_id = $1::text::uuid ORDER BY position",
            &[json!(scene)],
        )
        .await
        .into_iter()
        .map(|row| row["id"].clone())
        .collect()
}

#[tokio::test]
async fn a_scene_loaded_into_the_table_takes_the_notes_and_keeps_the_rest() {
    let scratch = seeded("update_scene_notes_loaded").await;
    let served = served(&scratch).await;
    let result = served
        .applied(
            "session.updateSceneNotes",
            command(&session(), &scene(), json!("Un indice dans la cave.")),
            None,
        )
        .await;
    assert_eq!(result.status, State::Applied);
    let held = scenes(&scratch).await;
    let written = held
        .iter()
        .find(|s| s["id"] == json!(scene()))
        .expect("kept");
    assert_eq!(written["notes"], json!("Un indice dans la cave."));
    assert_eq!(written["title"], json!("La taverne"), "its title is kept");
    assert_eq!(written["position"], json!(1), "its rank is kept");
    assert_eq!(held.len(), 3, "no scene added or removed");
    let others: Vec<_> = held.iter().filter(|s| s["id"] != json!(scene())).collect();
    assert_eq!(
        others[0]["notes"],
        Value::Null,
        "the other scenes are untouched"
    );
    assert_eq!(others[1]["notes"], json!("à eux"));
    assert_eq!(
        links(&scratch, &scene()).await,
        [json!(place()), json!(event())],
        "its links are kept, in their order"
    );
}

#[tokio::test]
async fn a_scene_with_no_notes_takes_them_and_the_list_answers_them() {
    let scratch = seeded("update_scene_notes_listed").await;
    let served = served(&scratch).await;
    served
        .applied(
            "session.updateSceneNotes",
            command(&session(), &second_scene(), json!("")),
            None,
        )
        .await;
    let held = scenes(&scratch).await;
    let written = held
        .iter()
        .find(|s| s["id"] == json!(second_scene()))
        .expect("kept");
    assert_eq!(written["notes"], json!(""), "blank is not null");
    assert!(links(&scratch, &second_scene()).await.is_empty());
}

#[tokio::test]
async fn a_scene_the_engine_holds_takes_the_notes_in_its_table() {
    let scratch = seeded("update_scene_notes_held").await;
    let served = served(&scratch).await;
    let added = uuid(0xc0, 9);
    served
        .plant(
            PLANT,
            [json!({
                "sceneId": added,
                "sceneSessionId": session(),
                "sceneTitle": "Ajoutée",
                "entityId": [place()],
            })],
        )
        .await;
    served
        .applied(
            "session.updateSceneNotes",
            command(&session(), &added, json!("écrites")),
            None,
        )
        .await;
    let held = scenes(&scratch).await;
    let written = held.iter().find(|s| s["id"] == json!(added)).expect("kept");
    assert_eq!(written["notes"], json!("écrites"));
    assert_eq!(written["title"], json!("Ajoutée"));
    assert_eq!(written["position"], json!(3), "after the session's last");
    assert_eq!(links(&scratch, &added).await, [json!(place())]);
}

#[tokio::test]
async fn a_scene_of_another_session_is_refused_422_and_no_notes_change() {
    let scratch = seeded("update_scene_notes_foreign").await;
    let served = served(&scratch).await;
    let before = scenes(&scratch).await;
    let answer = served
        .submit(
            "session.updateSceneNotes",
            command(&session(), &foreign_scene(), json!("intrus")),
            None,
        )
        .await;
    let why = refused(answer, RefusalKind::Unprocessable);
    assert!(why.contains("pas de la séance"), "{why}");
    assert_eq!(served.settled().await.len(), 0, "nothing was queued");
    assert_eq!(scenes(&scratch).await, before);
}

#[tokio::test]
async fn a_scene_the_table_does_not_hold_is_a_404() {
    let scratch = seeded("update_scene_notes_missing").await;
    let served = served(&scratch).await;
    let answer = served
        .submit(
            "session.updateSceneNotes",
            command(&session(), &uuid(0xc0, 8), json!("x")),
            None,
        )
        .await;
    assert!(!refused(answer, RefusalKind::NotFound).is_empty());
    assert_eq!(served.settled().await.len(), 0);
}
