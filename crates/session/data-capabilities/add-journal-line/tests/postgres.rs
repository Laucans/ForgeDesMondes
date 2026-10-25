//! `session.addJournalLine` over PostgreSQL (`DATABASE_URL`), as the server
//! plants it beside Session, whose projections write its tables: each test
//! works in a scratch database of its own, migrated as the server migrates
//! it, dropped when it ends (`platform_test_kit::Scratch`). A live session
//! the server loaded into `session` — which the engine holds no row of —
//! takes each phrase heard as the next line of `session_journal_line`, once,
//! and stays live with its microphone on; a session in prep, closed, or with
//! its microphone off takes none, and one nowhere is a 404.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use harness_dataguard::command::State;
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::store::Ids;
use platform_test_kit::{Bench, NOW, Scratch, Served, file, planter, refused, uuid};
use serde_json::{Value, json};

const ADD: &str = "session.addJournalLine";
/// A session's row written again, as any other command of the aggregate
/// writes it: test-only.
const WRITE: &str = "test.writeSession";

fn campaign() -> String {
    uuid(0xca, 1)
}
fn live() -> String {
    uuid(0xb0, 1)
}
fn prep() -> String {
    uuid(0xb0, 2)
}
fn closed() -> String {
    uuid(0xb0, 3)
}
fn muted() -> String {
    uuid(0xb0, 4)
}

/// Fresh line ids, uuids as the server's are.
#[derive(Default)]
struct Lines(AtomicU64);

impl Ids for Lines {
    fn fresh(&self) -> String {
        uuid(0x1a, self.0.fetch_add(1, Ordering::Relaxed) + 1)
    }
}

/// The engine as the server plants this unit and its aggregate over
/// `scratch`'s tables.
async fn served(scratch: &Scratch) -> Served {
    Bench::new(
        include_str!("../data-capability.json"),
        &file("crates/session/aggregates/Session/aggregate.json"),
        [aggregate_session::plugin(), add_journal_line::plugin()],
    )
    .planting(&planter(WRITE, "Session", "$sessionId", "update"))
    .ids(Arc::new(Lines::default()))
    .postgres(scratch)
    .await
}

/// A campaign and its sessions as the fixtures load them: live with the
/// microphone on, in prep, closed, and live with the microphone off.
async fn seeded(label: &str) -> Scratch {
    let scratch = Scratch::new(label).await;
    scratch
        .seed(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES ('{c}', 'Valombre', 'campagne');
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on, started_at, closed_at) VALUES
                ('{live}', '{c}', 1, 'Le gué', 'live', true, now(), NULL),
                ('{prep}', '{c}', 2, 'Le phare', 'prep', false, NULL, NULL),
                ('{closed}', '{c}', 3, 'La crypte', 'closed', false, now(), now()),
                ('{muted}', '{c}', 4, 'Le col', 'live', false, now(), NULL);",
            c = campaign(),
            live = live(),
            prep = prep(),
            closed = closed(),
            muted = muted(),
        ))
        .await;
    scratch
}

/// Every journal line: its id, its session, its instant and its text, by text.
async fn journal(scratch: &Scratch) -> Vec<Value> {
    scratch
        .rows(
            "SELECT id::text AS id, session_id::text AS session,
                floor(extract(epoch FROM spoken_at) * 1000)::int8 AS at, text
             FROM session_journal_line ORDER BY text",
            &[],
        )
        .await
}

/// The status and the microphone of `session`, as its row holds them.
async fn session(scratch: &Scratch, id: &str) -> Vec<Value> {
    scratch
        .rows(
            "SELECT status, microphone_on FROM session WHERE id = $1::text::uuid",
            &[json!(id)],
        )
        .await
}

#[tokio::test]
async fn text_only_no_audio_each_phrase_heard_is_the_next_line_of_the_live_session_s_journal() {
    let scratch = seeded("add_journal_line_live").await;
    let served = served(&scratch).await;
    for text in ["On reprend sous le pont.", "Morlox finit par avouer."] {
        let result = served
            .applied(ADD, json!({ "text": text }), Some(&live()))
            .await;
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    // Dated by the engine's clock (the bench's, in seconds).
    assert_eq!(
        journal(&scratch).await,
        [
            json!({ "id": uuid(0x1a, 2), "session": live(), "at": NOW * 1000, "text": "Morlox finit par avouer." }),
            json!({ "id": uuid(0x1a, 1), "session": live(), "at": NOW * 1000, "text": "On reprend sous le pont." }),
        ]
    );
    // The session is still live, its microphone on.
    assert_eq!(
        session(&scratch, &live()).await,
        [json!({ "status": "live", "microphone_on": true })]
    );
}

#[tokio::test]
async fn a_later_write_of_the_session_carries_its_last_line_and_adds_it_no_second_time() {
    let scratch = seeded("add_journal_line_once").await;
    let served = served(&scratch).await;
    served
        .applied(ADD, json!({ "text": "Une seule fois." }), Some(&live()))
        .await;
    let result = served
        .applied(
            WRITE,
            json!({ "sessionId": live(), "sessionTitle": "Le gué, suite" }),
            None,
        )
        .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(journal(&scratch).await.len(), 1);
}

#[tokio::test]
async fn status_lifecycle_a_session_in_prep_closed_or_its_microphone_off_takes_no_line() {
    let scratch = seeded("add_journal_line_refused").await;
    let served = served(&scratch).await;
    for (id, why) in [
        (prep(), "status-lifecycle"),
        (closed(), "status-lifecycle"),
        (muted(), "micro"),
    ] {
        let answer = served
            .submit(ADD, json!({ "text": "Encore une chose…" }), Some(&id))
            .await;
        let message = refused(answer, RefusalKind::Unprocessable);
        assert!(message.contains(why), "{id}: {message}");
    }
    assert_eq!(served.settled().await.len(), 0, "nothing was queued");
    assert!(journal(&scratch).await.is_empty());
}

#[tokio::test]
async fn a_session_nowhere_is_a_404_and_a_blank_phrase_is_refused() {
    let scratch = seeded("add_journal_line_nowhere").await;
    let served = served(&scratch).await;
    let answer = served
        .submit(ADD, json!({ "text": "x" }), Some(&uuid(0xb0, 9)))
        .await;
    assert!(!refused(answer, RefusalKind::NotFound).is_empty());
    let answer = served
        .submit(ADD, json!({ "text": "  " }), Some(&live()))
        .await;
    assert!(!refused(answer, RefusalKind::Unprocessable).is_empty());
    assert_eq!(served.settled().await.len(), 0);
    assert!(journal(&scratch).await.is_empty());
}
