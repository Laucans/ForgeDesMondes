//! The Session aggregate's microphone over PostgreSQL (`DATABASE_URL`): a
//! write of a session the tables hold moves its `status`, its `started_at`
//! — stamped once — and its `microphone_on`, in the transaction that applies
//! it; a row that is no session's, a tombstone, or a session the tables do
//! not hold writes nothing and rejects nothing; a status taken back
//! (`status-lifecycle`) is rejected, a session only the tables hold judged
//! from the row they hold. The DataCapabilities here are test-only: each
//! write is the row as given. Each test works in a scratch database of its
//! own, migrated as the server migrates it, dropped when it ends.

use std::collections::BTreeMap;

use aggregate_session::{DOCUMENT, Microphone, SCENE_POSITION, order_id, plugin, presence_id};
use harness_dataguard::command::State;
use harness_dataguard::plugins::ProjectPg;
use harness_dataguard::store::Write;
use platform_test_kit::{Bench, Scratch, Served, planter};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's sessions: in prep, live since a moment, closed, and one the
/// tables do not hold.
const PREP: &str = "51000000-0000-4000-8000-000000000001";
const LIVE: &str = "51000000-0000-4000-8000-000000000002";
const CLOSED: &str = "51000000-0000-4000-8000-000000000003";
const NOWHERE: &str = "51000000-0000-4000-8000-0000000000ff";
/// A PC of Valombre.
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
/// When `LIVE` went live, in milliseconds since the epoch, and a later moment.
const SINCE: u64 = 1_792_000_000_000;
const LATER: u64 = 1_792_000_600_000;

/// A session written, and a session deleted: test-only.
const WRITE: &str = "test.writeSession";
const REMOVE: &str = "test.removeSession";

/// Valombre with its three sessions, as the fixtures write them: rows only
/// the tables hold, the engine knows none.
async fn scratch(label: &str) -> Scratch {
    let scratch = Scratch::new(label).await;
    scratch
        .seed(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Valombre', 'campagne');
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                 ('{PREP}', '{VALOMBRE}', 1, 'La séance 1', 'prep', false);
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on, started_at) VALUES
                 ('{LIVE}', '{VALOMBRE}', 2, 'La séance 2', 'live', true, to_timestamp({SINCE} / 1000.0));
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on, started_at, closed_at) VALUES
                 ('{CLOSED}', '{VALOMBRE}', 3, 'La séance 3', 'closed', false,
                  to_timestamp({SINCE} / 1000.0), to_timestamp({SINCE} / 1000.0));"
        ))
        .await;
    scratch
}

/// The engine as the server plants this aggregate over `scratch`.
async fn served(scratch: &Scratch) -> Served {
    Bench::new(
        &planter(WRITE, "Session", "$sessionId", "update"),
        DOCUMENT,
        [plugin()],
    )
    .planting(&planter(REMOVE, "Session", "$sessionId", "delete"))
    .postgres(scratch)
    .await
}

/// The session `id` as `session` holds its three columns.
async fn session(scratch: &Scratch, id: &str) -> Vec<Value> {
    scratch
        .rows(
            "SELECT status, microphone_on,
                floor(extract(epoch FROM started_at) * 1000)::int8 AS started_at
             FROM session WHERE id = $1::text::uuid",
            &[json!(id)],
        )
        .await
}

/// The three columns of every session, by position.
async fn sessions(scratch: &Scratch) -> Vec<Value> {
    scratch
        .rows(
            "SELECT id::text AS id, status, microphone_on,
                floor(extract(epoch FROM started_at) * 1000)::int8 AS started_at
             FROM session ORDER BY position",
            &[],
        )
        .await
}

fn row(status: &str, microphone: bool, started_at: Option<u64>) -> Value {
    json!({ "status": status, "microphone_on": microphone, "started_at": started_at })
}

#[tokio::test]
async fn a_session_in_prep_goes_live_with_its_start_and_its_microphone_on() {
    let scratch = scratch("goes_live").await;
    let served = served(&scratch).await;
    let result = served
        .applied(
            WRITE,
            json!({ "sessionId": PREP, "sessionStatus": "live", "startedAt": SINCE, "microphoneOn": true }),
            None,
        )
        .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        session(&scratch, PREP).await,
        [row("live", true, Some(SINCE))]
    );
}

#[tokio::test]
async fn a_start_given_as_a_timestamp_text_is_stamped_too() {
    let scratch = scratch("start_text").await;
    let served = served(&scratch).await;
    let result = served
        .applied(
            WRITE,
            json!({ "sessionId": PREP, "sessionStatus": "live", "startedAt": "2026-10-25T14:00:00Z" }),
            None,
        )
        .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        session(&scratch, PREP).await,
        [row("live", false, Some(1_792_936_800_000))]
    );
}

#[tokio::test]
async fn a_microphone_turned_off_leaves_the_start_it_stamped_and_the_fields_it_does_not_carry() {
    let scratch = scratch("mic_off").await;
    let served = served(&scratch).await;
    let result = served
        .applied(
            WRITE,
            json!({ "sessionId": LIVE, "microphoneOn": false }),
            None,
        )
        .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        session(&scratch, LIVE).await,
        [row("live", false, Some(SINCE))]
    );
    // A later start does not move the one stamped, and the microphone, not
    // carried, stays off.
    let result = served
        .applied(
            WRITE,
            json!({ "sessionId": LIVE, "sessionStatus": "live", "startedAt": LATER }),
            None,
        )
        .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        session(&scratch, LIVE).await,
        [row("live", false, Some(SINCE))]
    );
}

#[tokio::test]
async fn a_write_moves_its_session_alone() {
    let scratch = scratch("alone").await;
    let served = served(&scratch).await;
    let before = sessions(&scratch).await;
    served
        .applied(
            WRITE,
            json!({ "sessionId": PREP, "sessionStatus": "live", "microphoneOn": true, "startedAt": SINCE }),
            None,
        )
        .await;
    let after = sessions(&scratch).await;
    assert_eq!(after[1..], before[1..]);
    assert_ne!(after[0], before[0]);
}

#[tokio::test]
async fn status_lifecycle_rejects_a_closed_session_going_back_to_live_and_the_table_stays() {
    let scratch = scratch("closed_back").await;
    let served = served(&scratch).await;
    let before = sessions(&scratch).await;
    // The engine never wrote this session: the judge sees the row the tables
    // hold, through the projection's hydrate.
    let result = served
        .applied(
            WRITE,
            json!({ "sessionId": CLOSED, "sessionStatus": "live", "microphoneOn": true, "startedAt": LATER }),
            None,
        )
        .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert!(
        result.violations.contains(&"status-lifecycle".to_owned()),
        "{result:?}"
    );
    assert_eq!(sessions(&scratch).await, before);
}

#[tokio::test]
async fn status_lifecycle_rejects_a_step_over_the_prep_to_closed_and_a_live_session_back_to_prep() {
    let scratch = scratch("steps").await;
    let served = served(&scratch).await;
    let before = sessions(&scratch).await;
    for (session, status) in [(PREP, "closed"), (LIVE, "prep")] {
        let result = served
            .applied(
                WRITE,
                json!({ "sessionId": session, "sessionStatus": status }),
                None,
            )
            .await;
        assert_eq!(result.status, State::Rejected, "{session}: {result:?}");
        assert!(
            result.violations.contains(&"status-lifecycle".to_owned()),
            "{result:?}"
        );
    }
    assert_eq!(sessions(&scratch).await, before);
}

#[tokio::test]
async fn a_session_the_tables_do_not_hold_writes_nothing_and_the_command_is_not_rejected() {
    let scratch = scratch("nowhere").await;
    let served = served(&scratch).await;
    let before = sessions(&scratch).await;
    let result = served
        .applied(
            WRITE,
            json!({ "sessionId": NOWHERE, "sessionStatus": "live", "microphoneOn": true, "startedAt": SINCE }),
            None,
        )
        .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(sessions(&scratch).await, before);
}

#[tokio::test]
async fn a_tombstone_writes_nothing() {
    let scratch = scratch("tombstone").await;
    let served = served(&scratch).await;
    let before = sessions(&scratch).await;
    let result = served
        .applied(REMOVE, json!({ "sessionId": LIVE }), None)
        .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(sessions(&scratch).await, before);
}

#[tokio::test]
async fn a_delete_is_no_way_round_status_lifecycle_and_the_table_stays() {
    let scratch = scratch("tombstone_back").await;
    let served = served(&scratch).await;
    let result = served
        .applied(REMOVE, json!({ "sessionId": CLOSED }), None)
        .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let before = sessions(&scratch).await;
    // The engine holds no row of the deleted session: the judge has no status
    // to compare, the table's is the one that refuses.
    let result = served
        .applied(
            WRITE,
            json!({ "sessionId": CLOSED, "sessionStatus": "live", "microphoneOn": true, "startedAt": LATER }),
            None,
        )
        .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(sessions(&scratch).await, before);
}

/// What `Microphone` makes of `write`, committed.
async fn project(scratch: &Scratch, partition: &str, values: Value, tombstone: bool) {
    let Value::Object(values) = values else {
        panic!("a row is an object");
    };
    let write = Write {
        partition: partition.to_owned(),
        values: values.into_iter().collect::<BTreeMap<_, _>>(),
        tombstone,
    };
    let mut client = scratch.connect().await;
    let tx = client.transaction().await.expect("a transaction");
    Microphone
        .project(&write, &tx)
        .await
        .expect("the microphone projected");
    tx.commit().await.expect("committed");
}

#[tokio::test]
async fn the_rows_beside_a_session_move_no_session() {
    let scratch = scratch("beside").await;
    let before = sessions(&scratch).await;
    let order = format!("Session/{}", order_id(PREP).expect("an order's id"));
    let presence = format!("Session/{}", presence_id(PREP, ELOWEN));
    for (partition, values) in [
        // A scene, an order, a presence and an entry « à créer ».
        (
            "Session/5c000000-0000-4000-8000-000000000001".to_owned(),
            json!({ "sceneId": "5c000000-0000-4000-8000-000000000001", "sceneSessionId": PREP, "sceneTitle": "La route" }),
        ),
        (
            order,
            json!({ SCENE_POSITION: ["5c000000-0000-4000-8000-000000000001"] }),
        ),
        (
            presence,
            json!({ "sessionPresentCharacterSessionId": PREP, "sessionPresentCharacterCharacterId": ELOWEN }),
        ),
        (
            "Session/7e000000-0000-4000-8000-000000000001".to_owned(),
            json!({ "entityToCreateId": "7e000000-0000-4000-8000-000000000001", "entityToCreateStatus": "a_creer", "entityToCreateSessionId": PREP }),
        ),
        // A session's own row, with none of the three.
        (
            format!("Session/{PREP}"),
            json!({ "sessionId": PREP, "sessionTitle": "Un autre titre" }),
        ),
    ] {
        project(&scratch, &partition, values, false).await;
    }
    // Even a row that is no session's, carrying a status, moves none.
    project(
        &scratch,
        &format!("Session/{}", presence_id(PREP, ELOWEN)),
        json!({ "sessionStatus": "live", "microphoneOn": true, "startedAt": SINCE }),
        false,
    )
    .await;
    project(
        &scratch,
        &format!("Session/{PREP}"),
        json!({ "sessionStatus": "live", "microphoneOn": true, "startedAt": SINCE }),
        true,
    )
    .await;
    assert_eq!(sessions(&scratch).await, before);
}

#[tokio::test]
async fn a_close_is_left_to_the_close_s_projection_and_moves_nothing_here() {
    let scratch = scratch("close_left").await;
    let before = sessions(&scratch).await;
    // `session.closeSession`'s projection finds the row still live only if
    // this one has not moved it first.
    project(
        &scratch,
        &format!("Session/{LIVE}"),
        json!({ "sessionId": LIVE, "sessionStatus": "closed", "microphoneOn": false, "startedAt": SINCE }),
        false,
    )
    .await;
    assert_eq!(sessions(&scratch).await, before);
}

#[tokio::test]
async fn the_hydrated_session_is_its_whole_row() {
    let scratch = scratch("hydrate_whole").await;
    // The engine's own tables, which the hydrate reads for a tombstone.
    let _served = served(&scratch).await;
    let client = scratch.connect().await;
    // The engine takes the first hydrate that knows the row: a close heads
    // its summary with the title this one gives.
    let row = Microphone
        .hydrate(&format!("Session/{LIVE}"), &client)
        .await
        .expect("hydrated")
        .expect("the tables hold it");
    assert_eq!(row["sessionTitle"], "La séance 2");
    assert_eq!(row["sessionPosition"], 2);
    assert_eq!(row["sessionStatus"], "live");
    assert_eq!(row["startedAt"], SINCE);
    assert!(!row.contains_key("plannedOn"), "{row:?}");
}

#[tokio::test]
async fn a_status_the_table_does_not_take_rejects_the_command() {
    let scratch = scratch("bad_status").await;
    let mut client = scratch.connect().await;
    let tx = client.transaction().await.expect("a transaction");
    let write = Write {
        partition: format!("Session/{PREP}"),
        values: BTreeMap::from([("sessionStatus".to_owned(), json!("paused"))]),
        tombstone: false,
    };
    let refused = Microphone.project(&write, &tx).await;
    assert!(
        matches!(
            refused,
            Err(harness_dataguard::plugins::ProjectionError::Refused(_))
        ),
        "{refused:?}"
    );
}
