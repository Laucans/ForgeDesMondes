//! The media Capabilities read from the tables the migrations build
//! (`DATABASE_URL`), through the connection `Reads` opens: `media.getBattlemap`
//! and `media.listMedia` are the registry's, served by the engine's reads,
//! not by a route of this server. Their answer keeps the numbers the DM
//! saved as numbers, and picks the id or the campaign it is given and no
//! other. (The names an id goes by on the wire are the product server's,
//! `crates/server/tests/fixture_names.rs`.) The rows go in through the Data
//! layer. A scratch database of its own, dropped when the test ends.

mod common;

use std::path::Path;

use common::Reads;
use data::Table;
use dataguard_server::engine::tokio_postgres::{self, NoTls};
use serde_json::{Value, json};

const CENDRES: &str = "c1000000-0000-4000-8000-000000000001";
const OTHER: &str = "c1000000-0000-4000-8000-000000000002";
const TAVERN: &str = "e1000000-0000-4000-8000-000000000001";
const GUE: &str = "b1000000-0000-4000-8000-000000000001";
const PORTRAIT: &str = "b1000000-0000-4000-8000-000000000002";
const ELSEWHERE: &str = "b1000000-0000-4000-8000-000000000003";

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

/// Two campaigns and a tavern in the first; in it, a battlemap of the
/// tavern and an illustration; in the other, a battlemap.
fn rows() -> Vec<Table> {
    vec![
        table(
            "campaign",
            "id",
            &["id", "name", "kind"],
            &json!([
                { "id": CENDRES, "name": "Cendres", "kind": "campagne" },
                { "id": OTHER, "name": "Autre", "kind": "oneshot" },
            ]),
        ),
        table(
            "entity",
            "id",
            &["id", "campaign_id", "type", "name"],
            &json!([{ "id": TAVERN, "campaign_id": CENDRES, "type": "lieu",
                "name": "La Taverne du Sanglier" }]),
        ),
        // The Entity aggregate refuses an element no version records.
        table(
            "entity_version",
            "id",
            &[
                "id",
                "entity_id",
                "version_number",
                "source",
                "snapshot",
                "diff",
            ],
            &json!([{ "id": "f1000000-0000-4000-8000-000000000001", "entity_id": TAVERN,
                "version_number": 1, "source": "dm", "snapshot": {}, "diff": [] }]),
        ),
        table(
            "media",
            "id",
            &[
                "id",
                "campaign_id",
                "kind",
                "title",
                "file_path",
                "width_px",
                "height_px",
                "place_id",
                "uploaded_at",
            ],
            &json!([
                { "id": GUE, "campaign_id": CENDRES, "kind": "battlemap", "title": "Le Gué",
                  "file_path": "cartes/gue.jpg", "width_px": 960, "height_px": 640,
                  "place_id": TAVERN, "uploaded_at": "2026-10-14T09:00:00Z" },
                { "id": PORTRAIT, "campaign_id": CENDRES, "kind": "illustration",
                  "title": "Portrait", "file_path": "art/portrait.png", "width_px": 800,
                  "height_px": 1200, "place_id": null, "uploaded_at": "2026-10-15T09:00:00Z" },
                { "id": ELSEWHERE, "campaign_id": OTHER, "kind": "battlemap", "title": "Ailleurs",
                  "file_path": "ailleurs.jpg", "width_px": 100, "height_px": 100,
                  "place_id": null, "uploaded_at": "2026-10-13T09:00:00Z" },
            ]),
        ),
        table(
            "battlemap_grid",
            "media_id",
            &[
                "media_id",
                "hex_size_px",
                "offset_x",
                "offset_y",
                "orientation",
                "color",
                "opacity",
            ],
            &json!([
                { "media_id": GUE, "hex_size_px": 64.5, "offset_x": -3.25, "offset_y": 8,
                  "orientation": "plat", "color": "accent", "opacity": 0.75 },
                { "media_id": ELSEWHERE, "hex_size_px": 30, "offset_x": 0, "offset_y": 0,
                  "orientation": "pointe", "color": "text", "opacity": 0.4 },
            ]),
        ),
    ]
}

/// A migrated scratch database holding [`rows`], the illustration the first
/// campaign's cover: its name, and a connection.
async fn scratch(name: &str) -> (String, tokio_postgres::Client) {
    let name = format!("{name}_{}", std::process::id());
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
    client
        .batch_execute(&format!(
            "UPDATE campaign SET cover_media_id = '{PORTRAIT}' WHERE id = '{CENDRES}'"
        ))
        .await
        .expect("a cover");
    (name, client)
}

async fn drop_scratch(name: &str, client: tokio_postgres::Client) {
    drop(client);
    connect(&url())
        .await
        .batch_execute(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .await
        .expect("the scratch database dropped");
}

#[tokio::test]
async fn a_battlemap_is_read_with_its_grid_as_saved_and_five_feet_a_hex() {
    let (name, client) = scratch("media_read_battlemap").await;
    let reads = Reads::open(&named(&name)).await.expect("the reads open");

    let battlemap = reads
        .run("media.getBattlemap", &json!({ "id": GUE }))
        .await
        .expect("an answer");
    assert_eq!(
        battlemap,
        json!({
            "id": GUE, "title": "Le Gué", "file": "cartes/gue.jpg", "size": null,
            "uploadedAt": "2026-10-14", "width": 960, "height": 640, "terrain": [],
            "place": TAVERN,
            // The numeric columns are numbers, as the DM entered them.
            "grid": { "size": 64.5, "ox": -3.25, "oy": 8, "orient": "flat",
                "color": "accent", "opacity": 0.75, "feetPerHex": 5 },
        })
    );
    let other = reads
        .run("media.getBattlemap", &json!({ "id": ELSEWHERE }))
        .await
        .expect("an answer");
    assert_eq!(
        other["grid"],
        json!({ "size": 30, "ox": 0, "oy": 0, "orient": "pointy", "color": "text",
            "opacity": 0.4, "feetPerHex": 5 })
    );
    for id in [
        json!({ "id": PORTRAIT }),
        json!({ "id": "nobody" }),
        json!({}),
    ] {
        assert_eq!(
            reads
                .run("media.getBattlemap", &id)
                .await
                .expect("an answer"),
            Value::Null,
            "{id}: an illustration, an unknown id or none is no battlemap"
        );
    }

    drop(reads);
    drop_scratch(&name, client).await;
}

#[tokio::test]
async fn a_campaign_s_library_lists_its_media_and_no_other_campaign_s() {
    let (name, client) = scratch("media_read_library").await;
    let reads = Reads::open(&named(&name)).await.expect("the reads open");

    let library = reads
        .run("media.listMedia", &json!({ "campaignId": CENDRES }))
        .await
        .expect("an answer");
    assert_eq!(library["cover"], PORTRAIT, "{library}");
    let battlemaps = library["battlemaps"].as_array().expect("a list");
    assert_eq!(battlemaps.len(), 1, "{library}");
    assert_eq!(battlemaps[0]["id"], GUE);
    assert_eq!(battlemaps[0]["file"], "gue.jpg");
    assert_eq!(battlemaps[0]["place"], TAVERN);
    assert_eq!(battlemaps[0]["grid"]["size"].as_f64(), Some(64.5));
    assert_eq!(battlemaps[0]["grid"]["opacity"].as_f64(), Some(0.75));
    assert_eq!(battlemaps[0]["grid"]["orient"], "flat");
    assert_eq!(battlemaps[0]["grid"]["feetPerHex"], 5);
    assert_eq!(
        library["illustrations"],
        json!([{ "id": PORTRAIT, "title": "Portrait", "file": "portrait.png",
            "uploadedAt": "2026-10-15", "place": null, "ratio": 0.67 }])
    );

    // Version 2 names the campaign `campaign`; version 1's `campaignId` is the
    // same read.
    let by_campaign = reads
        .run("media.listMedia", &json!({ "campaign": CENDRES }))
        .await
        .expect("an answer");
    assert_eq!(
        by_campaign, library,
        "campaign and campaignId list one library"
    );
    let both = reads
        .run(
            "media.listMedia",
            &json!({ "campaign": CENDRES, "campaignId": OTHER }),
        )
        .await
        .expect("an answer");
    assert_eq!(both, library, "campaign wins over campaignId");

    let other = reads
        .run("media.listMedia", &json!({ "campaignId": OTHER }))
        .await
        .expect("an answer");
    assert_eq!(other["cover"], Value::Null);
    assert_eq!(other["battlemaps"][0]["id"], ELSEWHERE);
    assert_eq!(other["illustrations"], json!([]));
    let unsaid = reads
        .run("media.listMedia", &json!({}))
        .await
        .expect("an answer");
    assert_eq!(
        unsaid,
        json!({ "cover": null, "battlemaps": [], "illustrations": [] }),
        "no campaign given, none read"
    );

    drop(reads);
    drop_scratch(&name, client).await;
}
