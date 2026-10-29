//! The session under way, its scenes and the battlemap instance of its live
//! scene, loaded from the mock-up's fixtures and read back through the server
//! by the names the Micro-UIs know them by. A `ScratchDb` of each test's own.

use std::path::Path;

use platform_scratch_db::ScratchDb;
use serde_json::{Value, json};
use server::{fixtures, id as uuid_of};

#[path = "common/served.rs"]
mod served;

use served::{folder, read, served};

fn fixture(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(folder().join(name)).expect("a fixture"))
        .expect("JSON")
}

/// A scratch database migrated and loaded with the fixtures, once.
async fn loaded(label: &str) -> (ScratchDb, data::Client) {
    let db = ScratchDb::create(label).await.expect("a scratch database");
    let mut client = db.connect().await.expect("PostgreSQL");
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/migrations");
    data::migrate(&mut client, &migrations)
        .await
        .expect("migrated");
    let plan = fixtures(&folder()).expect("the fixtures");
    data::insert_missing(&mut client, &plan.tables)
        .await
        .expect("the fixtures are inserted");
    (db, client)
}

#[tokio::test]
async fn the_real_store_has_the_session_under_way_its_scenes_and_the_live_battlemap() {
    let (db, _client) = loaded("live_session_reads").await;
    let served = served(&db).await;

    // AC1: with no id, the running session is s4, live.
    let live = read(&served, "session.getLiveSession", json!({})).await;
    assert_eq!(live["session"]["id"], "s4", "{live}");
    assert_eq!(live["session"]["status"], "live", "{live}");
    assert_eq!(live["session"]["startedAt"], "20:02", "{live}");

    // AC2: the scenes of the board, in order, sc3 naming its battlemap.
    let scenes = read(&served, "session.listScenes", json!({ "session": "s4" })).await;
    let board = fixture("SceneBoard.json");
    let expected: Vec<&Value> = board["scenes"]
        .as_array()
        .expect("scenes")
        .iter()
        .map(|s| &s["id"])
        .collect();
    let listed: Vec<&Value> = scenes
        .as_array()
        .expect("a list")
        .iter()
        .map(|s| &s["id"])
        .collect();
    assert_eq!(listed, expected, "{scenes}");
    let sc3 = &scenes[2];
    assert_eq!(sc3["id"], "sc3");
    assert_eq!(sc3["battlemap"], "bmi-gue-s4", "{sc3}");

    // AC3: the instance, with its PCs and the three bandits at 11/11.
    let instance = read(
        &served,
        "table.getBattlemapInstance",
        json!({ "instance": "bmi-gue-s4", "view": "dm" }),
    )
    .await;
    assert_eq!(instance["id"], "bmi-gue-s4", "{instance}");
    let tokens = instance["state"]["tokens"].as_array().expect("tokens");
    let token = |name: &str| {
        tokens
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("no token {name}: {instance}"))
    };
    assert_eq!(token("Elowen")["kind"], "pj");
    for bandit in ["Bandit 1", "Bandit 2", "Bandit 3"] {
        assert_eq!(token(bandit)["hp"], 11, "{bandit}");
        assert_eq!(token(bandit)["hpMax"], 11, "{bandit}");
    }
}

#[tokio::test]
async fn a_restart_loads_nothing_twice_and_leaves_the_session_the_dm_closed() {
    let (db, mut client) = loaded("live_session_restart").await;
    let plan = fixtures(&folder()).expect("the fixtures");
    let count = |sql: &'static str| {
        let client = &client;
        async move {
            client
                .query_one(sql, &[])
                .await
                .expect("a count")
                .get::<_, i64>(0)
        }
    };
    let before = (
        count("SELECT count(*) FROM session").await,
        count("SELECT count(*) FROM scene").await,
        count("SELECT count(*) FROM battlemap_instance").await,
        count("SELECT count(*) FROM token").await,
    );
    // What the fixtures plan: the list's sessions and the one a oneshot's creation writes.
    let planned = |table: &str| -> i64 {
        let rows: usize = plan
            .tables
            .iter()
            .filter(|t| t.name == table)
            .map(|t| t.rows.len())
            .sum();
        i64::try_from(rows).expect("a count")
    };
    assert_eq!(
        before,
        (
            planned("session"),
            planned("scene"),
            planned("battlemap_instance"),
            planned("token")
        )
    );
    assert!(before.0 > 0 && before.1 > 0 && before.2 > 0 && before.3 > 0);

    // AC4: the same plan again inserts nothing.
    let again = fixtures(&folder()).expect("the fixtures");
    assert_eq!(
        data::insert_missing(&mut client, &again.tables)
            .await
            .expect("planned again"),
        0
    );

    // The store's live session is the one under way, until the DM closes it.
    let s4 = uuid_of("session", "s4");
    assert_eq!(
        server::held_live_sessions(&client).await.expect("held"),
        std::slice::from_ref(&s4)
    );

    // AC4: the DM closed the session since; the load leaves their row alone.
    client
        .batch_execute(&format!(
            "UPDATE session SET status = 'closed', closed_at = now() WHERE id = '{s4}'"
        ))
        .await
        .expect("the DM closes the session");
    assert!(
        server::held_live_sessions(&client)
            .await
            .expect("held")
            .is_empty()
    );
    let restarted = fixtures(&folder()).expect("the fixtures");
    assert_eq!(
        data::insert_missing(&mut client, &restarted.tables)
            .await
            .expect("planned after the restart"),
        0
    );
    let row = client
        .query_one(
            &format!("SELECT status, started_at IS NOT NULL FROM session WHERE id = '{s4}'"),
            &[],
        )
        .await
        .expect("the session");
    assert_eq!(row.get::<_, String>(0), "closed", "the DM's row is kept");
    assert!(row.get::<_, bool>(1), "and so is its start time");
    let served = served(&db).await;
    assert_eq!(
        read(&served, "session.getLiveSession", json!({})).await,
        Value::Null,
        "no session is under way once the DM closed it"
    );
}
