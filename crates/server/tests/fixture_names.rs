//! The mock-up's fixtures, loaded into the tables, read back through the
//! server by the names the Micro-UIs know them by (D8): the slugs go in, the
//! query runs on the stored uuids, and the slugs come back out — the answers
//! the mock-up's own fixtures give. A scratch database of its own
//! (`DATABASE_URL`), dropped when the test ends.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::MEMORY;
use dataguard_server::engine::tokio_postgres::{self, Client, NoTls};
use ports_character_sheets::SharedCharacterSheets;
use ports_character_sheets::fake::FakeCharacterSheets;
use ports_llm::SharedLlm;
use ports_llm::fake::FakeLlm;
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};
use server::{
    Config, Names, app, fixtures, held_instances, held_versions, id as uuid_of, plant, renaming,
    shared,
};
use tower::ServiceExt;

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

/// `url()` naming database `name`.
fn named(name: &str) -> String {
    let url = url();
    let (base, query) = url
        .split_once('?')
        .map_or((url.as_str(), ""), |(b, q)| (b, q));
    let (server, _) = base.rsplit_once('/').expect("a database in the URL");
    if query.is_empty() {
        format!("{server}/{name}")
    } else {
        format!("{server}/{name}?{query}")
    }
}

async fn connect(url: &str) -> Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures");
    serde_json::from_str(&std::fs::read_to_string(path.join(name)).expect("the fixture"))
        .expect("JSON")
}

/// What `app` answers to a read of `capability` with `variables`.
async fn read(app: &axum::Router, capability: &str, variables: Value) -> Value {
    let request = Request::post(format!("/capabilities/{capability}"))
        .header("content-type", "application/json")
        .body(Body::from(json!({ "variables": variables }).to_string()))
        .expect("a request");
    let response = app.clone().oneshot(request).await.expect("an answer");
    assert_eq!(response.status(), StatusCode::OK, "{capability}");
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    serde_json::from_slice::<Value>(&bytes).expect("JSON")["data"].clone()
}

/// The `(id, player)` of every character of `listed`.
fn characters(listed: &Value) -> BTreeSet<(String, String)> {
    listed
        .as_array()
        .expect("a list")
        .iter()
        .map(|c| (c["id"].to_string(), c["player"].to_string()))
        .collect()
}

#[tokio::test]
async fn the_fixtures_are_read_back_by_the_names_the_mock_up_gives_them() {
    let name = format!("fixture_names_{}", std::process::id());
    let admin = connect(&url()).await;
    for sql in [
        format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
        format!("CREATE DATABASE {name}"),
    ] {
        admin.batch_execute(&sql).await.expect("a scratch database");
    }
    let mut client = connect(&named(&name)).await;
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/migrations");
    data::migrate(&mut client, &migrations)
        .await
        .expect("migrated");
    let plan = fixtures(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures"))
        .expect("the fixtures");
    data::insert_missing(&mut client, &plan.tables)
        .await
        .expect("the fixtures are inserted");
    // The battlemap's media, as the first load left it: a restart leaves it as it is.
    let medium = format!(
        "SELECT (m.*)::text FROM media m JOIN battlemap_grid g ON g.media_id = m.id \
         WHERE m.id = '{}'",
        uuid_of("media", "bm-gue")
    );
    let before: String = client
        .query_one(&medium, &[])
        .await
        .expect("a medium")
        .get(0);
    // A restart plans the same rows and loads none of them twice.
    let again = fixtures(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures"))
        .expect("the fixtures");
    assert_eq!(
        data::insert_missing(&mut client, &again.tables)
            .await
            .expect("the fixtures are planned again"),
        0
    );
    let count = |table: &'static str| {
        let client = &client;
        async move {
            let row = client
                .query_one(&format!("SELECT count(*) FROM {table}"), &[])
                .await
                .expect("a count");
            row.get::<_, i64>(0)
        }
    };
    assert_eq!(count("battlemap_instance").await, 1, "no instance twice");
    assert_eq!(count("token").await, 8, "no token twice");
    let after: String = client
        .query_one(&medium, &[])
        .await
        .expect("a medium")
        .get(0);
    assert_eq!(after, before, "the original battlemap is unchanged");
    // The DM took Kael off the second session and ticked him again since: the
    // row is no longer the fixture's, and the load leaves it alone.
    client
        .batch_execute(&format!(
            "UPDATE session_present_character SET added_at = now() \
             WHERE session_id = '{}' AND character_id = '{}'",
            uuid_of("session", "s2"),
            uuid_of("entity", "kael"),
        ))
        .await
        .expect("an update");
    assert_eq!(
        data::insert_missing(&mut client, &again.tables)
            .await
            .expect("a presence the DM changed is not loaded again"),
        0
    );
    // `session.listScenes` leaves out a battlemap whose partition is tombstoned, in the
    // engine's `dataguard_rows`, which the Postgres store makes and this memory engine does not.
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS dataguard_rows (partition text PRIMARY KEY, \
             fields jsonb NOT NULL DEFAULT '{}'::jsonb, tombstoned boolean NOT NULL DEFAULT false)",
        )
        .await
        .expect("the engine's rows table");
    let names = plan.names;

    // The engine over memory, its Capabilities reading the scratch database,
    // as the binary plants it when DATABASE_URL_READONLY names one.
    let config = Config::read(|var| match var {
        "DATABASE_URL" => Some(MEMORY.to_string()),
        "DATABASE_URL_READONLY" => Some(named(&name)),
        _ => None,
    })
    .expect("a valid configuration");
    let llm = SharedLlm::new(FakeLlm::new());
    let speech = SharedSpeechToText::new(FakeSpeechToText::new(String::new()));
    let sheets = SharedCharacterSheets::new(FakeCharacterSheets::new());
    let planted = plant(&config, shared(&llm, &speech, &sheets))
        .await
        .expect("the in-memory engine");
    let served = app(&planted, Arc::new(llm));
    let renamed = renaming(served.clone(), Names::new(names));

    let campaign = json!({ "campaign": "cendres-valombre" });
    let listed = read(&renamed, "campaign.listCharacters", campaign.clone()).await;
    assert!(characters(&listed).len() >= 5, "{listed}");
    assert_eq!(
        characters(&listed),
        characters(&fixture("campaign.listCharacters.json")),
        "{listed}"
    );
    let players = read(&renamed, "campaign.listPlayers", campaign.clone()).await;
    let ids = |players: &Value| -> BTreeSet<String> {
        players
            .as_array()
            .expect("a list")
            .iter()
            .map(|p| p["id"].to_string())
            .collect()
    };
    assert_eq!(
        ids(&players),
        ids(&fixture("campaign.listPlayers.json")),
        "{players}"
    );

    let sheet = read(&renamed, "campaign.getEntity", json!({ "id": "elowen" })).await;
    assert_eq!(sheet["id"], "elowen", "{sheet}");
    assert_eq!(sheet["player"], "jou-julie", "{sheet}");
    // The history of the elements the fixture gives one: its versions, newest
    // first, dated and numbered as the fixture has them. Only a creation's
    // origin differs — the table knows no import, and a first version is the
    // DM's — and what a version changed is told by the fields of the sheet.
    let described = fixture("campaign.listEntityVersions.json");
    for (slug, account) in described.as_object().expect("histories") {
        let history = read(
            &renamed,
            "campaign.listEntityVersions",
            json!({ "entity": slug }),
        )
        .await;
        assert_eq!(history["name"], account["name"], "{history}");
        let versions = |history: &Value| -> Vec<(Value, Value)> {
            history["versions"]
                .as_array()
                .expect("versions")
                .iter()
                .map(|v| (v["v"].clone(), v["date"].clone()))
                .collect()
        };
        assert_eq!(versions(&history), versions(account), "{slug}: {history}");
        for (answered, told) in history["versions"]
            .as_array()
            .expect("versions")
            .iter()
            .zip(account["versions"].as_array().expect("versions"))
        {
            if answered["v"] != 1 {
                assert_eq!(answered["origin"], told["origin"], "{slug} {answered}");
            }
            assert!(
                answered["changes"]
                    .as_array()
                    .is_some_and(|c| !c.is_empty()),
                "{slug} {answered}"
            );
        }
    }
    let overview = read(
        &renamed,
        "campaign.getCampaign",
        json!({ "id": "cendres-valombre" }),
    )
    .await;
    assert_eq!(overview["id"], "cendres-valombre", "{overview}");

    // The media the fixtures describe are loaded with them, by their names.
    let battlemap = read(&renamed, "media.getBattlemap", json!({ "id": "bm-moulin" })).await;
    assert_eq!(battlemap["id"], "bm-moulin", "{battlemap}");
    assert_eq!(battlemap["place"], "moulin", "{battlemap}");
    assert_eq!(battlemap["grid"]["orient"], "flat", "{battlemap}");
    assert_eq!(
        battlemap["grid"]["opacity"].as_f64(),
        Some(0.55),
        "{battlemap}"
    );
    let library = read(
        &renamed,
        "media.listMedia",
        json!({ "campaignId": "cendres-valombre" }),
    )
    .await;
    let described = fixture("media.listMedia.json");
    let media = |library: &Value, kind: &str| -> BTreeSet<(String, String)> {
        library[kind]
            .as_array()
            .expect("a list")
            .iter()
            .map(|m| (m["id"].to_string(), m["place"].to_string()))
            .collect()
    };
    for kind in ["battlemaps", "illustrations"] {
        assert_eq!(media(&library, kind), media(&described, kind), "{library}");
    }
    assert_eq!(library["illustrations"].as_array().map(Vec::len), Some(5));

    // The sessions, the last brief and the to-create list, by their slugs.
    let sessions = read(&renamed, "session.listSessions", campaign.clone()).await;
    let described = fixture("session.listSessions.json");
    let summary = |sessions: &Value| -> Vec<(String, Value, Value, Value)> {
        sessions
            .as_array()
            .expect("a list")
            .iter()
            .map(|s| {
                (
                    s["id"].to_string(),
                    s["order"].clone(),
                    s["title"].clone(),
                    s["present"].clone(),
                )
            })
            .collect()
    };
    assert_eq!(summary(&sessions).len(), 4, "{sessions}");
    assert_eq!(
        summary(&sessions),
        summary(&described),
        "in order: {sessions}"
    );
    // The session under way is the one `session.getLiveSession.json` names.
    assert_eq!(sessions[3]["status"], "live", "{sessions}");
    assert_eq!(sessions[0]["status"], "closed", "{sessions}");

    let listed = read(&renamed, "campaign.listEntitiesToCreate", campaign.clone()).await;
    let listed = listed["items"].clone();
    let described = fixture("campaign.listEntitiesToCreate.json");
    let entry = |list: &Value, id: &str| -> Value {
        list.as_array()
            .expect("a list")
            .iter()
            .find(|e| e["id"] == id)
            .unwrap_or_else(|| panic!("{id} in {list}"))
            .clone()
    };
    assert_eq!(listed.as_array().map(Vec::len), Some(3), "{listed}");
    for id in ["tc-odric", "tc-vesna", "tc-gue"] {
        let (got, want) = (entry(&listed, id), entry(&described, id));
        for key in ["type", "name", "session", "excerpt", "status", "entity"] {
            assert_eq!(got[key], want[key], "{id}.{key}: {got}");
        }
    }

    let brief = read(&renamed, "session.getLastBrief", json!({ "session": "s4" })).await;
    let described = fixture("session.getLastBrief.json");
    assert_eq!(brief["session"], "s4", "{brief}");
    assert_eq!(brief["transcript"], described["transcript"], "{brief}");
    assert_eq!(brief["duration"], "04:12", "{brief}");
    assert_eq!(brief["lastRun"], described["lastRun"], "{brief}");
    // The brief's scenes are the session's, now loaded with the elements they link; the
    // fixture also lists what is « à créer », which is the to-create entries' own `scene_id`
    // and not loaded, so the loaded links are the fixture's without those.
    let scenes_of = |brief: &Value| -> Vec<(Value, Value, Vec<String>)> {
        brief["result"]["scenes"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|scene| {
                let links = scene["links"].as_array().expect("links");
                let links = links.iter().filter_map(Value::as_str);
                let links = links.map(|l| l.trim_end_matches(" (à créer)").to_string());
                (
                    scene["order"].clone(),
                    scene["title"].clone(),
                    links.collect(),
                )
            })
            .collect()
    };
    let (loaded, wanted) = (scenes_of(&brief), scenes_of(&described));
    assert_eq!(loaded.len(), 3, "{brief}");
    for ((order, title, links), (want_order, want_title, want_links)) in loaded.iter().zip(&wanted)
    {
        assert_eq!((order, title), (want_order, want_title), "{brief}");
        assert!(links.iter().all(|l| want_links.contains(l)), "{brief}");
    }
    let missing = brief["result"]["missing"].as_array().expect("a list");
    let missing: BTreeSet<&str> = missing.iter().filter_map(|m| m["id"].as_str()).collect();
    assert_eq!(
        missing,
        BTreeSet::from(["tc-odric", "tc-gue"]),
        "the created entry is no longer missing: {brief}"
    );

    // The scenes of the session being prepared, with what each links, by rank.
    let scenes = read(&renamed, "session.listScenes", json!({ "session": "s4" })).await;
    let described = fixture("session.listScenes.json");
    let (scenes, described) = (
        scenes.as_array().expect("a list"),
        described.as_array().unwrap(),
    );
    assert_eq!(scenes.len(), 3, "{scenes:?}");
    let sorted = |list: &Value| -> Vec<String> {
        let mut ids: Vec<String> = list
            .as_array()
            .expect("a list")
            .iter()
            .map(Value::to_string)
            .collect();
        ids.sort();
        ids
    };
    for (got, want) in scenes.iter().zip(described) {
        for key in ["id", "order", "title", "notes"] {
            assert_eq!(got[key], want[key], "{}.{key}: {got}", want["id"]);
        }
        for key in ["places", "characters", "events"] {
            assert_eq!(got[key], want[key], "{}.{key}: {got}", want["id"]);
        }
    }
    // A medium is listed once linked to the scene or once at one of its places: the
    // fixture's own media, and, for the second scene, the cover illustration of Valombre.
    for (got, want) in scenes.iter().zip(described) {
        let (got, want) = (sorted(&got["media"]), sorted(&want["media"]));
        assert!(
            want.iter().all(|m| got.contains(m)),
            "{got:?} lacks {want:?}"
        );
    }
    assert_eq!(sorted(&scenes[0]["media"]), sorted(&described[0]["media"]));
    assert_eq!(sorted(&scenes[2]["media"]), sorted(&described[2]["media"]));
    assert_eq!(
        sorted(&scenes[1]["media"]),
        sorted(&json!(["ill-moulin", "ill-cover"])),
        "the cover is at Valombre, a place of the scene"
    );

    // The scene of the ambush has its battlemap instance, the others none.
    let battlemaps: Vec<&Value> = scenes.iter().map(|s| &s["battlemap"]).collect();
    assert_eq!(
        battlemaps,
        [&json!(null), &json!(null), &json!("bmi-gue-s4")],
        "{scenes:?}"
    );
    // The instance, with its tokens at their hexes, as the DM sees it.
    let instance = fixture("table.getBattlemapInstance.json");
    let read_instance = |view: &'static str| {
        let renamed = &renamed;
        async move {
            let variables = json!({ "instance": "bmi-gue-s4", "view": view });
            read(renamed, "table.getBattlemapInstance", variables).await
        }
    };
    let dm = read_instance("dm").await;
    for key in ["id", "battlemap", "session", "scene"] {
        assert_eq!(dm[key], instance[key], "{key}: {dm}");
    }
    assert_eq!(dm["state"]["combat"], json!(null), "{dm}");
    let tokens = |instance: &Value| -> BTreeMap<String, Value> {
        instance["state"]["tokens"]
            .as_array()
            .expect("tokens")
            .iter()
            .map(|t| (t["id"].as_str().expect("an id").to_string(), t.clone()))
            .collect()
    };
    let (placed, described) = (tokens(&dm), tokens(&instance));
    assert_eq!(placed.len(), 8, "{dm}");
    assert_eq!(
        placed.keys().collect::<Vec<_>>(),
        described.keys().collect::<Vec<_>>(),
        "{dm}"
    );
    for (slug, want) in &described {
        let got = &placed[slug];
        for key in [
            "char", "kind", "name", "num", "q", "r", "visible", "hp", "hpMax", "states", "dead",
        ] {
            assert_eq!(got[key], want[key], "{slug}.{key}: {got}");
        }
    }
    // The players do not see the hidden chief, nor a hit point.
    let players = read_instance("player").await;
    let seen = tokens(&players);
    assert_eq!(seen.len(), 7, "{players}");
    assert!(!seen.contains_key("t-vesna-1"), "{players}");
    assert!(seen.values().all(|t| t.get("hp").is_none()), "{players}");
    assert_eq!(seen["t-bandit-1"]["status"], "indemne", "{players}");

    // The closed session's journal and summary.
    let journal = read(
        &renamed,
        "session.getSessionJournal",
        json!({ "session": "s3" }),
    )
    .await;
    let described = fixture("session.getSessionJournal.json");
    assert_eq!(
        journal["journal"].as_array().map(Vec::len),
        Some(14),
        "{journal}"
    );
    // `journalStats` is a figure of the summary, not of the journal: another read.
    assert_eq!(
        journal["journal"], described["journal"],
        "in order: {journal}"
    );
    let previous = read(
        &renamed,
        "session.getPreviousSessionSummary",
        json!({ "session": "s4" }),
    )
    .await;
    let described = fixture("session.getPreviousSessionSummary.json");
    assert_eq!(previous["session"], described["session"], "{previous}");
    assert_eq!(previous["summary"], described["summary"], "{previous}");
    assert_eq!(
        previous["summaryMeta"], described["summaryMeta"],
        "{previous}"
    );

    // Without the names, a slug names no row: the store knows uuids only.
    assert_eq!(
        read(&served, "campaign.listCharacters", campaign).await,
        json!([])
    );

    drop((renamed, served, client));
    admin
        .batch_execute(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .await
        .expect("the scratch database dropped");
}

// AC3: a scene the store gave another instance loads neither the fixture's instance nor its tokens, combat or seats
#[tokio::test]
async fn an_instance_the_dm_made_on_a_scene_is_left_alone_by_the_load() {
    let name = format!("fixture_dm_instance_{}", std::process::id());
    let admin = connect(&url()).await;
    for sql in [
        format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
        format!("CREATE DATABASE {name}"),
    ] {
        admin.batch_execute(&sql).await.expect("a scratch database");
    }
    let mut client = connect(&named(&name)).await;
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/migrations");
    data::migrate(&mut client, &migrations)
        .await
        .expect("migrated");
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures");
    let (scene, fixture_instance) = (
        uuid_of("scene", "sc3"),
        uuid_of("battlemap_instance", "bmi-gue-s4"),
    );
    // The store holds the fixture's instance with its tokens, as a first load left it.
    let first = fixtures(&folder).expect("the fixtures");
    data::insert_missing(&mut client, &first.tables)
        .await
        .expect("the fixtures are inserted");
    let seated: i64 = client
        .query_one(
            "SELECT (SELECT count(*) FROM combat) + (SELECT count(*) FROM initiative_entry)",
            &[],
        )
        .await
        .expect("a count")
        .get(0);
    assert_eq!(
        seated,
        1 + 4,
        "the first load left its combat and its four seats"
    );
    // The DM replaced it: the fixture's instance is gone, theirs is on the scene.
    let dm = "d0d0d0d0-0000-4000-8000-000000000001";
    client
        .batch_execute(&format!(
            "DELETE FROM battlemap_instance WHERE id = '{fixture_instance}'; \
             INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) \
             VALUES ('{dm}', '{}', '{}', '{scene}')",
            uuid_of("media", "bm-gue"),
            uuid_of("session", "s4"),
        ))
        .await
        .expect("the DM's instance");
    // The restart plans the fixture's instance again, and the load leaves it out.
    let mut again = fixtures(&folder).expect("the fixtures");
    let held = held_instances(&client).await.expect("the instances held");
    assert_eq!(held, vec![(dm.to_string(), scene.clone())]);
    again.leave_out_taken_scenes(&held);
    assert_eq!(
        data::insert_missing(&mut client, &again.tables)
            .await
            .expect("the rest of the fixtures are planned again"),
        0
    );
    let row = client
        .query_one(
            &format!(
                "SELECT (SELECT count(*) FROM battlemap_instance WHERE scene_id = '{scene}'), \
                 (SELECT count(*) FROM token), (SELECT count(*) FROM combat), \
                 (SELECT count(*) FROM initiative_entry)"
            ),
            &[],
        )
        .await
        .expect("a count");
    assert_eq!(row.get::<_, i64>(0), 1, "one instance on the scene");
    assert_eq!(row.get::<_, i64>(1), 0, "no token of the fixture's");
    assert_eq!(row.get::<_, i64>(2), 0, "no combat of the fixture's");
    assert_eq!(row.get::<_, i64>(3), 0, "no seat of the fixture's");
    drop(client);
    admin
        .batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .await
        .expect("the scratch database is dropped");
}

#[tokio::test]
async fn an_element_the_app_already_versioned_keeps_its_versions_through_the_load() {
    let name = format!("fixture_app_versions_{}", std::process::id());
    let admin = connect(&url()).await;
    for sql in [
        format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
        format!("CREATE DATABASE {name}"),
    ] {
        admin.batch_execute(&sql).await.expect("a scratch database");
    }
    let mut client = connect(&named(&name)).await;
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/migrations");
    data::migrate(&mut client, &migrations)
        .await
        .expect("migrated");
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures");
    let (morlox, elowen) = (uuid_of("entity", "morlox"), uuid_of("entity", "elowen"));
    // The app wrote Morlox: its first version is under a random id, and it has no other.
    let mut first = fixtures(&folder).expect("the fixtures");
    for table in first
        .tables
        .iter_mut()
        .filter(|t| t.name == "entity_version")
    {
        table
            .rows
            .retain(|row| row["entity_id"] != json!(morlox) || row["version_number"] == json!(1));
        for row in &mut table.rows {
            if row["entity_id"] == json!(morlox) {
                row["id"] = json!("a99a0000-0000-4000-8000-000000000001");
            }
        }
    }
    data::insert_missing(&mut client, &first.tables)
        .await
        .expect("the store as the app left it");
    // The restart plans Morlox's four versions: the second would be refused by the
    // version trigger, and with it the whole load.
    let unguarded = fixtures(&folder).expect("the fixtures");
    assert!(
        data::insert_missing(&mut client, &unguarded.tables)
            .await
            .is_err(),
        "the trigger refuses a second version beside the app's first"
    );
    let mut again = fixtures(&folder).expect("the fixtures");
    let held = held_versions(&client).await.expect("the versions held");
    again.leave_out_taken_versions(&held);
    data::insert_missing(&mut client, &again.tables)
        .await
        .expect("the load goes through, Morlox left alone");
    let count = |entity: String| {
        let client = &client;
        async move {
            client
                .query_one(
                    &format!("SELECT count(*) FROM entity_version WHERE entity_id = '{entity}'"),
                    &[],
                )
                .await
                .expect("a count")
                .get::<_, i64>(0)
        }
    };
    assert_eq!(count(morlox).await, 1, "the app's version alone");
    assert_eq!(count(elowen).await, 3, "the history of the others");
    drop(client);
    admin
        .batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .await
        .expect("the scratch database is dropped");
}
