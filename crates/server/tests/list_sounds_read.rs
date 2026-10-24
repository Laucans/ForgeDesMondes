//! soundbox.listSounds over PostgreSQL (`DATABASE_URL`): the persisted query
//! of the Capability runs against the tables the migrations build, and the
//! answer carries each sound's own volume as a percent, its effective gain
//! under the campaign's master volume (EffectiveVolume@1), whether a
//! playback of it runs, and honours the name search and the tag filter. It is
//! the registry's, served by the engine's reads, through the connection
//! `Reads` opens. The rows go in through the Data layer, in a scratch database
//! of its own, dropped when the test ends.

mod common;

use std::path::Path;

use common::Reads;
use data::Table;
use dataguard_server::engine::tokio_postgres::{self, NoTls};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const EMPTY: &str = "c1000000-0000-4000-8000-000000000002";
const SESSION: &str = "51000000-0000-4000-8000-000000000001";
const CLOSED: &str = "51000000-0000-4000-8000-000000000002";
const CROWD: &str = "50000000-0000-4000-8000-000000000001";
const DOOR: &str = "50000000-0000-4000-8000-000000000002";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

/// `url()` naming database `name`.
fn named(name: &str) -> String {
    let url = url();
    let (base, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let (server, _) = base.rsplit_once('/').expect("a database in the URL");
    if query.is_empty() {
        format!("{server}/{name}")
    } else {
        format!("{server}/{name}?{query}")
    }
}

async fn connect(url: &str) -> tokio_postgres::Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

fn table(
    name: &'static str,
    key: &'static str,
    columns: &'static [&'static str],
    rows: &Value,
) -> Table {
    Table {
        name,
        key,
        columns,
        rows: rows.as_array().expect("rows").clone(),
    }
}

/// A campaign under a master of 0.8 with a crowd at 0.55 and a door at 0.8,
/// a running playback of the crowd and a stopped one of the door; a campaign
/// with no sound under the default master.
fn rows() -> Vec<Table> {
    vec![
        table(
            "campaign",
            "id",
            &["id", "name", "kind", "sound_master_volume"],
            &json!([
                { "id": VALOMBRE, "name": "Valombre", "kind": "campagne", "sound_master_volume": 0.8 },
                { "id": EMPTY, "name": "Vide", "kind": "oneshot", "sound_master_volume": 0.6 },
            ]),
        ),
        table(
            "sound",
            "id",
            &[
                "id",
                "campaign_id",
                "name",
                "file_path",
                "tags",
                "volume",
                "created_at",
            ],
            &json!([
                { "id": CROWD, "campaign_id": VALOMBRE, "name": "Bruit de foule en taverne",
                  "file_path": "foule.ogg", "tags": ["foule", "taverne"], "volume": 0.55,
                  "created_at": "2026-10-01T10:00:00Z" },
                { "id": DOOR, "campaign_id": VALOMBRE, "name": "Porte qui grince",
                  "file_path": "porte.ogg", "tags": ["porte"], "volume": 0.8,
                  "created_at": "2026-10-01T10:01:00Z" },
            ]),
        ),
        table(
            "session",
            "id",
            &[
                "id",
                "campaign_id",
                "position",
                "title",
                "status",
                "microphone_on",
            ],
            &json!([
                { "id": SESSION, "campaign_id": VALOMBRE, "position": 1,
                  "title": "Soir 1", "status": "live", "microphone_on": false },
                { "id": CLOSED, "campaign_id": VALOMBRE, "position": 2,
                  "title": "Soir 0", "status": "closed", "microphone_on": false },
            ]),
        ),
        table(
            "sound_playback",
            "id",
            &[
                "id",
                "session_id",
                "sound_id",
                "origin",
                "started_at",
                "stopped_at",
            ],
            &json!([
                { "id": "52000000-0000-4000-8000-000000000001", "session_id": SESSION,
                  "sound_id": CROWD, "origin": "soundbox", "started_at": "2026-10-01T11:00:00Z",
                  "stopped_at": null },
                { "id": "52000000-0000-4000-8000-000000000002", "session_id": SESSION,
                  "sound_id": DOOR, "origin": "pastille", "started_at": "2026-10-01T11:00:00Z",
                  "stopped_at": "2026-10-01T11:00:05Z" },
                // A session that ended with a playback left open: nothing plays.
                { "id": "52000000-0000-4000-8000-000000000003", "session_id": CLOSED,
                  "sound_id": DOOR, "origin": "soundbox", "started_at": "2026-09-01T11:00:00Z",
                  "stopped_at": null },
            ]),
        ),
    ]
}

#[tokio::test]
async fn the_query_answers_volumes_as_percents_gains_playing_and_filters() {
    let name = format!("list_sounds_read_{}", std::process::id());
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
    data::insert_missing(&mut client, &rows())
        .await
        .expect("the rows are inserted");

    let reads = Reads::open(&named(&name)).await.expect("the reads open");
    let run = |variables: Value| {
        let reads = &reads;
        async move {
            reads
                .run("soundbox.listSounds", &variables)
                .await
                .expect("an answer")
        }
    };

    let all = run(json!({ "campaign": VALOMBRE })).await;
    assert_eq!(all["masterVolume"], 80, "{all}");
    assert_eq!(
        all["sounds"],
        json!([
            { "id": CROWD, "name": "Bruit de foule en taverne", "tags": ["foule", "taverne"],
              "volume": 55, "file": "foule.ogg", "gain": 0.44, "playing": true },
            { "id": DOOR, "name": "Porte qui grince", "tags": ["porte"],
              "volume": 80, "file": "porte.ogg", "gain": 0.64, "playing": false },
        ]),
        "a running playback plays, a stopped one or one of a closed session does not"
    );

    let tagged = run(json!({ "campaign": VALOMBRE, "tag": "Taverne" })).await;
    assert_eq!(tagged["sounds"][0]["id"], CROWD, "{tagged}");
    assert_eq!(tagged["sounds"].as_array().map(Vec::len), Some(1));
    let searched = run(json!({ "campaign": VALOMBRE, "search": "porte" })).await;
    assert_eq!(searched["sounds"][0]["id"], DOOR, "{searched}");
    assert_eq!(searched["sounds"].as_array().map(Vec::len), Some(1));

    let empty = run(json!({ "campaign": EMPTY })).await;
    assert_eq!(empty, json!({ "masterVolume": 60, "sounds": [] }));
    let unknown = run(json!({ "campaign": "nobody" })).await;
    assert_eq!(
        unknown,
        Value::Null,
        "an id no campaign holds has no answer"
    );
    let unsaid = run(json!({})).await;
    assert_eq!(unsaid, Value::Null, "no campaign given, none read");

    drop(reads);
    drop(client);
    admin
        .batch_execute(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .await
        .expect("the scratch database dropped");
}
