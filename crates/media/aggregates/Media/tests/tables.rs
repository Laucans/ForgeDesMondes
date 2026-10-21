//! The Media aggregate's tables over PostgreSQL (`DATABASE_URL`), as the
//! server's store runs its projection: what the engine applies on
//! `Media/<id>` reaches `media` and `battlemap_grid` in the apply
//! transaction, and what they refuse rejects the command whole; a start
//! writes again what the tables lost, each media alone; a media only the
//! tables hold is the engine's row; a place is read from the tables. The
//! DataCapabilities here are test-only: each write is the row as given.
//! Each test works in a scratch database of its own, migrated as the server
//! migrates it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::Plugins;
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::store::Store;
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use serde_json::{Value, json};

/// A media written whole, test-only: an upload's row.
const ADD: &str = r#"{"dataCapability":"test.addMedia","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Media","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// Fields of a media changed, test-only: a grid saved, a place changed.
const UPDATE: &str = r#"{"dataCapability":"test.updateMedia","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Media","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A media deleted: the engine's tombstone.
const DELETE: &str = r#"{"dataCapability":"test.deleteMedia","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Media","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
const PHARE: &str = "e1000000-0000-4000-8000-000000000004";
const MOULIN: &str = "e1000000-0000-4000-8000-000000000005";
/// Fixtures' media: a battlemap with its grid, an illustration at the Taverne.
const GUE: &str = "a1000000-0000-4000-8000-000000000001";
const SALLE: &str = "a1000000-0000-4000-8000-000000000002";
/// Media the tests upload.
const CARTE: &str = "a1000000-0000-4000-8000-00000000000a";
const PORTRAIT: &str = "a1000000-0000-4000-8000-00000000000b";

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

/// A database of its own, migrated as the server migrates it — unless
/// `bare` — holding, as the fixtures write them: Valombre with the Taverne
/// and the Moulin (lieux) and Morlox (an NPC), Brume with its lighthouse (a
/// lieu), Valombre's battlemap of the ford with its grid and no place, and
/// its illustration of the tavern's hall at the Taverne.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        Self::created(name, false).await
    }

    async fn created(name: &str, bare: bool) -> Scratch {
        let name = format!("media_tables_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let mut client = connect(&url).await;
        if !bare {
            let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
            data::migrate(&mut client, &migrations)
                .await
                .expect("migrated");
            let versions: String = [TAVERNE, MORLOX, PHARE, MOULIN]
                .iter()
                .map(|element| {
                    format!(
                        "INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
                         VALUES ('{element}', 1, 'dm', '{{}}', '[]');"
                    )
                })
                .collect();
            client
                .batch_execute(&format!(
                    "INSERT INTO campaign (id, name, kind) VALUES
                         ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                         ('{BRUME}', 'La Brume', 'oneshot');
                     INSERT INTO entity (id, campaign_id, type, name) VALUES
                         ('{TAVERNE}', '{VALOMBRE}', 'lieu', 'La Taverne du Sanglier'),
                         ('{MORLOX}', '{VALOMBRE}', 'pnj', 'Morlox'),
                         ('{PHARE}', '{BRUME}', 'lieu', 'Le Phare'),
                         ('{MOULIN}', '{VALOMBRE}', 'lieu', 'Le Moulin');
                     {versions}
                     INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id) VALUES
                         ('{GUE}', '{VALOMBRE}', 'battlemap', 'Le Gué', 'media/gue.png', 960, 640, NULL),
                         ('{SALLE}', '{VALOMBRE}', 'illustration', 'La salle', 'media/salle.png', 800, 600, '{TAVERNE}');
                     INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
                         VALUES ('{GUE}', 30, 12, 8, 'pointe', '#202020', 0.4);"
                ))
                .await
                .expect("Valombre, Brume and their media");
        }
        Scratch { name, url, client }
    }

    /// The store as the server opens it over this database, the aggregate's
    /// projection warmed.
    async fn store(&self) -> Arc<PgStore> {
        let plugins: Plugins = std::iter::once(aggregate_media::plugin()).collect();
        Arc::new(
            PgStore::open(connect(&self.url).await)
                .await
                .expect("the store")
                .with_projections(&plugins)
                .await
                .expect("warmed"),
        )
    }

    /// The engine over [`Self::store`], with the aggregate's plugin and the
    /// test-only DataCapabilities.
    async fn serve(&self) -> Engine {
        let mut engine = Engine::new(
            self.store().await,
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_aggregate(Aggregate::parse(aggregate_media::DOCUMENT).expect("contract I"))
        .with_plugins(std::iter::once(aggregate_media::plugin()).collect());
        for manifest in [ADD, UPDATE, DELETE] {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        engine.load().await.expect("the engine loads");
        engine
    }

    /// The rows `sql` selects, each as JSON.
    async fn rows(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(&format!("SELECT to_jsonb(q)::text FROM ({sql}) q"), &[])
            .await
            .expect("the tables")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
    }

    /// `media`'s row and its grid, as the tables hold them.
    async fn media(&self, media: &str) -> Vec<Value> {
        self.rows(&format!(
            "SELECT m.kind, m.title, m.place_id, g.hex_size_px::float8 AS size, g.orientation, \
             g.opacity::float8 AS opacity, g.feet_per_hex \
             FROM media m LEFT JOIN battlemap_grid g ON g.media_id = m.id WHERE m.id = '{media}'"
        ))
        .await
    }

    /// `media`'s place as its row holds it: `None` for none.
    async fn place_of(&self, media: &str) -> Option<String> {
        self.client
            .query_one(
                "SELECT place_id::text FROM media WHERE id = ($1::text)::uuid",
                &[&media],
            )
            .await
            .expect("the media")
            .get(0)
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

/// A command of `data_capability` writing `payload` on `media`, applied:
/// what the applier made of it.
async fn written(
    engine: &Engine,
    data_capability: &str,
    media: &str,
    payload: Value,
) -> Result<CommandResult, Error> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": data_capability,
        "targetId": media,
        "payload": payload,
        "caller": { "by": "dm", "from": "test", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await.expect("queued");
    engine.run_once().await.map(|mut settled| settled.remove(0))
}

/// The same, which the applier applied or rejected.
async fn settled(
    engine: &Engine,
    data_capability: &str,
    media: &str,
    payload: Value,
) -> CommandResult {
    written(engine, data_capability, media, payload)
        .await
        .expect("the applier ran")
}

/// An upload's row on `id`: a battlemap with its grid, or an illustration.
fn uploaded(id: &str, kind: &str, place: Option<&str>) -> Value {
    let mut row = json!({
        "id": id, "campaignId": VALOMBRE, "kind": kind, "title": "La Taverne",
        "filePath": "media/taverne.png", "widthPx": 2048, "heightPx": 1536,
        "placeId": place, "uploadedAt": "2026-10-21T09:00:00.123Z",
    });
    if kind == "battlemap" {
        for (field, value) in [
            ("mediaId", json!(id)),
            ("hexSizePx", json!(32)),
            ("offsetX", json!(0)),
            ("offsetY", json!(0)),
            ("orientation", json!("pointe")),
            ("color", json!("text")),
            ("opacity", json!(0.45)),
        ] {
            row[field] = value;
        }
    }
    row
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

#[tokio::test]
async fn an_upload_reaches_media_and_a_battlemap_its_grid_at_five_feet_a_hex() {
    let s = Scratch::new("upload").await;
    let engine = s.serve().await;
    applied(
        &settled(
            &engine,
            "test.addMedia",
            CARTE,
            uploaded(CARTE, "battlemap", Some(TAVERNE)),
        )
        .await,
    );
    applied(
        &settled(
            &engine,
            "test.addMedia",
            PORTRAIT,
            uploaded(PORTRAIT, "illustration", None),
        )
        .await,
    );
    assert_eq!(
        s.media(CARTE).await,
        [json!({
            "kind": "battlemap", "title": "La Taverne", "place_id": TAVERNE, "size": 32,
            "orientation": "pointe", "opacity": 0.45, "feet_per_hex": 5,
        })]
    );
    assert_eq!(
        s.media(PORTRAIT).await,
        [json!({
            "kind": "illustration", "title": "La Taverne", "place_id": null, "size": null,
            "orientation": null, "opacity": null, "feet_per_hex": null,
        })]
    );
    let stamped = s
        .rows(&format!(
            "SELECT uploaded_at = '2026-10-21T09:00:00.123Z' AS stamped FROM media WHERE id = '{CARTE}'"
        ))
        .await;
    assert_eq!(stamped, [json!({ "stamped": true })]);
    s.drop().await;
}

#[tokio::test]
async fn a_grid_saved_and_a_place_changed_reach_their_columns_alone() {
    let s = Scratch::new("update").await;
    let engine = s.serve().await;
    // The ford's battlemap was written by no command: the tables alone hold it.
    let grid = json!({
        "hexSizePx": 48, "offsetX": 30, "offsetY": 25, "orientation": "plat",
        "color": "#c8a24b", "opacity": 0.2,
    });
    applied(&settled(&engine, "test.updateMedia", GUE, grid).await);
    applied(
        &settled(
            &engine,
            "test.updateMedia",
            GUE,
            json!({ "placeId": MOULIN }),
        )
        .await,
    );
    assert_eq!(
        s.media(GUE).await,
        [json!({
            "kind": "battlemap", "title": "Le Gué", "place_id": MOULIN, "size": 48,
            "orientation": "plat", "opacity": 0.2, "feet_per_hex": 5,
        })]
    );
    // Detached.
    applied(
        &settled(
            &engine,
            "test.updateMedia",
            SALLE,
            json!({ "placeId": null }),
        )
        .await,
    );
    assert_eq!(s.place_of(SALLE).await, None);
    s.drop().await;
}

#[tokio::test]
async fn what_the_tables_refuse_rejects_the_command_and_writes_nothing_of_it() {
    let s = Scratch::new("refused").await;
    let engine = s.serve().await;
    // media-place-is-lieu: the table's deferred trigger, judged in the apply
    // transaction rather than at its commit.
    let result = settled(
        &engine,
        "test.addMedia",
        CARTE,
        uploaded(CARTE, "battlemap", Some(MORLOX)),
    )
    .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    // A campaign the tables do not hold: its foreign key.
    let mut orphan = uploaded(PORTRAIT, "illustration", None);
    orphan["campaignId"] = json!("c1000000-0000-4000-8000-0000000000ff");
    let result = settled(&engine, "test.addMedia", PORTRAIT, orphan).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    for media in [CARTE, PORTRAIT] {
        assert!(s.media(media).await.is_empty(), "{media}");
    }
    let engine_rows = s
        .rows("SELECT partition FROM dataguard_rows WHERE partition LIKE 'Media/%'")
        .await;
    assert!(engine_rows.is_empty(), "{engine_rows:?}");
    s.drop().await;
}

#[tokio::test]
async fn a_projection_that_cannot_run_fails_the_step_and_leaves_the_command_queued() {
    let s = Scratch::new("failed").await;
    let engine = s.serve().await;
    let mut row = uploaded(PORTRAIT, "illustration", None);
    // No uuid: the table cannot even read it.
    row["campaignId"] = json!("valombre");
    let failed = written(&engine, "test.addMedia", PORTRAIT, row).await;
    assert!(
        matches!(&failed, Err(Error::Store(message)) if message.contains("uuid")),
        "{failed:?}"
    );
    assert!(s.media(PORTRAIT).await.is_empty());
    s.drop().await;
}

#[tokio::test]
async fn a_tombstone_writes_nothing_of_the_tables() {
    let s = Scratch::new("tombstone").await;
    let engine = s.serve().await;
    let before = s.media(GUE).await;
    applied(&settled(&engine, "test.deleteMedia", GUE, json!({})).await);
    assert_eq!(s.media(GUE).await, before);
    s.drop().await;
}

#[tokio::test]
async fn another_write_of_the_media_never_writes_its_place_again() {
    let s = Scratch::new("other_write").await;
    let engine = s.serve().await;
    applied(
        &settled(
            &engine,
            "test.updateMedia",
            GUE,
            json!({ "placeId": TAVERNE }),
        )
        .await,
    );
    // The place is removed behind the engine's back: `ON DELETE SET NULL`
    // leaves the media without it, and the engine's row keeps it.
    s.client
        .batch_execute(&format!("DELETE FROM entity WHERE id = '{TAVERNE}'"))
        .await
        .expect("the place removed");
    assert_eq!(s.place_of(GUE).await, None);
    applied(
        &settled(
            &engine,
            "test.updateMedia",
            GUE,
            json!({ "title": "Le Gué des Corbeaux" }),
        )
        .await,
    );
    assert_eq!(s.place_of(GUE).await, None);
    s.drop().await;
}

#[tokio::test]
async fn a_start_writes_again_the_media_the_grid_and_the_place_the_tables_lost() {
    let s = Scratch::new("repair").await;
    {
        let engine = s.serve().await;
        applied(
            &settled(
                &engine,
                "test.addMedia",
                CARTE,
                uploaded(CARTE, "battlemap", None),
            )
            .await,
        );
        let grid = json!({
            "hexSizePx": 48, "offsetX": 30, "offsetY": 25, "orientation": "plat",
            "color": "#c8a24b", "opacity": 0.2,
        });
        applied(&settled(&engine, "test.updateMedia", GUE, grid).await);
        applied(
            &settled(
                &engine,
                "test.updateMedia",
                GUE,
                json!({ "placeId": TAVERNE }),
            )
            .await,
        );
        applied(
            &settled(
                &engine,
                "test.updateMedia",
                SALLE,
                json!({ "placeId": null }),
            )
            .await,
        );
    }
    // Lost after their apply, as a server before this one could lose them:
    // the upload whole (its grid goes with it), the saved grid, both places.
    s.client
        .batch_execute(&format!(
            "DELETE FROM media WHERE id = '{CARTE}';
             UPDATE battlemap_grid SET hex_size_px = 64, orientation = 'pointe' WHERE media_id = '{GUE}';
             UPDATE media SET place_id = NULL WHERE id = '{GUE}';
             UPDATE media SET place_id = '{TAVERNE}' WHERE id = '{SALLE}'"
        ))
        .await
        .expect("the projections lost");
    let _restarted = s.store().await;
    assert_eq!(
        s.media(CARTE).await,
        [json!({
            "kind": "battlemap", "title": "La Taverne", "place_id": null, "size": 32,
            "orientation": "pointe", "opacity": 0.45, "feet_per_hex": 5,
        })]
    );
    assert_eq!(
        s.media(GUE).await,
        [json!({
            "kind": "battlemap", "title": "Le Gué", "place_id": TAVERNE, "size": 48,
            "orientation": "plat", "opacity": 0.2, "feet_per_hex": 5,
        })]
    );
    assert_eq!(s.place_of(SALLE).await, None);
    s.drop().await;
}

#[tokio::test]
async fn what_the_tables_refuse_at_a_start_is_left_aside_and_keeps_nothing_else_from_its_row() {
    let s = Scratch::new("aside").await;
    {
        let engine = s.serve().await;
        for (media, kind) in [(CARTE, "battlemap"), (PORTRAIT, "illustration")] {
            applied(
                &settled(
                    &engine,
                    "test.addMedia",
                    media,
                    uploaded(media, kind, Some(TAVERNE)),
                )
                .await,
            );
        }
        let grid = json!({
            "hexSizePx": 48, "offsetX": 30, "offsetY": 25, "orientation": "plat",
            "color": "#c8a24b", "opacity": 0.2,
        });
        applied(&settled(&engine, "test.updateMedia", GUE, grid).await);
        applied(
            &settled(
                &engine,
                "test.updateMedia",
                SALLE,
                json!({ "placeId": MOULIN }),
            )
            .await,
        );
    }
    // Every projection lost, and the Taverne made an NPC since: the two
    // uploads and the ford's place are what `media-place-is-lieu` refuses;
    // one grid the table refuses, the other lost.
    s.client
        .batch_execute(&format!(
            "BEGIN;
             SELECT set_config(entity_versioned_setting('{TAVERNE}'), 'on', true);
             DELETE FROM media WHERE id IN ('{CARTE}', '{PORTRAIT}');
             UPDATE entity SET type = 'pnj' WHERE id = '{TAVERNE}';
             UPDATE media SET place_id = NULL WHERE id = '{SALLE}';
             UPDATE battlemap_grid SET hex_size_px = 64, orientation = 'pointe' WHERE media_id = '{GUE}';
             UPDATE dataguard_rows SET fields = fields || '{{\"placeId\": \"{TAVERNE}\"}}' WHERE partition = 'Media/{GUE}';
             COMMIT"
        ))
        .await
        .expect("the projections lost");
    let _restarted = s.store().await;
    for media in [CARTE, PORTRAIT] {
        assert!(s.media(media).await.is_empty(), "{media} left aside");
    }
    assert_eq!(s.place_of(GUE).await, None, "a place left aside");
    assert_eq!(s.place_of(SALLE).await.as_deref(), Some(MOULIN));
    assert_eq!(
        s.media(GUE).await,
        [json!({
            "kind": "battlemap", "title": "Le Gué", "place_id": null, "size": 48,
            "orientation": "plat", "opacity": 0.2, "feet_per_hex": 5,
        })]
    );
    s.drop().await;
}

#[tokio::test]
async fn tables_not_migrated_yet_keep_no_store_from_starting() {
    let s = Scratch::created("bare", true).await;
    let store = s.store().await;
    assert_eq!(store.data_version().await.expect("a version"), 0);
    s.drop().await;
}

#[tokio::test]
async fn a_media_only_the_tables_hold_is_the_engine_s_row_until_a_command_writes_it() {
    let s = Scratch::new("hydrate").await;
    let store = s.store().await;
    let row = store
        .row(&format!("Media/{}", GUE.to_uppercase()))
        .await
        .expect("the row");
    let expected: Value = json!({
        "id": GUE, "campaignId": VALOMBRE, "kind": "battlemap", "title": "Le Gué",
        "filePath": "media/gue.png", "widthPx": 960, "heightPx": 640,
        "mediaId": GUE, "hexSizePx": 30.00, "offsetX": 12.00, "offsetY": 8.00,
        "orientation": "pointe", "color": "#202020", "opacity": 0.40, "feetPerHex": 5,
    });
    let mut row: serde_json::Map<String, Value> = row.into_iter().collect();
    assert!(
        row.remove("uploadedAt").is_some_and(|at| at.is_string()),
        "{row:?}"
    );
    for (field, value) in expected.as_object().expect("an object") {
        let held = &row[field];
        match (held.as_f64(), value.as_f64()) {
            (Some(held), Some(value)) => assert!((held - value).abs() < 1e-9, "{field}"),
            _ => assert_eq!(held, value, "{field}"),
        }
    }
    assert_eq!(
        row.len(),
        expected.as_object().map_or(0, serde_json::Map::len)
    );
    // An illustration has no grid, and its place is the table's.
    let salle = store.row(&format!("Media/{SALLE}")).await.expect("the row");
    assert_eq!(salle["placeId"], json!(TAVERNE));
    assert!(!salle.contains_key("mediaId"), "{salle:?}");
    // No such media, and one the engine deleted, are none.
    assert!(
        store
            .row("Media/a1000000-0000-4000-8000-0000000000ff")
            .await
            .expect("no row")
            .is_empty()
    );
    let engine = s.serve().await;
    applied(&settled(&engine, "test.deleteMedia", SALLE, json!({})).await);
    assert!(
        store
            .row(&format!("Media/{SALLE}"))
            .await
            .expect("no row")
            .is_empty()
    );
    s.drop().await;
}

#[tokio::test]
async fn a_place_is_read_from_the_tables_live_in_whatever_case() {
    let s = Scratch::new("place").await;
    // The engine's own tables, which a tombstone is read from.
    let _store = s.store().await;
    let tables = ReadOnly::new(connect(&s.url).await);
    let place = |id: String| {
        let tables = &tables;
        async move { aggregate_media::place(tables, &id).await.expect("read") }
    };
    let lieu = place(TAVERNE.to_uppercase()).await.expect("the Taverne");
    assert_eq!(lieu["type"], "lieu");
    assert_eq!(lieu["entityCampaignId"], VALOMBRE);
    assert_eq!(lieu.len(), 2, "{lieu:?}");
    assert_eq!(
        place(MORLOX.to_owned()).await.expect("Morlox")["type"],
        "pnj"
    );
    assert_eq!(place("la-taverne".to_owned()).await, None);
    assert_eq!(
        place("e1000000-0000-4000-8000-0000000000ff".to_owned()).await,
        None
    );
    // One the engine deleted is none.
    s.client
        .batch_execute(&format!(
            "INSERT INTO dataguard_rows (partition, tombstoned) VALUES ('Entity/{MOULIN}', true)"
        ))
        .await
        .expect("the Moulin deleted");
    assert_eq!(place(MOULIN.to_owned()).await, None);
    drop(tables);
    s.drop().await;
}
