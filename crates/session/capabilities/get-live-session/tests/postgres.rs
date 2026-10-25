//! session.getLiveSession over PostgreSQL (`DATABASE_URL`): the real SQL of
//! the Capability, run by the test kit's [`Scratch`] on a database migrated as
//! the server migrates it, and answered by `answer`. Two campaigns, each with
//! a session: the session read by its id, and the live session read without
//! one, each answer the campaign it belongs to. Each test works in a database
//! of its own, dropped when it ends — even on a panic.

use std::collections::BTreeMap;

use get_live_session::{SQL, answer};
use platform_test_kit::Scratch;
use serde_json::{Value, json};

/// Les Cendres de Valombre.
const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Les Brumes, a second campaign.
const BRUMES: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's session 4, live since 20:02 UTC.
const EMBUSCADE: &str = "51000000-0000-4000-8000-000000000001";
/// Brumes' session 1, still in preparation.
const PASSEUR: &str = "51000000-0000-4000-8000-000000000002";
/// Elowen, a PC of Valombre, ticked present in session 4.
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";

/// A database of its own: Valombre with its live session 4 and Elowen
/// present, Brumes with its session 1 in preparation.
async fn seeded(label: &str) -> Scratch {
    let db = Scratch::new(&format!("get_live_session_{label}")).await;
    db.seed(&format!(
        "INSERT INTO campaign (id, name, kind) VALUES \
         ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'), \
         ('{BRUMES}', 'Les Brumes', 'oneshot'); \
         INSERT INTO entity (id, campaign_id, type, name) VALUES \
         ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen Ventargent'); \
         INSERT INTO character (entity_id, token_color) VALUES ('{ELOWEN}', '#888'); \
         INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
         VALUES ('{ELOWEN}', 1, 'dm', '{{}}', '[]'); \
         INSERT INTO session (id, campaign_id, position, title, planned_on, status, started_at, microphone_on) VALUES \
         ('{EMBUSCADE}', '{VALOMBRE}', 4, 'L’embuscade du Gué', '2026-10-17', 'live', \
          timestamptz '2026-10-17 20:02:00+00', true), \
         ('{PASSEUR}', '{BRUMES}', 1, 'Le Passeur', NULL, 'prep', NULL, false); \
         INSERT INTO session_present_character (session_id, character_id, added_at) \
         VALUES ('{EMBUSCADE}', '{ELOWEN}', timestamptz '2026-10-17 20:03:00+00')"
    ))
    .await;
    db
}

/// The answer to a read with `id` (`None`: no id, the running session), as
/// the registry serves it: the rows of [`SQL`] bound to it, then `answer`.
async fn read(db: &Scratch, id: Option<&str>) -> Value {
    let id = id.map_or(Value::Null, |id| json!(id));
    let rows = db.rows(SQL, std::slice::from_ref(&id)).await;
    let variables = BTreeMap::from([("id".to_string(), id)]);
    answer(&Value::Array(rows), &variables)
}

#[tokio::test]
async fn each_session_read_by_its_id_answers_its_own_campaign() {
    let db = seeded("by_id").await;
    assert_eq!(
        read(&db, Some(EMBUSCADE)).await["session"]["campaign"],
        VALOMBRE
    );
    assert_eq!(
        read(&db, Some(PASSEUR)).await["session"]["campaign"],
        BRUMES
    );
}

#[tokio::test]
async fn the_live_session_read_without_an_id_answers_its_own_campaign() {
    let db = seeded("live").await;
    let got = read(&db, None).await;
    assert_eq!(got["session"]["id"], EMBUSCADE);
    assert_eq!(got["session"]["campaign"], VALOMBRE);
    // Brumes' session goes live after it: the one that started last is read,
    // with its own campaign.
    db.seed(&format!(
        "UPDATE session SET status = 'live', started_at = timestamptz '2026-10-17 21:00:00+00' \
         WHERE id = '{PASSEUR}'"
    ))
    .await;
    let got = read(&db, None).await;
    assert_eq!(got["session"]["id"], PASSEUR);
    assert_eq!(got["session"]["campaign"], BRUMES);
}

#[tokio::test]
async fn the_whole_answer_keeps_every_v1_field_beside_the_campaign() {
    let db = seeded("whole").await;
    assert_eq!(
        read(&db, Some(EMBUSCADE)).await,
        json!({
            "session": {
                "id": EMBUSCADE,
                "campaign": VALOMBRE,
                "order": 4,
                "title": "L’embuscade du Gué",
                "date": "2026-10-17",
                "startedAt": "20:02",
                "status": "live",
                "statusLabel": "En direct",
                "present": [ELOWEN],
            },
            "microphone": { "on": true },
        })
    );
}

#[tokio::test]
async fn no_matching_session_and_none_live_is_still_null() {
    let db = seeded("none").await;
    db.seed(&format!(
        "UPDATE session SET status = 'closed' WHERE id = '{EMBUSCADE}'"
    ))
    .await;
    assert_eq!(read(&db, None).await, Value::Null);
    assert_eq!(
        read(&db, Some("51000000-0000-4000-8000-0000000000ff")).await,
        Value::Null
    );
    assert_eq!(read(&db, Some("pas-un-uuid")).await, Value::Null);
}
