//! The campaign Capabilities read from the tables the migrations build
//! (`DATABASE_URL`), through the registry's reads `Reads` opens: their
//! variables are bound to the query, and a Capability answers for the id it
//! is given and no other. (The names an id goes by on the wire are the
//! product server's, `crates/server/tests/fixture_names.rs`.) The rows go in
//! through the Data layer. A scratch database of its own, dropped when the
//! test ends.

mod common;

use std::path::Path;

use common::Reads;
use data::Table;
use dataguard_server::engine::tokio_postgres::{self, NoTls};
use serde_json::{Value, json};

const CENDRES: &str = "c1000000-0000-4000-8000-000000000001";
const OTHER: &str = "c1000000-0000-4000-8000-000000000002";
const JULIE: &str = "a1000000-0000-4000-8000-000000000001";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000002";

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

/// Two campaigns; one player, a PC and an NPC in the first.
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
            "player",
            "id",
            &["id", "campaign_id", "name"],
            &json!([{ "id": JULIE, "campaign_id": CENDRES, "name": "Julie" }]),
        ),
        table(
            "entity",
            "id",
            &["id", "campaign_id", "type", "name", "notes"],
            &json!([
                { "id": ELOWEN, "campaign_id": CENDRES, "type": "pj", "name": "Elowen",
                  "notes": "Cherche son frère." },
                { "id": MORLOX, "campaign_id": CENDRES, "type": "pnj", "name": "Morlox",
                  "notes": null },
            ]),
        ),
        table(
            "character",
            "entity_id",
            &["entity_id", "player_id", "dexterity", "token_color"],
            &json!([
                { "entity_id": ELOWEN, "player_id": JULIE, "dexterity": 18, "token_color": "success" },
                { "entity_id": MORLOX, "player_id": null, "dexterity": 10, "token_color": "secondary" },
            ]),
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
            &json!([
                { "id": "f1000000-0000-4000-8000-000000000001", "entity_id": ELOWEN,
                  "version_number": 1, "source": "dm", "snapshot": {}, "diff": [] },
                { "id": "f1000000-0000-4000-8000-000000000002", "entity_id": MORLOX,
                  "version_number": 1, "source": "dm", "snapshot": {}, "diff": [] },
            ]),
        ),
    ]
}

/// A migrated scratch database holding [`rows`]: its name, and a connection.
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
async fn a_capability_answers_for_the_id_it_is_given_and_no_other() {
    let (name, client) = scratch("campaign_read").await;
    let reads = Reads::open(&named(&name)).await.expect("the reads open");
    let run = |id: &'static str, variables: Value| {
        let reads = &reads;
        async move { reads.run(id, &variables).await.expect("an answer") }
    };

    let players = run("campaign.listPlayers", json!({ "campaign": CENDRES })).await;
    assert_eq!(
        players,
        json!([{ "id": JULIE, "name": "Julie", "notes": null }])
    );
    let none = run("campaign.listPlayers", json!({ "campaign": OTHER })).await;
    assert_eq!(none, json!([]), "the other campaign has no player");
    let unsaid = run("campaign.listPlayers", json!({})).await;
    assert_eq!(unsaid, json!([]), "no campaign given, none read");

    let characters = run("campaign.listCharacters", json!({ "campaign": CENDRES })).await;
    let kinds: Vec<(&str, &Value)> = characters
        .as_array()
        .expect("a list")
        .iter()
        .map(|c| (c["kind"].as_str().expect("a kind"), &c["player"]))
        .collect();
    assert_eq!(kinds, [("pj", &json!(JULIE)), ("pnj", &Value::Null)]);

    let campaign = run("campaign.getCampaign", json!({ "id": CENDRES })).await;
    assert_eq!(campaign["name"], "Cendres");
    assert_eq!(campaign["counts"]["characters"], 2);
    assert_eq!(campaign["counts"]["players"], 1);
    let unknown = run("campaign.getCampaign", json!({ "id": "nobody" })).await;
    assert_eq!(unknown, Value::Null, "an unknown id has no answer");

    let sheet = run("campaign.getEntity", json!({ "id": ELOWEN })).await;
    assert_eq!(sheet["name"], "Elowen");
    assert_eq!(
        sheet["modifiers"]["DEX"], 4,
        "AbilityModifier@1: 18 gives +4"
    );

    // A Capability the product does not serve is refused, as the wire
    // refuses it (404): the engine's reads have no "null" for it.
    let refused = reads
        .run("campaign.noSuchCapability", &json!({}))
        .await
        .expect_err("not served");
    assert!(
        refused
            .to_string()
            .contains("not a registered persisted query"),
        "{refused}"
    );

    drop(reads);
    drop_scratch(&name, client).await;
}
