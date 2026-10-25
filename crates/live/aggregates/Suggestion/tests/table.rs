//! The Suggestion aggregate's table over PostgreSQL (`DATABASE_URL`), as the
//! server plants it: every applied write of a suggestion is a `live_suggestion`
//! row, inserted once in the transaction that applies it; a deletion inserts
//! nothing; what the table refuses — a session, an element or a sound it does
//! not hold, an instant or an id it cannot read — rejects the command and
//! leaves the tables as they were. The DataCapabilities here are test-only.
//! Each test works in a scratch database of its own, migrated by the Data
//! layer, dropped when it ends.

use platform_test_kit::{Bench, Scratch, Served, planter, uuid};
use serde_json::{Value, json};

use harness_dataguard::command::{CommandResult, State};

const AGGREGATE: &str = include_str!("../aggregate.json");

/// A suggestion added: written once, under the id its payload names.
const ADD: &str = "test.addSuggestion";
/// The same, an upsert: the same suggestion can be written again.
const WRITE: &str = "test.writeSuggestion";
/// A suggestion removed: the engine's soft delete, a tombstone.
const REMOVE: &str = "test.removeSuggestion";

fn campaign() -> String {
    uuid(0xc1, 1)
}
fn session() -> String {
    uuid(0x51, 1)
}
fn morlox() -> String {
    uuid(0xe1, 2)
}
fn rain() -> String {
    uuid(0x50, 1)
}
fn heard() -> String {
    uuid(0x5a, 1)
}

/// Valombre, its live session 1, Morlox and the rain.
async fn scratch(label: &str) -> Scratch {
    let (campaign, session, morlox, rain) = (campaign(), session(), morlox(), rain());
    let scratch = Scratch::new(label).await;
    scratch
        .seed(&format!(
            "INSERT INTO campaign (id, name, kind)
                 VALUES ('{campaign}', 'Les Cendres de Valombre', 'campagne');
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                 VALUES ('{session}', '{campaign}', 1, 'Séance 1', 'live', true);
             INSERT INTO entity (id, campaign_id, type, name)
                 VALUES ('{morlox}', '{campaign}', 'pnj', 'Morlox');
             INSERT INTO character (entity_id, token_color) VALUES ('{morlox}', '#888');
             INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                 VALUES ('{morlox}', 1, 'dm', '{{}}', '[]');
             INSERT INTO sound (id, campaign_id, name, file_path)
                 VALUES ('{rain}', '{campaign}', 'Pluie', 'sounds/pluie.ogg');"
        ))
        .await;
    scratch
}

/// The engine as the server plants this aggregate over `scratch`, with the
/// test-only DataCapabilities.
async fn serve(scratch: &Scratch) -> Served {
    Bench::new(
        &planter(ADD, "Suggestion", "$id", "insert"),
        AGGREGATE,
        [aggregate_suggestion::plugin()],
    )
    .planting(&planter(WRITE, "Suggestion", "$id", "upsert"))
    .planting(&planter(REMOVE, "Suggestion", "$id", "delete"))
    .postgres(scratch)
    .await
}

/// Morlox, recognised in what the DM said.
fn morlox_heard() -> Value {
    json!({
        "id": heard(),
        "sessionId": session(),
        "entityId": morlox(),
        "matchedText": "Morlox",
        "detectedAt": "2026-10-18T20:31:07Z",
    })
}

/// The rain, recognised in what the DM said.
fn rain_heard() -> Value {
    json!({
        "id": heard(),
        "sessionId": session(),
        "soundId": rain(),
        "matchedText": "il pleut des cordes",
        "detectedAt": "2026-10-18T20:31:07Z",
    })
}

fn with(mut base: Value, field: &str, value: Value) -> Value {
    base[field] = value;
    base
}

/// Every `live_suggestion` row, as the table holds it.
async fn suggestions(scratch: &Scratch) -> Vec<Value> {
    scratch
        .rows(
            "SELECT id::text AS id, session_id::text AS session_id,
                 entity_id::text AS entity_id, sound_id::text AS sound_id,
                 matched_text, to_char(detected_at AT TIME ZONE 'UTC',
                     'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS detected_at
             FROM live_suggestion ORDER BY detected_at, id",
            &[],
        )
        .await
}

/// How many rows the engine holds for `partition`, tombstones included.
async fn engine_rows(scratch: &Scratch, partition: &str) -> i64 {
    scratch
        .client()
        .query_one(
            "SELECT count(*) FROM dataguard_rows WHERE partition = $1",
            &[&partition],
        )
        .await
        .expect("the engine's rows")
        .get(0)
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// The command is rejected, and neither the table nor the engine keeps
/// anything of it.
async fn nothing_written(scratch: &Scratch, result: &CommandResult) {
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(suggestions(scratch).await, Vec::<Value>::new());
    assert_eq!(
        engine_rows(scratch, &format!("Suggestion/{}", heard())).await,
        0
    );
}

#[tokio::test]
async fn a_suggestion_of_an_element_is_a_row_with_no_sound() {
    let scratch = scratch("suggestion_element").await;
    let served = serve(&scratch).await;
    applied(&served.applied(ADD, morlox_heard(), None).await);
    assert_eq!(
        suggestions(&scratch).await,
        [json!({
            "id": heard(),
            "session_id": session(),
            "entity_id": morlox(),
            "sound_id": null,
            "matched_text": "Morlox",
            "detected_at": "2026-10-18T20:31:07Z",
        })]
    );
}

#[tokio::test]
async fn a_suggestion_of_a_sound_is_a_row_with_no_element() {
    let scratch = scratch("suggestion_sound").await;
    let served = serve(&scratch).await;
    applied(&served.applied(ADD, rain_heard(), None).await);
    assert_eq!(
        suggestions(&scratch).await,
        [json!({
            "id": heard(),
            "session_id": session(),
            "entity_id": null,
            "sound_id": rain(),
            "matched_text": "il pleut des cordes",
            "detected_at": "2026-10-18T20:31:07Z",
        })]
    );
}

#[tokio::test]
async fn the_same_suggestion_applied_twice_is_one_row() {
    let scratch = scratch("suggestion_twice").await;
    let served = serve(&scratch).await;
    applied(&served.applied(WRITE, morlox_heard(), None).await);
    applied(&served.applied(WRITE, morlox_heard(), None).await);
    assert_eq!(suggestions(&scratch).await.len(), 1);
}

#[tokio::test]
async fn a_suggestion_is_written_once_and_never_rewritten() {
    let scratch = scratch("suggestion_once").await;
    let served = serve(&scratch).await;
    applied(&served.applied(WRITE, morlox_heard(), None).await);
    applied(
        &served
            .applied(
                WRITE,
                with(morlox_heard(), "matchedText", json!("Morlox le borgne")),
                None,
            )
            .await,
    );
    let rows = suggestions(&scratch).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["matched_text"], "Morlox");
}

#[tokio::test]
async fn two_suggestions_are_two_rows() {
    let scratch = scratch("suggestion_two").await;
    let served = serve(&scratch).await;
    applied(&served.applied(ADD, morlox_heard(), None).await);
    let rain = with(rain_heard(), "id", json!(uuid(0x5a, 2)));
    applied(&served.applied(ADD, rain, None).await);
    assert_eq!(suggestions(&scratch).await.len(), 2);
}

#[tokio::test]
async fn a_tombstone_inserts_nothing() {
    let scratch = scratch("suggestion_tombstone").await;
    let served = serve(&scratch).await;
    applied(&served.applied(REMOVE, json!({ "id": heard() }), None).await);
    assert_eq!(suggestions(&scratch).await, Vec::<Value>::new());
    // The engine's row is the tombstone the Capability leaves the suggestion
    // out for.
    assert!(
        scratch
            .client()
            .query_one(
                "SELECT tombstoned FROM dataguard_rows WHERE partition = $1",
                &[&format!("Suggestion/{}", heard())],
            )
            .await
            .expect("the tombstone")
            .get::<_, bool>(0)
    );
}

#[tokio::test]
async fn a_suggestion_deleted_after_its_insert_is_tombstoned_for_the_capability() {
    let scratch = scratch("suggestion_deleted").await;
    let served = serve(&scratch).await;
    applied(&served.applied(ADD, morlox_heard(), None).await);
    applied(&served.applied(REMOVE, json!({ "id": heard() }), None).await);
    assert_eq!(suggestions(&scratch).await.len(), 1);
    // What `live.listSuggestions` leaves out, by this very predicate.
    assert_eq!(
        scratch
            .rows(
                "SELECT g.id::text AS id FROM live_suggestion g
                 WHERE NOT EXISTS (SELECT 1 FROM dataguard_rows r
                     WHERE r.partition = 'Suggestion/' || g.id::text AND r.tombstoned)",
                &[],
            )
            .await,
        Vec::<Value>::new()
    );
}

#[tokio::test]
async fn a_session_the_tables_do_not_hold_rejects_the_command() {
    let scratch = scratch("suggestion_no_session").await;
    let served = serve(&scratch).await;
    let result = served
        .applied(
            ADD,
            with(morlox_heard(), "sessionId", json!(uuid(0x51, 0xffff))),
            None,
        )
        .await;
    nothing_written(&scratch, &result).await;
}

#[tokio::test]
async fn an_element_the_tables_do_not_hold_rejects_the_command() {
    let scratch = scratch("suggestion_no_element").await;
    let served = serve(&scratch).await;
    let result = served
        .applied(
            ADD,
            with(morlox_heard(), "entityId", json!(uuid(0xe1, 0xffff))),
            None,
        )
        .await;
    nothing_written(&scratch, &result).await;
}

#[tokio::test]
async fn a_sound_the_tables_do_not_hold_rejects_the_command() {
    let scratch = scratch("suggestion_no_sound").await;
    let served = serve(&scratch).await;
    let result = served
        .applied(
            ADD,
            with(rain_heard(), "soundId", json!(uuid(0x50, 0xffff))),
            None,
        )
        .await;
    nothing_written(&scratch, &result).await;
}

#[tokio::test]
async fn an_instant_the_table_cannot_read_rejects_the_command() {
    let scratch = scratch("suggestion_bad_instant").await;
    let served = serve(&scratch).await;
    // The judge reads a date and a time's shape; the table reads the month.
    let result = served
        .applied(
            ADD,
            with(morlox_heard(), "detectedAt", json!("2026-13-45T20:31:07Z")),
            None,
        )
        .await;
    nothing_written(&scratch, &result).await;
}

#[tokio::test]
async fn an_id_that_is_no_uuid_rejects_the_command() {
    let scratch = scratch("suggestion_bad_id").await;
    let served = serve(&scratch).await;
    let result = served
        .applied(ADD, with(morlox_heard(), "id", json!("porte")), None)
        .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(suggestions(&scratch).await, Vec::<Value>::new());
    assert_eq!(engine_rows(&scratch, "Suggestion/porte").await, 0);
}

#[tokio::test]
async fn a_table_that_is_gone_fails_the_step_instead_of_rejecting_the_command() {
    let scratch = scratch("suggestion_no_table").await;
    let served = serve(&scratch).await;
    scratch.seed("DROP TABLE live_suggestion").await;
    served
        .submit(ADD, morlox_heard(), None)
        .await
        .expect("queued");
    // Not the table refusing a value: the command is not judged, the step fails.
    let failure = served.engine.run_once().await.expect_err("the step fails");
    assert!(failure.to_string().contains("live_suggestion"), "{failure}");
    assert_eq!(
        engine_rows(&scratch, &format!("Suggestion/{}", heard())).await,
        0
    );
}
