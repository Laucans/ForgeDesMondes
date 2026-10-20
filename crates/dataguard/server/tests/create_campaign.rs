//! campaign.createCampaign over PostgreSQL (`DATABASE_URL`): a creation over
//! the wire writes a `campaign` row that
//! `campaign.listCampaigns` lists — a oneshot with its single `session`, at
//! position 1, in prep, the microphone off (`oneshot-single-session`), a
//! campagne with none — its master volume inside 0–1
//! (`master-volume-range`); the same idempotency key twice creates once; a
//! creation the tables lost is written again at the next start. Each test
//! works in a scratch database of its own, migrated as the server migrates
//! it, dropped when it ends. What the engine judges and queues — the
//! refusals, `oneshot-single-session` and `master-volume-range` on what a
//! creation writes, an idempotency key — is proved over its in-memory store,
//! no table needed.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, Reads, planted, router_reading};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const MIGRATIONS: [&str; 5] = [
    include_str!("../../../data/migrations/0002_campaign.sql"),
    include_str!("../../../data/migrations/0003_player.sql"),
    include_str!("../../../data/migrations/0004_entity.sql"),
    include_str!("../../../data/migrations/0005_media.sql"),
    include_str!("../../../data/migrations/0006_session.sql"),
];

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";

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

/// A database of its own, migrated, holding Valombre, a campaign the
/// fixtures wrote.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

/// The engine as the server plants it, and its wire protocol, reading
/// through the product's Capabilities.
struct Served {
    engine: Arc<Engine>,
    app: Router,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("create_campaign_test_{name}_{}", std::process::id());
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
                     VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne', 0.8)"
            ))
            .await
            .expect("Valombre");
        Scratch { name, url, client }
    }

    async fn serve(&self) -> Served {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let engine = Arc::new(engine);
        let reads = Reads::open(&self.url).await.expect("the reads open");
        let app = router_reading(
            engine.clone(),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
            Arc::new(reads),
        );
        Served { engine, app }
    }

    /// The campaigns the table holds by `name`: `(id, kind, master volume)`.
    async fn campaigns(&self, name: &str) -> Vec<(String, String, f64)> {
        self.client
            .query(
                "SELECT id::text, kind, sound_master_volume::float8 FROM campaign WHERE name = $1",
                &[&name],
            )
            .await
            .expect("the campaigns")
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2)))
            .collect()
    }

    /// The sessions of `campaign`: `(id, position, title, status, microphone
    /// on)`.
    async fn sessions(&self, campaign: &str) -> Vec<(String, i32, String, String, bool)> {
        self.client
            .query(
                "SELECT id::text, position, title, status, microphone_on FROM session
                 WHERE campaign_id = ($1::text)::uuid ORDER BY position",
                &[&campaign],
            )
            .await
            .expect("the sessions")
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4)))
            .collect()
    }

    /// How many rows `table` holds.
    async fn count(&self, table: &str) -> i64 {
        self.client
            .query_one(&format!("SELECT count(*) FROM {table}"), &[])
            .await
            .expect("a count")
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

impl Served {
    /// The engine over its in-memory store, as `DATABASE_URL=memory` plants
    /// it: what it judges and queues, with no table to project to.
    async fn memory() -> Served {
        let Planted { engine, rows } = planted(MEMORY).await.expect("the engine in memory");
        let engine = Arc::new(engine);
        let app = router_reading(
            engine.clone(),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
            Arc::new(Reads::none()),
        );
        Served { engine, app }
    }

    /// `body` posted to `uri`, and the answer.
    async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::post(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("a request");
        let response = self.app.clone().oneshot(request).await.expect("an answer");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// `payload` submitted as the rules' tests send it — no target — by the
    /// DM, who approves their own campaign's critical fields (its id and its
    /// kind), from the home page, with `key` as its idempotency key: the
    /// answer, and the commands one pass of the applier settled.
    async fn create(
        &self,
        payload: Value,
        key: Option<&str>,
    ) -> (StatusCode, Value, Vec<CommandResult>) {
        let mut body = json!({
            "dataCapability": "campaign.createCampaign@1",
            "payload": payload,
            "caller": { "by": "dm", "from": "CampaignList", "roles": ["role:approver"] },
        });
        if let Some(key) = key {
            body["idempotencyKey"] = json!(key);
        }
        let (status, answer) = self.post("/commands", body).await;
        let settled = self.engine.run_once().await.expect("the applier ran");
        (status, answer, settled)
    }

    /// `payload` created and applied: its campaign's id.
    async fn created(&self, payload: Value) -> String {
        let (status, answer, settled) = self.create(payload, None).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        assert_eq!(settled.len(), 1, "{settled:?}");
        assert_eq!(settled[0].status, State::Applied, "{settled:?}");
        answer["partition"]
            .as_str()
            .and_then(|p| p.strip_prefix("Campaign/"))
            .expect("a Campaign partition")
            .to_owned()
    }

    /// What `campaign.listCampaigns` answers.
    async fn listed(&self) -> Vec<Value> {
        let (status, answer) = self
            .post(
                "/capabilities/campaign.listCampaigns",
                json!({ "variables": {} }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        answer["data"].as_array().cloned().expect("a list")
    }

    /// `payload` submitted straight to the engine as campaign.createCampaign
    /// on `Campaign/<target>`, bypassing the wire's preparation: what the
    /// applier made of it.
    async fn straight(&self, target: &str, payload: Value) -> CommandResult {
        let Value::Object(payload) = payload else {
            panic!("a payload is an object");
        };
        self.engine
            .submit(Submission {
                data_capability: "campaign.createCampaign@1".to_string(),
                payload: payload.into_iter().collect(),
                target_id: Some(target.to_string()),
                based_on: None,
                idempotency_key: None,
                caller: Caller {
                    by: "dm".to_string(),
                    from: "CampaignList".to_string(),
                    permissions: Vec::new(),
                    roles: vec!["role:approver".to_string()],
                },
            })
            .await
            .expect("queued");
        self.engine
            .run_once()
            .await
            .expect("the applier ran")
            .remove(0)
    }
}

/// A oneshot's creation as the wire's preparation writes it, on `id`, with
/// its session `session`.
fn oneshot(id: &str, session: &str) -> Value {
    json!({
        "id": id, "name": "Le Bal", "kind": "oneshot", "pitch": null, "art": null, "glyph": null,
        "createdAt": "2026-10-20T09:30:00.000Z", "soundMasterVolume": 1.0,
        "sessionId": session, "sessionCampaignId": id, "sessionPosition": 1,
        "sessionTitle": "Le Bal", "sessionStatus": "prep", "microphoneOn": false,
    })
}

#[tokio::test]
async fn a_campagne_is_listed_by_campaign_list_campaigns_and_has_no_session() {
    let scratch = Scratch::new("campagne").await;
    let served = scratch.serve().await;
    let id = served
        .created(json!({
            "name": "Les Brumes de Corbière",
            "type": "campagne",
            "pitch": "Un port noyé de brume.",
            "art": "forest",
            "glyph": "B",
        }))
        .await;
    let listed = served.listed().await;
    let campaign = listed
        .iter()
        .find(|c| c["id"] == json!(id))
        .unwrap_or_else(|| panic!("{id} is not listed: {listed:?}"));
    assert_eq!(campaign["name"], "Les Brumes de Corbière");
    assert_eq!(campaign["type"], "campagne");
    assert_eq!(campaign["pitch"], "Un port noyé de brume.");
    assert_eq!(campaign["art"], "forest");
    assert_eq!(campaign["glyph"], "B");
    assert!(
        listed
            .iter()
            .any(|c| c["name"] == "Les Cendres de Valombre"),
        "{listed:?}"
    );
    assert_eq!(scratch.sessions(&id).await, []);
    assert_eq!(scratch.count("session").await, 0);
    scratch.drop().await;
}

#[tokio::test]
async fn oneshot_single_session_a_oneshot_is_created_with_one_session_in_prep_at_position_1_microphone_off()
 {
    let scratch = Scratch::new("oneshot").await;
    let served = scratch.serve().await;
    let id = served
        .created(json!({ "name": "La Nuit du Bal", "type": "oneshot" }))
        .await;
    assert_eq!(
        scratch.campaigns("La Nuit du Bal").await,
        [(id.clone(), "oneshot".to_string(), 1.0)]
    );
    let sessions = scratch.sessions(&id).await;
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    let (_, position, title, status, microphone_on) = &sessions[0];
    assert_eq!(
        (*position, title.as_str(), status.as_str(), *microphone_on),
        (1, "La Nuit du Bal", "prep", false)
    );
    assert!(
        served
            .listed()
            .await
            .iter()
            .any(|c| c["id"] == json!(id) && c["type"] == "oneshot"),
        "the oneshot is listed"
    );
    scratch.drop().await;
}

#[tokio::test]
async fn the_same_idempotency_key_twice_creates_one_campaign_and_one_session() {
    let scratch = Scratch::new("idempotent").await;
    let served = scratch.serve().await;
    let payload = json!({ "name": "Le Bal masqué", "type": "oneshot" });
    let (status, first, settled) = served
        .create(payload.clone(), Some("CampaignList:bal"))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    let (status, second, settled) = served.create(payload, Some("CampaignList:bal")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    assert_eq!(second["commandId"], first["commandId"], "{second}");
    assert!(settled.is_empty(), "nothing more to apply: {settled:?}");
    let campaigns = scratch.campaigns("Le Bal masqué").await;
    assert_eq!(campaigns.len(), 1, "{campaigns:?}");
    assert_eq!(scratch.sessions(&campaigns[0].0).await.len(), 1);
    assert_eq!(scratch.count("session").await, 1);
    scratch.drop().await;
}

#[tokio::test]
async fn master_volume_range_a_new_campaign_s_master_volume_lies_between_0_and_1() {
    let scratch = Scratch::new("volume").await;
    let served = scratch.serve().await;
    for (name, kind) in [("Le Val", "campagne"), ("La Fête", "oneshot")] {
        served
            .created(json!({ "name": name, "type": kind, "soundMasterVolume": 4 }))
            .await;
        let campaigns = scratch.campaigns(name).await;
        assert_eq!(campaigns.len(), 1, "{campaigns:?}");
        let volume = campaigns[0].2;
        assert!((0.0..=1.0).contains(&volume), "{name}: {volume}");
    }
    scratch.drop().await;
}

#[tokio::test]
async fn a_creation_with_no_name_or_of_no_kind_is_refused_422_and_nothing_is_queued() {
    let served = Served::memory().await;
    for payload in [
        json!({ "type": "campagne" }),
        json!({ "name": "  ", "type": "oneshot" }),
        json!({ "name": "Le Val", "type": "saga" }),
    ] {
        let (status, answer, settled) = served.create(payload.clone(), None).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{payload}: {answer}"
        );
        assert!(answer["error"].is_string(), "{answer}");
        assert!(answer.get("commandId").is_none(), "{answer}");
        assert!(settled.is_empty(), "{settled:?}");
    }
}

#[tokio::test]
async fn oneshot_single_session_refuses_a_creation_that_does_not_carry_exactly_the_session_its_kind_has()
 {
    let served = Served::memory().await;
    let id = |n: u8| format!("c2000000-0000-4000-8000-0000000000{n:02}");
    let session = "d2000000-0000-4000-8000-000000000001";
    let mut cases = Vec::new();
    let mut without = oneshot(&id(1), session);
    for field in [
        "sessionId",
        "sessionCampaignId",
        "sessionPosition",
        "sessionTitle",
        "sessionStatus",
        "microphoneOn",
    ] {
        without.as_object_mut().expect("an object").remove(field);
    }
    cases.push((id(1), without));
    for (n, (field, value)) in [
        ("sessionPosition", json!(2)),
        ("sessionStatus", json!("live")),
        ("microphoneOn", json!(true)),
        ("sessionCampaignId", json!(VALOMBRE)),
        ("sessionId", json!("")),
        ("sessionTitle", json!(null)),
    ]
    .into_iter()
    .enumerate()
    {
        let n = u8::try_from(n).expect("a few") + 2;
        let mut payload = oneshot(&id(n), session);
        payload[field] = value;
        cases.push((id(n), payload));
    }
    let mut campagne = oneshot(&id(9), session);
    campagne["kind"] = json!("campagne");
    cases.push((id(9), campagne));
    for (target, payload) in cases {
        let result = served.straight(&target, payload.clone()).await;
        assert_eq!(result.status, State::Rejected, "{payload}: {result:?}");
        assert_eq!(result.violations, ["oneshot-single-session"], "{payload}");
    }
    let mut campagne = oneshot(&id(10), session);
    campagne["kind"] = json!("campagne");
    for field in [
        "sessionId",
        "sessionCampaignId",
        "sessionPosition",
        "sessionTitle",
        "sessionStatus",
        "microphoneOn",
    ] {
        campagne.as_object_mut().expect("an object").remove(field);
    }
    for (target, payload) in [(id(10), campagne), (id(11), oneshot(&id(11), session))] {
        let result = served.straight(&target, payload.clone()).await;
        assert_eq!(result.status, State::Applied, "{payload}: {result:?}");
    }
}

#[tokio::test]
async fn master_volume_range_refuses_a_creation_outside_0_to_1() {
    let served = Served::memory().await;
    let mut payload = oneshot(
        "c3000000-0000-4000-8000-000000000001",
        "d3000000-0000-4000-8000-000000000001",
    );
    payload["soundMasterVolume"] = json!(1.5);
    let result = served
        .straight("c3000000-0000-4000-8000-000000000001", payload)
        .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["master-volume-range"], "{result:?}");
}

#[tokio::test]
async fn over_the_wire_the_same_idempotency_key_twice_is_one_command_applied_once() {
    let served = Served::memory().await;
    let payload = json!({ "name": "Le Bal masqué", "type": "oneshot" });
    let (status, first, settled) = served
        .create(payload.clone(), Some("CampaignList:bal"))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    assert_eq!(settled.len(), 1, "{first} {settled:?}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    let (status, second, settled) = served.create(payload, Some("CampaignList:bal")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    assert_eq!(second["commandId"], first["commandId"], "{second}");
    assert_eq!(second["partition"], first["partition"], "{second}");
    assert!(settled.is_empty(), "nothing more to apply: {settled:?}");
    let (_, other, settled) = served
        .create(json!({ "name": "Le Bal masqué", "type": "oneshot" }), None)
        .await;
    assert_ne!(
        other["partition"], first["partition"],
        "a creation with no key is another campaign"
    );
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
}

#[tokio::test]
async fn a_creation_the_tables_lost_is_written_again_with_its_session_at_the_next_start() {
    let scratch = Scratch::new("repair").await;
    let id = {
        let served = scratch.serve().await;
        served
            .created(json!({ "name": "La Foire", "type": "oneshot", "glyph": "F" }))
            .await
    };
    let session = scratch.sessions(&id).await;
    scratch
        .client
        .batch_execute(&format!("DELETE FROM campaign WHERE id = '{id}'"))
        .await
        .expect("the projection lost");
    assert_eq!(scratch.count("session").await, 0, "cascaded");
    let served = scratch.serve().await;
    assert_eq!(scratch.campaigns("La Foire").await.len(), 1);
    assert_eq!(
        scratch.sessions(&id).await,
        session,
        "the same session again"
    );
    assert!(
        served
            .listed()
            .await
            .iter()
            .any(|c| c["id"] == json!(id) && c["glyph"] == "F"),
        "listed again"
    );
    // A start again writes nothing twice.
    drop(scratch.serve().await);
    assert_eq!(scratch.count("campaign").await, 2);
    assert_eq!(scratch.count("session").await, 1);
    scratch.drop().await;
}

#[tokio::test]
async fn a_oneshot_s_session_deleted_since_never_comes_back_at_a_start() {
    let scratch = Scratch::new("deleted").await;
    let id = {
        let served = scratch.serve().await;
        served
            .created(json!({ "name": "La Kermesse", "type": "oneshot" }))
            .await
    };
    assert_eq!(scratch.sessions(&id).await.len(), 1);
    scratch
        .client
        .batch_execute(&format!("DELETE FROM session WHERE campaign_id = '{id}'"))
        .await
        .expect("the session deleted");
    drop(scratch.serve().await);
    assert_eq!(scratch.campaigns("La Kermesse").await.len(), 1);
    assert_eq!(
        scratch.sessions(&id).await,
        [],
        "the campaign is held already"
    );
    scratch.drop().await;
}
