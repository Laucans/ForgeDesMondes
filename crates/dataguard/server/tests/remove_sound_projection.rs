//! soundbox.removeSound over PostgreSQL (`DATABASE_URL`): a removal applied
//! on `Sound/<id>` is the engine's tombstone, the sound kept there as it
//! was, and takes the sound's row out of the `sound` table — so
//! soundbox.listSounds no longer lists it — its playbacks with it
//! (`SoundPlayback.soundId`, onDelete cascade); a sound removed is none to
//! remove again; one the table still holds after its removal is taken out
//! at the next start. Each test works in a scratch database of its own,
//! migrated by the Data layer, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, Reads, planted, router};
use harness_dataguard::command::{CommandResult, State};
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const LIVE: &str = "5e000000-0000-4000-8000-000000000001";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(url: &str) -> Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// `DATABASE_URL` naming the database `name` instead.
fn naming(name: &str) -> String {
    let url = url();
    let (address, query) = url.split_once('?').unwrap_or((&url, ""));
    let (server, _) = address.rsplit_once('/').expect("a URL naming its database");
    let query = if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    };
    format!("{server}/{name}{query}")
}

/// A database of its own, migrated by the Data layer, holding Valombre and
/// its live session.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("remove_sound_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let mut client = connect(&url).await;
        let migrations = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/migrations");
        data::migrate(&mut client, Path::new(migrations))
            .await
            .expect("migrated by the Data layer");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) \
                 VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'); \
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) \
                 VALUES ('{LIVE}', '{VALOMBRE}', 1, 'Le Sanglier', 'live', false);"
            ))
            .await
            .expect("Valombre and its live session");
        Scratch { name, url, client }
    }

    /// The engine as the server plants it over this database, and its wire
    /// protocol.
    async fn serve(&self) -> (Arc<Engine>, Router) {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let engine = Arc::new(engine);
        let app = router(
            engine.clone(),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        (engine, app)
    }

    /// The names of the sounds soundbox.listSounds lists for Valombre,
    /// sorted.
    async fn listed(&self) -> Vec<String> {
        let reads = Reads::open(&self.url).await.expect("the reads open");
        let answer = reads
            .run("soundbox.listSounds", &json!({ "campaign": VALOMBRE }))
            .await
            .expect("an answer");
        let names = answer["sounds"]
            .as_array()
            .unwrap_or_else(|| panic!("sounds: {answer}"))
            .iter()
            .map(|sound| sound["name"].as_str().expect("a name").to_owned());
        let mut names: Vec<String> = names.collect();
        names.sort();
        names
    }

    /// How many rows of `table` name the sound `id`.
    async fn rows_of(&self, table: &str, column: &str, id: &str) -> i64 {
        self.client
            .query_one(
                &format!("SELECT count(*) FROM {table} WHERE {column} = $1::text::uuid"),
                &[&id],
            )
            .await
            .expect("counted")
            .get(0)
    }

    /// The engine's row of `Sound/<id>`: whether it is a tombstone, and the
    /// sound it keeps.
    async fn engine_row(&self, id: &str) -> (bool, Value) {
        let row = self
            .client
            .query_one(
                "SELECT tombstoned, fields::text FROM dataguard_rows WHERE partition = $1",
                &[&format!("Sound/{id}")],
            )
            .await
            .expect("the engine's row");
        (
            row.get(0),
            serde_json::from_str(row.get::<_, &str>(1)).expect("JSON"),
        )
    }

    async fn drop(self) {
        drop(self.client);
        connect(&url())
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
    }
}

/// `POST /commands` of `data_capability` by the DM's library, approver of
/// their own sounds' critical fields: the status, and the answer.
async fn post(app: &Router, data_capability: &str, mut body: Value) -> (StatusCode, Value) {
    body["dataCapability"] = json!(data_capability);
    body["caller"] = json!({ "by": "dm", "from": "SoundLibrary", "roles": ["role:approver"] });
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request");
    let response = app.clone().oneshot(request).await.expect("an answer");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, serde_json::from_slice(&bytes).expect("JSON"))
}

/// [`post`], accepted, and what the applier made of it.
async fn command(
    engine: &Engine,
    app: &Router,
    data_capability: &str,
    body: Value,
) -> (Value, CommandResult) {
    let (status, answer) = post(app, data_capability, body).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    (answer, settled.remove(0))
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// The sound `name` added by soundbox.addSound and applied: its id.
async fn add(engine: &Engine, app: &Router, name: &str) -> String {
    let (answer, result) = command(
        engine,
        app,
        "soundbox.addSound@1",
        json!({ "payload": {
            "campaign": VALOMBRE,
            "name": name,
            "tags": ["bois"],
            "volume": 80,
            "file": "sons/porte.ogg",
        } }),
    )
    .await;
    applied(&result);
    answer["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("Sound/"))
        .expect("a Sound partition")
        .to_owned()
}

/// The sound `id` removed by the DM, the wire carrying `payload`: what the
/// applier made of it.
async fn remove(engine: &Engine, app: &Router, id: &str, payload: Value) -> CommandResult {
    command(
        engine,
        app,
        "soundbox.removeSound@1",
        json!({ "targetId": id, "payload": payload }),
    )
    .await
    .1
}

#[tokio::test]
async fn a_sound_removed_leaves_the_library_s_readers_and_its_tombstone_keeps_it_as_it_was() {
    let scratch = Scratch::new("removed").await;
    let (engine, app) = scratch.serve().await;
    let door = add(&engine, &app, "Porte").await;
    add(&engine, &app, "Foule").await;
    assert_eq!(scratch.listed().await, ["Foule", "Porte"]);
    let (_, played) = command(
        &engine,
        &app,
        "soundbox.playSound@1",
        json!({ "target": { "id": door }, "payload": {} }),
    )
    .await;
    applied(&played);
    assert_eq!(
        scratch.rows_of("sound_playback", "sound_id", &door).await,
        1
    );

    // What the wire carries besides the sound is not written.
    let result = remove(&engine, &app, &door, json!({ "name": "Effacée" })).await;
    applied(&result);

    assert_eq!(scratch.listed().await, ["Foule"]);
    assert_eq!(scratch.rows_of("sound", "id", &door).await, 0);
    // Its playbacks follow it: SoundPlayback.soundId, onDelete cascade.
    assert_eq!(
        scratch.rows_of("sound_playback", "sound_id", &door).await,
        0
    );
    let (tombstoned, kept) = scratch.engine_row(&door).await;
    assert!(tombstoned, "{kept}");
    assert_eq!(kept["name"], "Porte", "{kept}");
    assert_eq!(kept["filePath"], "sons/porte.ogg", "{kept}");
    assert_eq!(kept["tags"], json!(["bois"]), "{kept}");
    assert_eq!(kept["campaignId"], VALOMBRE, "{kept}");
    scratch.drop().await;
}

#[tokio::test]
async fn a_sound_removed_is_none_to_remove_or_edit_again() {
    let scratch = Scratch::new("again").await;
    let (engine, app) = scratch.serve().await;
    let door = add(&engine, &app, "Porte").await;
    applied(&remove(&engine, &app, &door, json!({})).await);
    for (data_capability, payload) in [
        ("soundbox.removeSound@1", json!({})),
        (
            "soundbox.updateSound@1",
            json!({ "name": "Porte qui grince" }),
        ),
    ] {
        let (status, answer) = post(
            &app,
            data_capability,
            json!({ "targetId": door, "payload": payload }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{data_capability}: {answer}");
        assert_eq!(answer["error"], "ce son n'existe pas");
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert!(scratch.engine_row(&door).await.0);
    scratch.drop().await;
}

#[tokio::test]
async fn a_removal_the_table_lost_is_made_again_at_the_next_start() {
    let scratch = Scratch::new("repair").await;
    let (engine, app) = scratch.serve().await;
    let door = add(&engine, &app, "Porte").await;
    add(&engine, &app, "Foule").await;
    applied(&remove(&engine, &app, &door, json!({})).await);
    // The removal lost: the door's row back in the table.
    scratch
        .client
        .batch_execute(&format!(
            "INSERT INTO sound (id, campaign_id, name, file_path) \
             VALUES ('{door}', '{VALOMBRE}', 'Porte', 'sons/porte.ogg')"
        ))
        .await
        .expect("the removal lost");
    assert_eq!(scratch.listed().await, ["Foule", "Porte"]);
    drop((engine, app));
    let _restarted = scratch.serve().await;
    assert_eq!(scratch.listed().await, ["Foule"]);
    assert!(scratch.engine_row(&door).await.0);
    scratch.drop().await;
}
