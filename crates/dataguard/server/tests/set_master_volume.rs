//! soundbox.setMasterVolume over PostgreSQL (`DATABASE_URL`): the DM's
//! master slider, in percent, on a campaign the tables hold — one the
//! fixtures wrote, which no command wrote before — writes
//! `campaign.sound_master_volume` as its fraction and changes no other
//! column of any table; a volume out of range is refused by the campaign's
//! own invariant (`master-volume-range`) and changes nothing, nor does one
//! naming a campaign the tables do not hold, which is never queued. Each test
//! works in a scratch database of its own, migrated as the server migrates
//! it, dropped when it ends.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, Rows, planted, router};
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
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const NOWHERE: &str = "c1000000-0000-4000-8000-0000000000ff";

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
/// Valombre, its master volume at 80 %, and Brume, another campaign at
/// full volume.
struct Scratch {
    name: String,
    url: String,
    client: Client,
    engine: Arc<Engine>,
    rows: Rows,
    app: Router,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("set_master_volume_test_{name}_{}", std::process::id());
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
                "INSERT INTO campaign (id, name, kind, sound_master_volume)
                     VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne', 0.8),
                            ('{BRUME}', 'La Brume', 'oneshot', 1);"
            ))
            .await
            .expect("Valombre and Brume");
        let (engine, rows, app) = serve(&url).await;
        Scratch {
            name,
            url,
            client,
            engine,
            rows,
            app,
        }
    }

    /// The server started again over the same database.
    async fn restart(&mut self) {
        (self.engine, self.rows, self.app) = serve(&self.url).await;
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

    /// `campaign`'s master volume as its table holds it.
    async fn master_volume(&self, campaign: &str) -> String {
        self.client
            .query_one(
                "SELECT sound_master_volume::text FROM campaign WHERE id = ($1::text)::uuid",
                &[&campaign],
            )
            .await
            .expect("the campaign")
            .get(0)
    }

    /// Submits `body` from the LiveSoundbox, with no role, as its master
    /// slider does: the answer, and what the applier made of it when it was
    /// queued.
    async fn command(&self, mut body: Value) -> (StatusCode, Value, Option<CommandResult>) {
        body["caller"] = json!({ "by": "dm", "from": "LiveSoundbox", "roles": [] });
        let request = Request::post("/commands")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("a request");
        let response = self.app.clone().oneshot(request).await.expect("an answer");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        let answer = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        let mut settled = self.engine.run_once().await.expect("the applier ran");
        if status != StatusCode::ACCEPTED {
            assert!(settled.is_empty(), "nothing was queued: {settled:?}");
            return (status, answer, None);
        }
        assert_eq!(settled.len(), 1, "{settled:?}");
        (status, answer, Some(settled.remove(0)))
    }

    /// Drops the database — again a moment later while a backend of another
    /// role, which a role that is no superuser may not end (an autovacuum
    /// worker its writes woke), still holds it.
    async fn drop(self) {
        drop((self.client, self.app, self.rows, self.engine));
        let admin = connect(&url()).await;
        let sql = format!("DROP DATABASE {} WITH (FORCE)", self.name);
        for _ in 0..50 {
            match admin.batch_execute(&sql).await {
                Err(e)
                    if e.code()
                        == Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE) =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                dropped => return dropped.expect("the scratch database dropped"),
            }
        }
        panic!("the scratch database {} still held", self.name);
    }
}

/// The engine as the server plants it over the database `url` names, its
/// rows, and its wire protocol.
async fn serve(url: &str) -> (Arc<Engine>, Rows, Router) {
    let Planted { engine, rows } = planted(url).await.expect("the engine over PostgreSQL");
    let engine = Arc::new(engine);
    let app = router(
        engine.clone(),
        rows.clone(),
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    (engine, rows, app)
}

fn setting(campaign: &str, volume: Value) -> Value {
    json!({
        "dataCapability": "soundbox.setMasterVolume@1",
        "payload": { "campaign": campaign, "volume": volume },
    })
}

#[tokio::test]
async fn the_slider_s_percent_is_the_campaign_s_master_volume_and_no_other_column_changes() {
    let scratch = Scratch::new("applied").await;
    let mut expected = scratch.tables().await;
    let (status, answer, result) = scratch.command(setting(VALOMBRE, json!(40))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let result = result.expect("queued");
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(answer["partition"], format!("Campaign/{VALOMBRE}"));
    assert_eq!(scratch.master_volume(VALOMBRE).await, "0.40");
    // Valombre's master volume, and nothing else of any table.
    for campaign in expected.get_mut("campaign").expect("the campaigns") {
        if campaign["id"] == VALOMBRE {
            campaign["sound_master_volume"] = json!(0.4);
        }
    }
    assert_eq!(scratch.tables().await, expected);
    let row = scratch
        .rows
        .projected(&format!("Campaign/{VALOMBRE}"))
        .await
        .expect("the row");
    assert_eq!(row["soundMasterVolume"], json!(0.4), "{row:?}");
    assert_eq!(scratch.master_volume(BRUME).await, "1.00");
    scratch.drop().await;
}

#[tokio::test]
async fn the_last_position_of_the_slider_wins_whatever_case_names_the_campaign() {
    let scratch = Scratch::new("overwrite").await;
    for (body, volume) in [
        (setting(&VALOMBRE.to_uppercase(), json!(100)), "1.00"),
        (setting(VALOMBRE, json!(0)), "0.00"),
        (
            // The campaign named as the command's target, as a Micro-UI may.
            json!({
                "dataCapability": "soundbox.setMasterVolume@1",
                "target": { "id": VALOMBRE.to_uppercase() },
                "payload": { "volume": 75 },
            }),
            "0.75",
        ),
    ] {
        let (status, answer, result) = scratch.command(body).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        let result = result.expect("queued");
        assert_eq!(result.status, State::Applied, "{result:?}");
        assert_eq!(scratch.master_volume(VALOMBRE).await, volume);
    }
    scratch.drop().await;
}

#[tokio::test]
async fn master_volume_range_rejects_a_volume_out_of_range_and_the_campaign_keeps_its_own() {
    let scratch = Scratch::new("rejected").await;
    let before = scratch.tables().await;
    for volume in [
        json!(150),
        json!(-5),
        json!(100.5),
        json!("40"),
        Value::Null,
    ] {
        let (status, answer, result) = scratch.command(setting(VALOMBRE, volume.clone())).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{volume}: {answer}");
        let result = result.expect("queued");
        assert_eq!(result.status, State::Rejected, "{volume}: {result:?}");
        assert_eq!(
            result.violations,
            vec!["master-volume-range".to_string()],
            "{volume}"
        );
    }
    assert_eq!(scratch.tables().await, before);
    assert_eq!(scratch.master_volume(VALOMBRE).await, "0.80");
    scratch.drop().await;
}

#[tokio::test]
async fn a_campaign_the_tables_do_not_hold_or_none_named_is_refused_before_it_is_queued() {
    let scratch = Scratch::new("refused").await;
    let before = scratch.tables().await;
    for body in [
        setting(NOWHERE, json!(40)),
        setting("pas-un-uuid", json!(40)),
        json!({ "dataCapability": "soundbox.setMasterVolume@1", "payload": { "volume": 40 } }),
    ] {
        let (status, answer, result) = scratch.command(body.clone()).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}: {answer}");
        assert_eq!(answer["error"], "cette campagne n'existe pas");
        assert!(result.is_none());
    }
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_fractional_percent_is_kept_to_the_whole_percent_the_table_holds() {
    let scratch = Scratch::new("rounded").await;
    let (status, answer, result) = scratch.command(setting(VALOMBRE, json!(12.5))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(result.expect("queued").status, State::Applied);
    assert_eq!(scratch.master_volume(VALOMBRE).await, "0.13");
    let row = scratch
        .rows
        .projected(&format!("Campaign/{VALOMBRE}"))
        .await
        .expect("the row");
    assert_eq!(row["soundMasterVolume"], json!(0.13), "{row:?}");
    // What the engine judged and keeps is what the table holds.
    let kept: String = scratch
        .client
        .query_one(
            "SELECT fields ->> 'soundMasterVolume' FROM dataguard_rows WHERE partition = $1",
            &[&format!("Campaign/{VALOMBRE}")],
        )
        .await
        .expect("the engine's row")
        .get(0);
    assert_eq!(kept, "0.13");
    scratch.drop().await;
}

#[tokio::test]
async fn a_master_volume_the_table_lost_is_written_again_at_the_next_start_and_no_other() {
    let mut scratch = Scratch::new("repair").await;
    let (_, answer, result) = scratch.command(setting(VALOMBRE, json!(40))).await;
    assert_eq!(result.expect("queued").status, State::Applied, "{answer}");
    // As if the projection had failed after the engine applied the volume;
    // Brume, which no command wrote, moved meanwhile.
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE campaign SET sound_master_volume = 0.8 WHERE id = '{VALOMBRE}';
             UPDATE campaign SET sound_master_volume = 0.3 WHERE id = '{BRUME}';"
        ))
        .await
        .expect("the volume lost");
    scratch.restart().await;
    assert_eq!(scratch.master_volume(VALOMBRE).await, "0.40");
    assert_eq!(scratch.master_volume(BRUME).await, "0.30");
    scratch.drop().await;
}
