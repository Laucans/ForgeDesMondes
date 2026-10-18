//! campaign.saveEntity over PostgreSQL (`DATABASE_URL`): what the engine
//! applies on `Entity/<id>` reaches `entity`, `character`, `entity_alias` and
//! `entity_version`, each save adding its version and leaving the earlier
//! ones as they were; and a start judges the aliases against the elements
//! the tables hold. Each test works in a scratch database of its own,
//! migrated as the server migrates it, dropped when it ends.

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
        let name = format!("save_entity_test_{name}_{}", std::process::id());
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

/// Saves `payload` as the DM, approver of their own campaign: the element's
/// id, and what the applier made of the save.
async fn save(engine: &Engine, app: &Router, payload: Value) -> (String, CommandResult) {
    let body = json!({
        "dataCapability": "campaign.saveEntity@1",
        "payload": payload,
        "caller": { "by": "dm", "from": "campaign.EntityEditor", "roles": ["role:approver"] },
    });
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
    let id = answer["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("Entity/"))
        .expect("an Entity partition")
        .to_owned();
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    (id, settled.remove(0))
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// Elowen, Claire's PC, as `notes` and `aliases` say.
fn elowen(id: Option<&str>, notes: &str, aliases: &[&str]) -> Value {
    let mut entity = json!({
        "kind": "pj",
        "name": "Elowen Ventargent",
        "aliases": aliases,
        "player": CLAIRE,
        "notes": notes,
        "stats": { "FOR": 12, "DEX": 18, "CON": 14, "INT": 10, "SAG": 15, "CHA": 9 },
        "ca": 16,
        "pvMax": 44,
        "features": [{ "name": "Attaque supplémentaire", "text": "Deux attaques." }],
        "color": "success",
    });
    if let Some(id) = id {
        entity["id"] = json!(id);
    }
    json!({ "campaign": VALOMBRE, "entity": entity })
}

#[tokio::test]
async fn a_save_reaches_the_tables_and_each_save_adds_a_version_leaving_the_earlier_ones() {
    let s = Scratch::new("tables").await;
    let (engine, app) = s.serve().await;
    let (id, result) = save(
        &engine,
        &app,
        elowen(None, "Cherche son frère.", &["Elowen", "la Rôdeuse"]),
    )
    .await;
    applied(&result);

    let entity = s
        .rows(&format!(
            "SELECT campaign_id, type, name, notes FROM entity WHERE id = '{id}'"
        ))
        .await;
    assert_eq!(
        entity,
        [
            json!({ "campaign_id": VALOMBRE, "type": "pj", "name": "Elowen Ventargent", "notes": "Cherche son frère." })
        ]
    );
    let character = s
        .rows(&format!(
            "SELECT player_id, strength, dexterity, armor_class, hp_max, actions, spells, token_color \
             FROM character WHERE entity_id = '{id}'"
        ))
        .await;
    assert_eq!(
        character,
        [json!({
            "player_id": CLAIRE, "strength": 12, "dexterity": 18, "armor_class": 16, "hp_max": 44,
            "actions": [{ "name": "Attaque supplémentaire", "text": "Deux attaques." }],
            "spells": [], "token_color": "success",
        })]
    );
    let aliases = format!("SELECT alias FROM entity_alias WHERE entity_id = '{id}' ORDER BY alias");
    assert_eq!(
        s.rows(&aliases).await,
        [
            json!({ "alias": "Elowen" }),
            json!({ "alias": "la Rôdeuse" })
        ]
    );
    let versions = format!(
        "SELECT version_number, source, snapshot, diff FROM entity_version \
         WHERE entity_id = '{id}' ORDER BY version_number"
    );
    let first = s.rows(&versions).await;
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(first[0]["version_number"], 1);
    assert_eq!(first[0]["source"], "dm");
    assert_eq!(
        first[0]["snapshot"]["aliases"],
        json!(["Elowen", "la Rôdeuse"])
    );
    assert_eq!(first[0]["snapshot"]["dexterity"], 18);
    assert_eq!(first[0]["snapshot"]["playerId"], CLAIRE);

    // Her notes change and she drops an alias for another.
    let (_, result) = save(
        &engine,
        &app,
        elowen(
            Some(&id),
            "Cherche son frère Aerin.",
            &["la Rôdeuse", "l'archère"],
        ),
    )
    .await;
    applied(&result);
    let both = s.rows(&versions).await;
    assert_eq!(both.len(), 2, "{both:?}");
    assert_eq!(both[0], first[0], "version 1 is never modified");
    assert_eq!(both[1]["version_number"], 2);
    let fields: Vec<&str> = both[1]["diff"]
        .as_array()
        .expect("a diff")
        .iter()
        .map(|c| c["field"].as_str().expect("a field"))
        .collect();
    assert_eq!(fields, ["aliases", "notes"]);
    assert_eq!(
        s.rows(&aliases).await,
        [
            json!({ "alias": "l'archère" }),
            json!({ "alias": "la Rôdeuse" })
        ]
    );

    // Only her notes: the diff lists notes alone.
    let (_, result) = save(
        &engine,
        &app,
        elowen(Some(&id), "Aerin est vivant.", &["la Rôdeuse", "l'archère"]),
    )
    .await;
    applied(&result);
    let all = s.rows(&versions).await;
    assert_eq!(&all[..2], &both[..]);
    assert_eq!(
        all[2]["diff"],
        json!([{ "field": "notes", "old": "Cherche son frère Aerin.", "new": "Aerin est vivant." }])
    );
    s.drop().await;
}

#[tokio::test]
async fn a_start_judges_an_alias_against_the_elements_the_tables_hold() {
    let s = Scratch::new("restart").await;
    {
        let (engine, app) = s.serve().await;
        applied(
            &save(&engine, &app, elowen(None, "", &["la Rôdeuse"]))
                .await
                .1,
        );
    }
    // Another server over the same database: Elowen is the tables'.
    let (engine, app) = s.serve().await;
    let morlox = json!({
        "campaign": VALOMBRE,
        "entity": { "kind": "pnj", "name": "Morlox", "aliases": ["LA RODEUSE"] },
    });
    let (id, result) = save(&engine, &app, morlox).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["alias-unique-in-campaign"]);
    assert!(
        s.rows(&format!("SELECT id FROM entity WHERE id = '{id}'"))
            .await
            .is_empty()
    );
    s.drop().await;
}

#[tokio::test]
async fn an_npc_made_a_place_loses_its_sheet_in_the_tables() {
    let s = Scratch::new("kind").await;
    let (engine, app) = s.serve().await;
    let (id, result) = save(
        &engine,
        &app,
        json!({
            "campaign": VALOMBRE,
            "entity": { "kind": "pnj", "name": "Le Moulin", "stats": { "DEX": 10 }, "color": "danger" },
        }),
    )
    .await;
    applied(&result);
    let sheet = format!("SELECT entity_id FROM character WHERE entity_id = '{id}'");
    assert_eq!(s.rows(&sheet).await.len(), 1);
    let (_, result) = save(
        &engine,
        &app,
        json!({ "campaign": VALOMBRE, "entity": { "id": id, "kind": "lieu" } }),
    )
    .await;
    applied(&result);
    assert!(s.rows(&sheet).await.is_empty());
    let versions = s
        .rows(&format!(
            "SELECT version_number, snapshot -> 'type' AS type FROM entity_version \
             WHERE entity_id = '{id}' ORDER BY version_number"
        ))
        .await;
    assert_eq!(
        versions,
        [
            json!({ "version_number": 1, "type": "pnj" }),
            json!({ "version_number": 2, "type": "lieu" })
        ]
    );
    s.drop().await;
}
