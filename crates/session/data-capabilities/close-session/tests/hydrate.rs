//! The Session aggregate's hydration by `Closes`, called as the engine calls
//! it: the row `session` holds, as the aggregate names its fields, the
//! fields the row holds no value for left out; `None` for a session the
//! tables lack and for a partition that is no uuid. The test works in a
//! scratch database of its own ([`ScratchDb`]), migrated as the server
//! migrates it, dropped when it ends.

use std::path::Path;

use close_session::Closes;
use harness_dataguard::plugins::ProjectPg;
use platform_scratch_db::ScratchDb;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's session 4, live, its microphone on.
const LIVE: &str = "51000000-0000-4000-8000-000000000004";
/// Valombre's session 3, closed, summarised.
const CLOSED: &str = "51000000-0000-4000-8000-000000000003";
/// A session the tables lack.
const ABSENT: &str = "51000000-0000-4000-8000-0000000000ff";

#[tokio::test]
async fn hydrate_reads_the_row_the_tables_hold_and_nothing_for_a_partition_no_session_s() {
    let db = ScratchDb::create("close_session_hydrate")
        .await
        .expect("DATABASE_URL names the harness's PostgreSQL");
    let mut client = db.connect().await.expect("PostgreSQL");
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
    data::migrate(&mut client, &migrations)
        .await
        .expect("migrated");
    client
        .batch_execute(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES
                 ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
             INSERT INTO session (id, campaign_id, position, title, status, started_at, microphone_on)
                 VALUES ('{LIVE}', '{VALOMBRE}', 4, 'L''embuscade du Gué', 'live', '2026-10-17 20:02:00+00', true);
             INSERT INTO session (id, campaign_id, position, title, status, started_at, closed_at,
                                  microphone_on, summary, summary_model)
                 VALUES ('{CLOSED}', '{VALOMBRE}', 3, 'Le marché noir', 'closed', '2026-10-03 20:00:00+00',
                         '2026-10-03 23:00:00+00', false, 'Morlox a avoué.', 'qwen2.5-14b-instruct');"
        ))
        .await
        .expect("Valombre and its sessions");

    let hydrate =
        async |partition: &str| Closes.hydrate(partition, &client).await.expect("hydrated");

    // A live session: the fields its row holds, the others left out.
    let live = hydrate(&format!("Session/{LIVE}")).await.expect("a row");
    assert_eq!(live["sessionId"], LIVE);
    assert_eq!(live["sessionCampaignId"], VALOMBRE);
    assert_eq!(live["sessionPosition"], 4);
    assert_eq!(live["sessionTitle"], "L'embuscade du Gué");
    assert_eq!(live["sessionStatus"], "live");
    assert_eq!(live["microphoneOn"], true);
    assert!(live.contains_key("startedAt"), "{live:?}");
    for absent in [
        "closedAt",
        "summary",
        "summaryModel",
        "summaryEditedAt",
        "plannedOn",
    ] {
        assert!(!live.contains_key(absent), "{absent} in {live:?}");
    }

    // A closed session: its close and its summary too.
    let closed = hydrate(&format!("Session/{CLOSED}")).await.expect("a row");
    assert_eq!(closed["sessionId"], CLOSED);
    assert_eq!(closed["sessionStatus"], "closed");
    assert_eq!(closed["microphoneOn"], false);
    assert_eq!(closed["summary"], "Morlox a avoué.");
    assert_eq!(closed["summaryModel"], "qwen2.5-14b-instruct");
    assert!(closed.contains_key("closedAt"), "{closed:?}");

    // A bare id names the session as well as `Session/<id>`.
    assert_eq!(hydrate(CLOSED).await, Some(closed));

    // A session the tables lack, a partition that is no uuid.
    for partition in [
        format!("Session/{ABSENT}"),
        "Session/order-1".into(),
        "0a1b".into(),
    ] {
        assert_eq!(hydrate(&partition).await, None, "{partition}");
    }
    drop(client);
    drop(db);
}
