//! campaign.exportCampaign over PostgreSQL (`DATABASE_URL`): an export of a
//! campaign the tables hold — one the fixtures wrote, which no command wrote
//! before — stamps `campaign.last_exported_at` and changes no other column
//! of any table (`export-stamps-date`); its command's result, once applied,
//! answers the archive — the campaign, its players, its elements with their
//! sheets, aliases and versions — and an export that fails changes nothing.
//! Each test works in a scratch database of its own, migrated as the server
//! migrates it, dropped when it ends.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, Rows, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const MIGRATIONS: [&str; 3] = [
    include_str!("../../../data/migrations/0002_campaign.sql"),
    include_str!("../../../data/migrations/0003_player.sql"),
    include_str!("../../../data/migrations/0004_entity.sql"),
];

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const CLAIRE: &str = "b1000000-0000-4000-8000-000000000001";
const NOWHERE: &str = "c1000000-0000-4000-8000-0000000000ff";

/// A campaign's fields changed, test-only: no DataCapability of the product
/// writes a campaign but the export.
const UPDATE: &str = r#"{"dataCapability":"test.updateCampaign","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A campaign deleted, test-only: the engine's soft delete, a tombstone.
const DELETE: &str = r#"{"dataCapability":"test.deleteCampaign","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

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

/// A database of its own, migrated, holding — as the fixtures write them —
/// Valombre, Claire its player, and Brume, another campaign.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("export_campaign_test_{name}_{}", std::process::id());
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
                "INSERT INTO campaign (id, name, kind, pitch, sound_master_volume)
                     VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne', 'Une cité sous la cendre.', 0.8),
                            ('{BRUME}', 'La Brume', 'oneshot', NULL, 1);
                 INSERT INTO player (id, campaign_id, name, notes) VALUES ('{CLAIRE}', '{VALOMBRE}', 'Claire', 'Joue Elowen.');"
            ))
            .await
            .expect("Valombre, Claire and Brume");
        Scratch { name, url, client }
    }

    /// The engine as the server plants it over this database, and its wire
    /// protocol.
    async fn serve(&self) -> (Arc<Engine>, Router) {
        let (engine, _, app) = self.serve_with(&[]).await;
        (engine, app)
    }

    /// The same with the test-only DataCapabilities `manifests`, and the
    /// engine's rows.
    async fn serve_with(&self, manifests: &[&str]) -> (Arc<Engine>, Rows, Router) {
        let Planted { mut engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        for manifest in manifests {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        let engine = Arc::new(engine);
        let app = router(
            engine.clone(),
            rows.clone(),
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        (engine, rows, app)
    }

    /// Every row of every table of the product, as JSON, by table: the
    /// DataGuard's own (`dataguard_*`) aside — its queue and its journal
    /// record every command, whatever it writes.
    async fn tables(&self) -> BTreeMap<String, Vec<Value>> {
        let names = self
            .client
            .query(
                "SELECT tablename::text FROM pg_tables
                 WHERE schemaname = 'public' AND tablename NOT LIKE 'dataguard%'
                 ORDER BY tablename",
                &[],
            )
            .await
            .expect("the tables");
        let mut tables = BTreeMap::new();
        for name in names {
            let name: String = name.get(0);
            let rows = self
                .client
                .query(
                    &format!("SELECT to_jsonb(t)::text FROM {name} t ORDER BY to_jsonb(t)::text"),
                    &[],
                )
                .await
                .expect("a table's rows")
                .iter()
                .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
                .collect();
            tables.insert(name, rows);
        }
        tables
    }

    /// Whether `campaign`'s last export is the instant `at` names.
    async fn exported_at(&self, campaign: &str, at: &str) -> bool {
        self.client
            .query_one(
                "SELECT last_exported_at IS NOT DISTINCT FROM ($2::text)::timestamptz
                 FROM campaign WHERE id = ($1::text)::uuid",
                &[&campaign, &at],
            )
            .await
            .expect("the campaign")
            .get(0)
    }

    /// Whether `campaign`'s row holds no export.
    async fn never_exported(&self, campaign: &str) -> bool {
        self.client
            .query_one(
                "SELECT last_exported_at IS NULL FROM campaign WHERE id = ($1::text)::uuid",
                &[&campaign],
            )
            .await
            .expect("the campaign")
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

/// `body` sent to `uri` — `POST` with a body, `GET` without — and the answer.
async fn call(app: &Router, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let request = match body {
        Some(body) => Request::post(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => Request::get(uri).body(Body::empty()),
    }
    .expect("a request");
    let response = app.clone().oneshot(request).await.expect("an answer");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Submits `body` as `from`, with no role, as a Micro-UI does: the answer,
/// and what the applier made of it when it was queued.
async fn command(
    engine: &Engine,
    app: &Router,
    mut body: Value,
) -> (StatusCode, Value, Option<CommandResult>) {
    body["caller"] = json!({ "by": "dm", "from": "CampaignOverview", "roles": [] });
    let (status, answer) = call(app, "/commands", Some(body)).await;
    if status != StatusCode::ACCEPTED {
        return (status, answer, None);
    }
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    (status, answer, Some(settled.remove(0)))
}

/// `data_capability` submitted straight to the engine on `Campaign/<target>`,
/// bypassing the wire's preparation: its id, and what the applier made of
/// it.
async fn submit(
    engine: &Engine,
    data_capability: &str,
    target: &str,
    payload: Value,
) -> (String, CommandResult) {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    let submitted = engine
        .submit(Submission {
            data_capability: data_capability.to_string(),
            payload: payload.into_iter().collect(),
            target_id: Some(target.to_string()),
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "CampaignOverview".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    (submitted.command_id, result)
}

/// Exports Valombre as the CampaignOverview panel does: the applied result,
/// and what `GET /commands/<id>` answers then.
async fn export(engine: &Engine, app: &Router, body: Value) -> Value {
    let (status, answer, result) = command(engine, app, body).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let result = result.expect("queued");
    assert_eq!(result.status, State::Applied, "{result:?}");
    let id = answer["commandId"].as_str().expect("a command id");
    let (status, answer) = call(app, &format!("/commands/{id}"), None).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    answer
}

fn exporting(id: &str) -> Value {
    json!({ "dataCapability": "campaign.exportCampaign@1", "payload": { "id": id } })
}

/// Saves an element of Valombre through campaign.saveEntity: its id.
async fn save(engine: &Engine, app: &Router, entity: Value) -> String {
    let mut body = json!({
        "dataCapability": "campaign.saveEntity@1",
        "payload": { "campaign": VALOMBRE, "entity": entity },
    });
    body["caller"] = json!({ "by": "dm", "from": "VersionHistory", "roles": ["role:approver"] });
    let (status, answer) = call(app, "/commands", Some(body)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    answer["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("Entity/"))
        .expect("an Entity partition")
        .to_owned()
}

/// Elowen, Claire's PC, saved twice — two versions — and the Tour d'Ambre,
/// a place: their ids.
async fn valombre_s_elements(engine: &Engine, app: &Router) -> (String, String) {
    let mut elowen = json!({
        "kind": "pj",
        "name": "Elowen Ventargent",
        "aliases": ["Elowen", "la Rôdeuse"],
        "player": CLAIRE,
        "notes": "Cherche son frère.",
        "stats": { "FOR": 12, "DEX": 18, "CON": 14, "INT": 10, "SAG": 15, "CHA": 9 },
        "ca": 16,
        "pvMax": 44,
        "color": "success",
    });
    let id = save(engine, app, elowen.clone()).await;
    elowen["id"] = json!(id);
    elowen["notes"] = json!("A retrouvé Aerin.");
    save(engine, app, elowen).await;
    let tower = save(
        engine,
        app,
        json!({ "kind": "lieu", "name": "La Tour d'Ambre", "description": "Au cœur de la cendre." }),
    )
    .await;
    (id, tower)
}

#[tokio::test]
async fn export_stamps_date_sets_last_exported_at_and_no_other_column_of_any_table_changes() {
    let scratch = Scratch::new("stamp").await;
    let (engine, app) = scratch.serve().await;
    valombre_s_elements(&engine, &app).await;
    let before = scratch.tables().await;
    assert!(before.contains_key("campaign") && before.contains_key("entity_version"));

    let answer = export(&engine, &app, exporting(VALOMBRE)).await;
    let stamp = answer["result"]["archive"]["content"]["exported_at"]
        .as_str()
        .expect("the export's time");
    assert!(scratch.exported_at(VALOMBRE, stamp).await, "{answer}");

    let mut after = scratch.tables().await;
    let campaigns = after.get_mut("campaign").expect("the campaign table");
    for campaign in campaigns.iter_mut() {
        if campaign["id"] == VALOMBRE {
            assert!(campaign["last_exported_at"].is_string(), "{campaign}");
            campaign["last_exported_at"] = Value::Null;
        }
    }
    campaigns.sort_by_key(Value::to_string);
    let mut expected = before;
    expected
        .get_mut("campaign")
        .expect("the campaign table")
        .sort_by_key(Value::to_string);
    assert_eq!(after, expected);
    scratch.drop().await;
}

#[tokio::test]
async fn the_archive_holds_the_campaign_its_players_and_its_elements_with_sheets_aliases_and_versions()
 {
    let scratch = Scratch::new("archive").await;
    let (engine, app) = scratch.serve().await;
    let (elowen, tower) = valombre_s_elements(&engine, &app).await;

    let answer = export(&engine, &app, exporting(VALOMBRE)).await;
    let archive = &answer["result"]["archive"];
    let file = archive["file"].as_str().expect("a file name");
    assert!(
        file.starts_with("les-cendres-de-valombre-") && file.ends_with("Z.json"),
        "{file}"
    );
    let content = &archive["content"];
    assert_eq!(content["format"], "campaign-archive");
    assert_eq!(content["version"], 1);
    assert_eq!(content["origin_campaign_id"], VALOMBRE);
    assert_eq!(content["campaign"]["id"], VALOMBRE);
    assert_eq!(content["campaign"]["name"], "Les Cendres de Valombre");
    assert_eq!(content["campaign"]["pitch"], "Une cité sous la cendre.");
    // As it was before this export: never exported.
    assert_eq!(content["campaign"]["last_exported_at"], Value::Null);

    let players = content["players"].as_array().expect("players");
    assert_eq!(players.len(), 1, "{players:?}");
    assert_eq!(players[0]["id"], CLAIRE);
    assert_eq!(players[0]["notes"], "Joue Elowen.");

    let elements = content["elements"].as_array().expect("elements");
    assert_eq!(elements.len(), 2, "{elements:?}");
    let of = |id: &str| {
        elements
            .iter()
            .find(|e| e["id"] == id)
            .unwrap_or_else(|| panic!("{id} is in the archive"))
    };
    let pc = of(&elowen);
    assert_eq!(pc["type"], "pj");
    assert_eq!(pc["notes"], "A retrouvé Aerin.");
    assert_eq!(pc["character"]["player_id"], CLAIRE);
    assert_eq!(pc["character"]["hp_max"], 44);
    assert_eq!(pc["aliases"], json!(["Elowen", "la Rôdeuse"]));
    let versions = pc["versions"].as_array().expect("versions");
    let numbers: Vec<&Value> = versions.iter().map(|v| &v["version_number"]).collect();
    assert_eq!(numbers, [1, 2]);
    assert!(
        versions
            .iter()
            .all(|v| v["snapshot"].is_object() && v["diff"].is_array())
    );
    let place = of(&tower);
    assert_eq!(place["type"], "lieu");
    assert_eq!(place["character"], Value::Null);
    assert_eq!(place["aliases"], json!([]));
    assert_eq!(place["versions"].as_array().map(Vec::len), Some(1));
    // Another campaign's rows are not this one's.
    let brume = export(&engine, &app, exporting(BRUME)).await;
    assert_eq!(brume["result"]["archive"]["content"]["players"], json!([]));
    assert_eq!(brume["result"]["archive"]["content"]["elements"], json!([]));
    scratch.drop().await;
}

#[tokio::test]
async fn an_export_named_by_its_target_or_in_any_case_stamps_the_one_campaign_and_each_export_its_own_time()
 {
    let scratch = Scratch::new("target").await;
    let (engine, app) = scratch.serve().await;
    // As the rules' tests name it: the target, and an empty payload.
    let first = export(
        &engine,
        &app,
        json!({
            "dataCapability": "campaign.exportCampaign@1",
            "target": { "aggregate": "Campaign", "id": VALOMBRE },
            "payload": {},
        }),
    )
    .await;
    let first = first["result"]["archive"]["content"]["exported_at"]
        .as_str()
        .expect("a time")
        .to_owned();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let second = export(&engine, &app, exporting(&VALOMBRE.to_uppercase())).await;
    let content = &second["result"]["archive"]["content"];
    assert_eq!(content["origin_campaign_id"], VALOMBRE);
    let stamp = content["exported_at"].as_str().expect("a time");
    assert!(stamp > first.as_str(), "{stamp} after {first}");
    assert!(scratch.exported_at(VALOMBRE, stamp).await);
    // The archive holds the campaign as the export before left it.
    let shown = content["campaign"]["last_exported_at"]
        .as_str()
        .expect("the previous export");
    let previous: bool = scratch
        .client
        .query_one(
            "SELECT ($1::text)::timestamptz = ($2::text)::timestamptz",
            &[&shown, &first],
        )
        .await
        .expect("compared")
        .get(0);
    assert!(previous, "{shown} is {first}");
    assert!(
        scratch.never_exported(BRUME).await,
        "Brume was not exported"
    );
    scratch.drop().await;
}

#[tokio::test]
async fn an_export_that_fails_leaves_last_exported_at_unchanged_and_answers_no_archive() {
    let scratch = Scratch::new("fails").await;
    let (engine, app) = scratch.serve().await;
    let before = scratch.tables().await;
    // A campaign the tables do not hold, or none named: refused, not queued.
    for body in [
        exporting(NOWHERE),
        exporting("pas-un-uuid"),
        json!({ "dataCapability": "campaign.exportCampaign@1", "payload": {} }),
    ] {
        let (status, answer, result) = command(&engine, &app, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
        assert_eq!(answer["error"], "cette campagne n'existe pas");
        assert!(result.is_none());
    }
    // One the DataGuard rejects when it is processed — a stamp of no date.
    let (submitted, result) = submit(
        &engine,
        "campaign.exportCampaign@1",
        VALOMBRE,
        json!({ "lastExportedAt": "" }),
    )
    .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert!(
        result.violations.iter().any(|v| v == "export-stamps-date"),
        "{result:?}"
    );
    let (_, answer) = call(&app, &format!("/commands/{submitted}"), None).await;
    assert_eq!(answer["result"]["archive"], Value::Null, "{answer}");
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn an_archive_is_answered_only_once_its_stamp_is_applied_never_for_an_export_rejected() {
    let scratch = Scratch::new("pending").await;
    let (engine, app) = scratch.serve().await;
    let mut body = exporting(BRUME);
    body["caller"] = json!({ "by": "dm", "from": "CampaignOverview", "roles": [] });
    let (status, answer) = call(&app, "/commands", Some(body)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let status_of = format!("/commands/{}", answer["commandId"].as_str().expect("an id"));
    // Queued: no archive yet.
    let (_, queued) = call(&app, &status_of, None).await;
    assert_ne!(queued["result"]["status"], "applied", "{queued}");
    assert_eq!(queued["result"]["archive"], Value::Null, "{queued}");
    // Brume is gone before the applier stamps it: the export is rejected.
    scratch
        .client
        .batch_execute(&format!("DELETE FROM campaign WHERE id = '{BRUME}'"))
        .await
        .expect("Brume deleted");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Rejected, "{result:?}");
    let (_, rejected) = call(&app, &status_of, None).await;
    assert_eq!(rejected["result"]["status"], "rejected", "{rejected}");
    assert_eq!(rejected["result"]["archive"], Value::Null, "{rejected}");
    scratch.drop().await;
}

#[tokio::test]
async fn a_stamp_the_table_lost_is_written_again_at_the_next_start_whatever_another_campaign_holds()
{
    let scratch = Scratch::new("repair").await;
    let (engine, _, app) = scratch.serve_with(&[UPDATE]).await;
    let answer = export(&engine, &app, exporting(VALOMBRE)).await;
    let stamp = answer["result"]["archive"]["content"]["exported_at"]
        .as_str()
        .expect("a time")
        .to_owned();
    // Another campaign's row holds a stamp that is no time.
    let (_, result) = submit(
        &engine,
        "test.updateCampaign@1",
        BRUME,
        json!({ "lastExportedAt": "pas une date" }),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    drop((engine, app));
    // As if the projection had failed after the engine applied the stamp.
    scratch
        .client
        .batch_execute("UPDATE campaign SET last_exported_at = NULL")
        .await
        .expect("the stamp lost");
    let (_engine, _app) = scratch.serve().await;
    assert!(scratch.exported_at(VALOMBRE, &stamp).await);
    scratch.drop().await;
}

#[tokio::test]
async fn a_campaign_the_engine_deleted_is_exported_no_more() {
    let scratch = Scratch::new("deleted").await;
    let (engine, _, app) = scratch.serve_with(&[DELETE]).await;
    let (_, result) = submit(&engine, "test.deleteCampaign@1", BRUME, json!({})).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let (status, answer, result) = command(&engine, &app, exporting(BRUME)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert!(result.is_none());
    // Nor is it held to judge a command written straight to the engine.
    let (_, result) = submit(
        &engine,
        "campaign.exportCampaign@1",
        BRUME,
        json!({ "lastExportedAt": "2026-10-18T09:30:00Z" }),
    )
    .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert!(scratch.never_exported(BRUME).await);
    scratch.drop().await;
}

#[tokio::test]
async fn a_campaign_s_row_is_its_table_s_over_what_the_engine_copied_of_it() {
    let scratch = Scratch::new("row").await;
    let (engine, rows, app) = scratch.serve_with(&[]).await;
    let partition = format!("Campaign/{VALOMBRE}");
    let row = rows.projected(&partition).await.expect("the row");
    assert_eq!(row["name"], "Les Cendres de Valombre");
    assert_eq!(row["kind"], "campagne");
    export(&engine, &app, exporting(VALOMBRE)).await;
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE campaign SET name = 'Valombre renaît' WHERE id = '{VALOMBRE}'"
        ))
        .await
        .expect("renamed");
    let row = rows.projected(&partition).await.expect("the row");
    assert_eq!(row["name"], "Valombre renaît");
    assert!(row["lastExportedAt"].is_string(), "{row:?}");
    let second = export(&engine, &app, exporting(VALOMBRE)).await;
    assert_eq!(
        second["result"]["archive"]["content"]["campaign"]["name"],
        "Valombre renaît"
    );
    scratch.drop().await;
}
