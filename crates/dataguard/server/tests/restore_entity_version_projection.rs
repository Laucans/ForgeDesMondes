//! campaign.restoreEntityVersion over PostgreSQL (`DATABASE_URL`): a
//! restoration applied on `Entity/<id>` rewrites `entity`, `character` and
//! `entity_alias` as the version restored holds them, and adds to
//! `entity_version` the version of source `restauration` naming it — which
//! the table's `restore_exact` trigger judges again — leaving the earlier
//! ones as they were. The versions a restoration names are the tables', at a
//! start too. Each test works in a scratch database of its own, migrated as
//! the server migrates it, dropped when it ends.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::command::{CommandResult, State};
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const MIGRATIONS: [&str; 3] = [
    include_str!("../../../data/migrations/0002_campaign.sql"),
    include_str!("../../../data/migrations/0003_player.sql"),
    include_str!("../../../data/migrations/0004_entity.sql"),
];

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const CLAIRE: &str = "b1000000-0000-4000-8000-000000000001";

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

/// A database of its own, migrated, holding Valombre and Claire, its player.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("restore_entity_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let client = connect(&url).await;
        for migration in MIGRATIONS {
            client.batch_execute(migration).await.expect("migrated");
        }
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 INSERT INTO player (id, campaign_id, name) VALUES ('{CLAIRE}', '{VALOMBRE}', 'Claire');"
            ))
            .await
            .expect("Valombre and Claire");
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

    /// The rows `sql` selects, each as JSON text.
    async fn rows(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(&format!("SELECT to_jsonb(q)::text FROM ({sql}) q"), &[])
            .await
            .expect("the tables")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
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

/// Submits `body` as the DM, approver of their own campaign: the answer,
/// and what the applier made of it.
async fn command(engine: &Engine, app: &Router, mut body: Value) -> (Value, CommandResult) {
    body["caller"] = json!({ "by": "dm", "from": "VersionHistory", "roles": ["role:approver"] });
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request");
    let response = app.clone().oneshot(request).await.expect("an answer");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    let answer: Value = serde_json::from_slice(&bytes).expect("JSON");
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    (answer, settled.remove(0))
}

/// Saves Elowen, Claire's PC, as `notes`, `aliases` and `hp` say: her id.
async fn save(
    engine: &Engine,
    app: &Router,
    id: Option<&str>,
    notes: &str,
    aliases: &[&str],
    hp: i64,
) -> String {
    let mut entity = json!({
        "kind": "pj",
        "name": "Elowen Ventargent",
        "aliases": aliases,
        "player": CLAIRE,
        "notes": notes,
        "stats": { "FOR": 12, "DEX": 18, "CON": 14, "INT": 10, "SAG": 15, "CHA": 9 },
        "ca": 16,
        "pvMax": hp,
        "color": "success",
    });
    if let Some(id) = id {
        entity["id"] = json!(id);
    }
    let (answer, result) = command(
        engine,
        app,
        json!({
            "dataCapability": "campaign.saveEntity@1",
            "payload": { "campaign": VALOMBRE, "entity": entity },
        }),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    answer["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("Entity/"))
        .expect("an Entity partition")
        .to_owned()
}

/// Restores `version` of `id`, as the VersionHistory panel names it.
async fn restore(engine: &Engine, app: &Router, id: &str, version: Value) -> CommandResult {
    command(
        engine,
        app,
        json!({
            "dataCapability": "campaign.restoreEntityVersion@1",
            "target": { "aggregate": "Entity", "id": id },
            "payload": { "version": version },
        }),
    )
    .await
    .1
}

/// Elowen brought to version 4, each save moving her notes, aliases and HP.
async fn at_version_4(engine: &Engine, app: &Router) -> String {
    let id = save(engine, app, None, "Cherche son frère.", &["Elowen"], 44).await;
    for (notes, aliases, hp) in [
        (
            "Cherche son frère Aerin.",
            &["Elowen", "la Rôdeuse"][..],
            40,
        ),
        ("A retrouvé Aerin.", &["la Rôdeuse"][..], 52),
        ("Aerin est mort.", &["l'archère"][..], 52),
    ] {
        save(engine, app, Some(&id), notes, aliases, hp).await;
    }
    id
}

#[tokio::test]
async fn a_restoration_rewrites_the_tables_as_the_version_and_adds_a_fifth_leaving_the_others() {
    let s = Scratch::new("tables").await;
    let (engine, app) = s.serve().await;
    let id = at_version_4(&engine, &app).await;
    let versions = format!(
        "SELECT id, version_number, source, snapshot, diff, restored_from_version_id \
         FROM entity_version WHERE entity_id = '{id}' ORDER BY version_number"
    );
    let before = s.rows(&versions).await;
    assert_eq!(before.len(), 4, "{before:?}");
    let v2 = &before[1];

    let result = restore(&engine, &app, &id, json!(2)).await;
    assert_eq!(result.status, State::Applied, "{result:?}");

    // restore-exact: the element, its sheet and its aliases as version 2.
    let entity = s
        .rows(&format!(
            "SELECT type, name, description, notes FROM entity WHERE id = '{id}'"
        ))
        .await;
    assert_eq!(
        entity,
        [
            json!({ "type": "pj", "name": "Elowen Ventargent", "description": null, "notes": "Cherche son frère Aerin." })
        ]
    );
    let character = s
        .rows(&format!(
            "SELECT player_id, strength, dexterity, constitution, intelligence, wisdom, charisma, \
             armor_class, hp_max, spells, actions, token_color FROM character WHERE entity_id = '{id}'"
        ))
        .await;
    assert_eq!(
        character,
        [json!({
            "player_id": CLAIRE, "strength": 12, "dexterity": 18, "constitution": 14,
            "intelligence": 10, "wisdom": 15, "charisma": 9, "armor_class": 16, "hp_max": 40,
            "spells": [], "actions": [], "token_color": "success",
        })]
    );
    assert_eq!(
        character[0]["hp_max"], v2["snapshot"]["hpMax"],
        "the sheet is version 2's"
    );
    let aliases = s
        .rows(&format!(
            "SELECT alias FROM entity_alias WHERE entity_id = '{id}' ORDER BY alias"
        ))
        .await;
    assert_eq!(
        aliases,
        [
            json!({ "alias": "Elowen" }),
            json!({ "alias": "la Rôdeuse" })
        ]
    );

    // every-change-versioned: version 5, a restoration naming version 2;
    // versions 1 to 4 as they were.
    let after = s.rows(&versions).await;
    assert_eq!(after.len(), 5, "{after:?}");
    assert_eq!(after[..4], before[..]);
    assert_eq!(after[4]["version_number"], 5);
    assert_eq!(after[4]["source"], "restauration");
    assert_eq!(after[4]["restored_from_version_id"], v2["id"]);
    let mut restored = after[4]["snapshot"].clone();
    let mut expected = v2["snapshot"].clone();
    // A field version 2 lacks is null in the restoration's snapshot.
    for snapshot in [&mut restored, &mut expected] {
        snapshot
            .as_object_mut()
            .expect("a snapshot")
            .retain(|_, value| !value.is_null());
    }
    assert_eq!(restored, expected);
    s.drop().await;
}

#[tokio::test]
async fn after_a_restart_the_versions_restored_are_the_tables_by_number_or_by_id() {
    let s = Scratch::new("restart").await;
    let id = {
        let (engine, app) = s.serve().await;
        at_version_4(&engine, &app).await
    };
    let v1 = s
        .rows(&format!(
            "SELECT id FROM entity_version WHERE entity_id = '{id}' AND version_number = 1"
        ))
        .await[0]["id"]
        .clone();
    // A new start: the history is read from entity_version.
    let (engine, app) = s.serve().await;
    let result = restore(&engine, &app, &id, json!(1)).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let result = restore(&engine, &app, &id, json!(5)).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let result = restore(&engine, &app, &id, v1.clone()).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let last = s
        .rows(&format!(
            "SELECT version_number, restored_from_version_id FROM entity_version \
             WHERE entity_id = '{id}' ORDER BY version_number DESC LIMIT 1"
        ))
        .await;
    assert_eq!(
        last,
        [json!({ "version_number": 7, "restored_from_version_id": v1 })]
    );
    let notes = s
        .rows(&format!("SELECT notes FROM entity WHERE id = '{id}'"))
        .await;
    assert_eq!(notes, [json!({ "notes": "Cherche son frère." })]);
    s.drop().await;
}
