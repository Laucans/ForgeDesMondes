//! campaign.listCampaigns over PostgreSQL (`DATABASE_URL`): the real SQL of the
//! Capability, run on a scratch database migrated as the server migrates it,
//! and answered by `answer`. The sessions are counted from the `session`
//! table; each test works in a database of its own, dropped when it ends.

use std::collections::BTreeMap;

use list_campaigns::{SQL, answer};
use platform_test_kit::{Scratch, read};
use serde_json::{Value, json};

const CENDRES: &str = "c1000000-0000-4000-8000-000000000001";
const NUIT: &str = "c1000000-0000-4000-8000-000000000002";
const BRUMES: &str = "c1000000-0000-4000-8000-000000000003";

/// A session of `campaign` (`status`; `started` a timestamp literal or NULL; `planned` a SQL
/// date expression or NULL).
fn session(campaign: &str, position: u8, status: &str, started: &str, planned: &str) -> String {
    format!(
        "INSERT INTO session (campaign_id, position, title, status, started_at, planned_on, microphone_on) \
         VALUES ('{campaign}', {position}, 'Séance {position}', '{status}', {started}, {planned}, false);"
    )
}

/// Two campaigns and a oneshot, newest first: Brumes (no session), La Nuit (one session) and
/// Les Cendres (a closed session started on 2026-10-03, a prep one a week from now).
async fn seeded(label: &str) -> Scratch {
    let db = Scratch::new(label).await;
    db.seed(&format!(
        "INSERT INTO campaign (id, name, kind, art, glyph, pitch, created_at) VALUES \
         ('{CENDRES}', 'Les Cendres', 'campagne', 'ember', 'C', 'Un bourg', '2026-01-01T00:00:00Z'), \
         ('{NUIT}', 'La Nuit', 'oneshot', 'dusk', 'N', 'Un masque', '2026-02-01T00:00:00Z'), \
         ('{BRUMES}', 'Les Brumes', 'campagne', 'night', 'B', NULL, '2026-03-01T00:00:00Z');"
    ))
    .await;
    db.seed(&session(
        CENDRES,
        1,
        "closed",
        "timestamptz '2026-10-03 20:00:00+00'",
        "NULL",
    ))
    .await;
    db.seed(&session(CENDRES, 2, "prep", "NULL", "CURRENT_DATE + 7"))
        .await;
    db.seed(&session(NUIT, 1, "closed", "NULL", "date '2026-07-11'"))
        .await;
    db
}

async fn listed(db: &Scratch) -> Value {
    answer(&Value::Array(db.rows(SQL, &[]).await), &BTreeMap::new())
}

async fn day(db: &Scratch, expression: &str) -> String {
    let rows = db
        .rows(
            &format!("SELECT to_char({expression}, 'YYYY-MM-DD') AS day"),
            &[],
        )
        .await;
    rows[0]["day"].as_str().expect("a day").to_string()
}

#[tokio::test]
async fn three_items_appear_newest_first_with_the_keys_of_the_fixture() {
    let db = seeded("list_campaigns_three").await;
    let listed = listed(&db).await;
    let items = listed.as_array().expect("an array");
    let ids: Vec<&Value> = items.iter().map(|c| &c["id"]).collect();
    assert_eq!(ids, [BRUMES, NUIT, CENDRES]);
    let keys = |item: &Value| -> Vec<String> {
        let mut keys: Vec<String> = item
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect();
        keys.sort_unstable();
        keys
    };
    let fixture = read::fixture("campaign.listCampaigns");
    for item in items {
        assert_eq!(keys(item), keys(&fixture[0]), "{item}");
    }
    assert_eq!(items[1]["type"], "oneshot");
}

#[tokio::test]
async fn a_campaign_without_session_counts_zero_and_has_no_date() {
    let db = seeded("list_campaigns_none").await;
    let brumes = listed(&db).await[0].clone();
    assert_eq!(brumes["sessions"], json!(0));
    assert_eq!(brumes["lastPlayed"], Value::Null);
    assert_eq!(brumes["next"], Value::Null);
}

#[tokio::test]
async fn a_oneshot_with_one_session_counts_one() {
    let db = seeded("list_campaigns_oneshot").await;
    let nuit = listed(&db).await[1].clone();
    assert_eq!(nuit["sessions"], json!(1));
    // Never started: its planned day stands for the day it was played.
    assert_eq!(nuit["lastPlayed"], "2026-07-11");
    assert_eq!(nuit["next"], Value::Null);
}

#[tokio::test]
async fn a_closed_and_a_prep_session_give_the_last_played_and_the_next_day() {
    let db = seeded("list_campaigns_dates").await;
    let cendres = listed(&db).await[2].clone();
    assert_eq!(cendres["sessions"], json!(2));
    assert_eq!(cendres["lastPlayed"], "2026-10-03");
    assert_eq!(cendres["next"], day(&db, "CURRENT_DATE + 7").await.as_str());
}

#[tokio::test]
async fn a_prep_session_planned_before_today_is_no_next_session() {
    let db = Scratch::new("list_campaigns_past").await;
    db.seed(&format!(
        "INSERT INTO campaign (id, name, kind) VALUES ('{CENDRES}', 'Les Cendres', 'campagne');"
    ))
    .await;
    db.seed(&session(CENDRES, 1, "prep", "NULL", "CURRENT_DATE - 1"))
        .await;
    db.seed(&session(CENDRES, 2, "prep", "NULL", "NULL")).await;
    let cendres = listed(&db).await[0].clone();
    assert_eq!(cendres["sessions"], json!(2));
    assert_eq!(cendres["next"], Value::Null);
    assert_eq!(cendres["lastPlayed"], Value::Null);
}

#[tokio::test]
async fn today_is_a_next_session_and_the_earliest_prep_day_wins() {
    let db = Scratch::new("list_campaigns_today").await;
    db.seed(&format!(
        "INSERT INTO campaign (id, name, kind) VALUES ('{CENDRES}', 'Les Cendres', 'campagne');"
    ))
    .await;
    db.seed(&session(CENDRES, 1, "prep", "NULL", "CURRENT_DATE + 3"))
        .await;
    db.seed(&session(CENDRES, 2, "prep", "NULL", "CURRENT_DATE"))
        .await;
    db.seed(&session(
        CENDRES,
        3,
        "live",
        "timestamptz '2026-05-01 23:30:00+00'",
        "NULL",
    ))
    .await;
    db.seed(&session(CENDRES, 4, "closed", "NULL", "date '2026-04-01'"))
        .await;
    let cendres = listed(&db).await[0].clone();
    assert_eq!(cendres["sessions"], json!(4));
    assert_eq!(cendres["next"], day(&db, "CURRENT_DATE").await.as_str());
    // The latest played day, whichever the status (live or closed) and the order of the rows.
    assert_eq!(cendres["lastPlayed"], "2026-05-01");
}
