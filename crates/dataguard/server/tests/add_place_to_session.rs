//! session.addPlaceToSession over the wire: a place of the session's
//! campaign joins a scene of the session — the one named, else its first —
//! after the scene's last element, once; every medium of that place is then
//! the session's through `session_media_view`, never stored
//! (`session-media-by-place`); a place of another campaign, or a scene of
//! another session, is rejected (`scene-links-same-campaign`).
//!
//! What the engine judges and queues — the refusals, the verdict, the rank, a
//! link asked twice — is proved over its in-memory store, the session, its
//! scenes and the places written as the engine's rows. What reaches the
//! tables is proved over PostgreSQL (`DATABASE_URL`), each test in a scratch
//! database of its own, migrated as the server migrates it, holding the
//! fixtures' rows no command wrote, dropped when it ends.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, Reads, Rows, planted, router_reading};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's session 4, in preparation, and its two scenes.
const SEANCE_4: &str = "51000000-0000-4000-8000-000000000004";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const ROUTE: &str = "5c000000-0000-4000-8000-000000000002";
/// Valombre's session 3, and its scene.
const SEANCE_3: &str = "51000000-0000-4000-8000-000000000003";
const PROLOGUE: &str = "5c000000-0000-4000-8000-000000000003";
/// Valombre's places, a PNJ of Valombre, Brume's place.
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000003";
const TOUR: &str = "e1000000-0000-4000-8000-000000000004";
const VESNA: &str = "e1000000-0000-4000-8000-000000000005";
const PHARE: &str = "e1000000-0000-4000-8000-000000000009";
/// The tavern's battlemap and illustration, the tower's battlemap, a medium
/// with no place.
const CARTE: &str = "3e000000-0000-4000-8000-000000000001";
const ENSEIGNE: &str = "3e000000-0000-4000-8000-000000000002";
const DONJON: &str = "3e000000-0000-4000-8000-000000000003";
const BROUILLARD: &str = "3e000000-0000-4000-8000-000000000004";

/// What the tests write straight on the engine's rows, the rows a session or
/// a scene holds: no wire prepares them.
const WRITE_SESSION: &str = r#"{"dataCapability":"test.writeSession","owner":"dataguard","version":1,"effect":"upsert","target":{"aggregate":"Session","id":"$id"},"touches":[],"payload":{},"mode":"overwrite","invariants":[],"permissions":[],"callableBy":["*"],"idempotencyKey":"optional"}"#;

/// The engine as the server plants it, its rows, and its wire protocol.
struct Served {
    engine: Arc<Engine>,
    rows: Rows,
    app: Router,
}

fn object(value: Value) -> serde_json::Map<String, Value> {
    let Value::Object(value) = value else {
        panic!("an object");
    };
    value
}

/// The wire protocol over `planted`, with the test-only DataCapability.
fn served(planted: Planted, reads: Reads) -> Served {
    let Planted { engine, rows } = planted;
    let manifest = DataCapability::parse(WRITE_SESSION).expect("a manifest");
    let engine = Arc::new(engine.with_data_capability(manifest));
    let app = router_reading(
        engine.clone(),
        rows.clone(),
        Arc::new(ports_llm::fake::FakeLlm::new()),
        Arc::new(reads),
    );
    Served { engine, rows, app }
}

impl Served {
    /// The engine over its in-memory store, as `DATABASE_URL=memory` plants
    /// it, holding Valombre's sessions 3 and 4, their scenes — the arrival
    /// linking the tower already — and the places of Valombre and Brume.
    async fn memory() -> Served {
        let s = served(
            planted(MEMORY).await.expect("the engine in memory"),
            Reads::none(),
        );
        for (session, campaign) in [(SEANCE_4, VALOMBRE), (SEANCE_3, VALOMBRE)] {
            s.written(
                "test.writeSession",
                session,
                json!({ "sessionCampaignId": campaign }),
            )
            .await;
        }
        for (scene, session, rank) in [
            (ARRIVEE, SEANCE_4, 1),
            (ROUTE, SEANCE_4, 2),
            (PROLOGUE, SEANCE_3, 1),
        ] {
            s.written(
                "test.writeSession",
                scene,
                json!({ "sceneId": scene, "sceneSessionId": session, "scenePosition": rank }),
            )
            .await;
        }
        for (element, campaign, kind, name) in [
            (TAVERNE, VALOMBRE, "lieu", "La Taverne du Sanglier"),
            (TOUR, VALOMBRE, "lieu", "La Tour d'Ébène"),
            (VESNA, VALOMBRE, "pnj", "Vesna"),
            (PHARE, BRUME, "lieu", "Le Phare"),
        ] {
            let (status, answer) = s
                .post(
                    "/commands",
                    json!({
                        "dataCapability": "campaign.saveEntity@1",
                        "payload": {
                            "campaign": campaign,
                            "entity": { "id": element, "kind": kind, "name": name },
                        },
                        "caller": { "by": "dm", "from": "EntitySheet", "roles": ["role:approver"] },
                    }),
                )
                .await;
            assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
            let settled = s.engine.run_once().await.expect("the applier ran");
            assert_eq!(settled[0].status, State::Applied, "{name} {settled:?}");
        }
        s
    }

    /// `payload` submitted straight to the engine as `data_capability` on
    /// `target`, bypassing the wire's preparation, by an approver: what the
    /// applier made of it.
    async fn straight(&self, data_capability: &str, target: &str, payload: Value) -> CommandResult {
        self.engine
            .submit(Submission {
                data_capability: format!("{data_capability}@1"),
                payload: object(payload).into_iter().collect(),
                target_id: Some(target.to_string()),
                based_on: None,
                idempotency_key: None,
                caller: Caller {
                    by: "dm".to_string(),
                    from: "SceneBoard".to_string(),
                    permissions: Vec::new(),
                    roles: vec!["role:approver".to_string()],
                },
            })
            .await
            .expect("queued");
        let mut settled = self.engine.run_once().await.expect("the applier ran");
        assert_eq!(settled.len(), 1, "{settled:?}");
        settled.remove(0)
    }

    /// The same, applied.
    async fn written(&self, data_capability: &str, target: &str, payload: Value) {
        let result = self
            .straight(data_capability, target, payload.clone())
            .await;
        assert_eq!(result.status, State::Applied, "{payload} {result:?}");
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

    /// `payload` submitted as the Médias page sends it — its target the
    /// session — by the DM, who approves the links' critical fields: the
    /// answer, and the commands one pass of the applier settled.
    async fn add(
        &self,
        payload: Value,
        key: Option<&str>,
    ) -> (StatusCode, Value, Vec<CommandResult>) {
        let mut body = json!({
            "dataCapability": "session.addPlaceToSession@1",
            "payload": payload,
            "target": { "aggregate": "Session", "id": payload["session"] },
            "caller": { "by": "dm", "from": "MediaLibrary", "roles": ["role:approver"] },
        });
        if let Some(key) = key {
            body["idempotencyKey"] = json!(key);
        }
        let (status, answer) = self.post("/commands", body).await;
        let settled = self.engine.run_once().await.expect("the applier ran");
        (status, answer, settled)
    }

    /// `payload` added and settled, accepted: its result and its partition.
    async fn added(&self, payload: Value) -> (CommandResult, String) {
        let (status, answer, mut settled) = self.add(payload, None).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        assert_eq!(settled.len(), 1, "{answer} {settled:?}");
        let partition = answer["partition"]
            .as_str()
            .expect("a partition")
            .to_owned();
        (settled.remove(0), partition)
    }

    /// The rank the engine's row of the link of `place` to `scene` holds.
    async fn rank(&self, scene: &str, place: &str) -> Value {
        let partition = format!("Session/{SEANCE_4}:{scene}:{place}");
        let row = self.rows.projected(&partition).await.expect("the row");
        row.get("sceneEntityPosition")
            .cloned()
            .unwrap_or(Value::Null)
    }
}

#[tokio::test]
async fn the_place_joins_the_scene_named_after_its_last_element_on_its_own_partition() {
    let s = Served::memory().await;
    let (result, partition) = s
        .added(json!({ "session": SEANCE_4, "scene": ARRIVEE, "place": TAVERNE }))
        .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(partition, format!("Session/{SEANCE_4}:{ARRIVEE}:{TAVERNE}"));
    assert_eq!(s.rank(ARRIVEE, TAVERNE).await, json!(1));
    // A second place of the same scene goes after it.
    let (result, partition) = s
        .added(json!({ "session": SEANCE_4, "scene": ARRIVEE, "place": TOUR.to_uppercase() }))
        .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(partition, format!("Session/{SEANCE_4}:{ARRIVEE}:{TOUR}"));
}

#[tokio::test]
async fn two_places_queued_for_a_scene_before_the_applier_runs_take_two_ranks() {
    let s = Served::memory().await;
    for place in [TAVERNE, TOUR] {
        let (status, answer) = s
            .post(
                "/commands",
                json!({
                    "dataCapability": "session.addPlaceToSession@1",
                    "payload": { "session": SEANCE_4, "scene": ROUTE, "place": place },
                    "caller": { "by": "dm", "from": "SceneBoard", "roles": ["role:approver"] },
                }),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    }
    let settled = s.engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 2, "{settled:?}");
    assert!(
        settled.iter().all(|r| r.status == State::Applied),
        "{settled:?}"
    );
    assert_eq!(s.rank(ROUTE, TAVERNE).await, json!(1));
    assert_eq!(s.rank(ROUTE, TOUR).await, json!(2));
}

#[tokio::test]
async fn a_place_already_linked_to_the_scene_asked_again_is_the_same_single_link() {
    let s = Served::memory().await;
    let payload = json!({ "session": SEANCE_4, "scene": ROUTE, "place": TAVERNE });
    let (first, partition) = s.added(payload.clone()).await;
    assert_eq!(first.status, State::Applied, "{first:?}");
    let (again, same) = s.added(payload).await;
    assert_eq!(again.status, State::Applied, "{again:?}");
    assert_eq!(same, partition, "one link, one row");
    assert_eq!(s.rank(ROUTE, TAVERNE).await, json!(1), "its rank kept");
}

#[tokio::test]
async fn the_same_idempotency_key_twice_is_one_command_applied_once() {
    let s = Served::memory().await;
    let payload = json!({ "session": SEANCE_4, "scene": ARRIVEE, "place": TAVERNE });
    let (status, first, settled) = s.add(payload.clone(), Some("MediaLibrary:taverne")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    let (status, second, settled) = s.add(payload, Some("MediaLibrary:taverne")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    assert_eq!(second["commandId"], first["commandId"], "{second}");
    assert!(settled.is_empty(), "nothing more to apply: {settled:?}");
}

#[tokio::test]
async fn scene_links_same_campaign_rejects_a_place_of_another_campaign() {
    let s = Served::memory().await;
    let (result, _) = s
        .added(json!({ "session": SEANCE_4, "scene": ARRIVEE, "place": PHARE }))
        .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(
        result.violations,
        ["scene-links-same-campaign"],
        "{result:?}"
    );
}

#[tokio::test]
async fn scene_links_same_campaign_rejects_a_scene_of_another_session() {
    let s = Served::memory().await;
    let (result, _) = s
        .added(json!({ "session": SEANCE_4, "scene": PROLOGUE, "place": TAVERNE }))
        .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(
        result.violations,
        ["scene-links-same-campaign"],
        "{result:?}"
    );
}

#[tokio::test]
async fn scene_links_same_campaign_rejects_what_nobody_knows() {
    let s = Served::memory().await;
    for payload in [
        // A place, a session, a scene nobody holds.
        json!({ "session": SEANCE_4, "scene": ARRIVEE, "place": "e1000000-0000-4000-8000-0000000000ff" }),
        json!({ "session": "51000000-0000-4000-8000-0000000000ff", "scene": ARRIVEE, "place": TAVERNE }),
        json!({ "session": SEANCE_4, "scene": "5c000000-0000-4000-8000-0000000000ff", "place": TAVERNE }),
    ] {
        let (result, _) = s.added(payload.clone()).await;
        assert_eq!(result.status, State::Rejected, "{payload} {result:?}");
        assert_eq!(
            result.violations,
            ["scene-links-same-campaign"],
            "{payload}"
        );
    }
}

#[tokio::test]
async fn scene_links_same_campaign_rejects_a_link_submitted_straight_to_the_engine() {
    let s = Served::memory().await;
    let link = format!("{SEANCE_4}:{ARRIVEE}:{TAVERNE}");
    let result = s
        .straight(
            "session.addPlaceToSession",
            &link,
            json!({ "sceneEntitySceneId": ARRIVEE, "entityId": TAVERNE, "sceneEntityPosition": 1 }),
        )
        .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(
        result.violations,
        ["scene-links-same-campaign"],
        "{result:?}"
    );
}

#[tokio::test]
async fn a_link_naming_no_session_no_place_or_no_scene_to_join_is_refused_422_and_not_queued() {
    let s = Served::memory().await;
    for (payload, says) in [
        (json!({ "scene": ARRIVEE, "place": TAVERNE }), "séance"),
        (
            json!({ "session": SEANCE_4, "scene": ARRIVEE, "place": "  " }),
            "lieu",
        ),
        // Over the engine's memory, no table finds the session's first scene.
        (json!({ "session": SEANCE_4, "place": TAVERNE }), "scène"),
        (
            json!({ "session": SEANCE_4, "scene": ARRIVEE, "place": VESNA }),
            "lieu",
        ),
    ] {
        let (status, answer, settled) = s.add(payload.clone(), None).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{payload} {answer}"
        );
        assert!(
            answer["error"].as_str().is_some_and(|e| e.contains(says)),
            "{payload} {answer}"
        );
        assert!(settled.is_empty(), "nothing queued: {settled:?}");
    }
}

// --- Over PostgreSQL --------------------------------------------------------

const MIGRATIONS: [&str; 5] = [
    include_str!("../../../data/migrations/0002_campaign.sql"),
    include_str!("../../../data/migrations/0003_player.sql"),
    include_str!("../../../data/migrations/0004_entity.sql"),
    include_str!("../../../data/migrations/0005_media.sql"),
    include_str!("../../../data/migrations/0006_session.sql"),
];

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

/// The SQL of an element of `campaign` with its first version
/// (`every-change-versioned`).
fn element(id: &str, campaign: &str, kind: &str, name: &str) -> String {
    format!(
        "INSERT INTO entity (id, campaign_id, type, name) VALUES ('{id}', '{campaign}', '{kind}', '{name}'); \
         INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
         VALUES ('{id}', 1, 'dm', '{{}}', '[]');"
    )
}

/// The SQL of the grid a battlemap has from its upload
/// (`grid-only-for-battlemap`).
fn grid(media: &str) -> String {
    format!(
        "INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) \
         VALUES ('{media}', 70, 12.5, 0, 'pointe', '#202020', 0.5);"
    )
}

/// A database of its own, migrated, holding what the fixtures wrote:
/// Valombre and Brume, Valombre's sessions 3 and 4 and their scenes — the
/// arrival linking the tower already — the places, the PNJ, the media.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("add_place_to_session_test_{name}_{}", std::process::id());
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
                "INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                     ('{BRUME}', 'Les Brumes de Corbière', 'campagne');
                 {}
                 {}
                 {}
                 {}
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                     ('{SEANCE_3}', '{VALOMBRE}', 3, 'Le prologue', 'closed', false),
                     ('{SEANCE_4}', '{VALOMBRE}', 4, 'La foire', 'prep', false);
                 INSERT INTO scene (id, session_id, position, title) VALUES
                     ('{ARRIVEE}', '{SEANCE_4}', 1, 'L''arrivée'),
                     ('{ROUTE}', '{SEANCE_4}', 2, 'La route'),
                     ('{PROLOGUE}', '{SEANCE_3}', 1, 'Le prologue');
                 INSERT INTO scene_entity (scene_id, entity_id, position) VALUES
                     ('{ARRIVEE}', '{TOUR}', 1);
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id) VALUES
                     ('{CARTE}', '{VALOMBRE}', 'battlemap', 'La salle', 'salle.png', 1400, 1000, '{TAVERNE}'),
                     ('{ENSEIGNE}', '{VALOMBRE}', 'illustration', 'L''enseigne', 'enseigne.png', 1400, 1000, '{TAVERNE}'),
                     ('{DONJON}', '{VALOMBRE}', 'battlemap', 'Le donjon', 'donjon.png', 1400, 1000, '{TOUR}'),
                     ('{BROUILLARD}', '{VALOMBRE}', 'illustration', 'Le brouillard', 'brouillard.png', 1400, 1000, NULL);
                 {}
                 {}",
                element(TAVERNE, VALOMBRE, "lieu", "La Taverne du Sanglier"),
                element(TOUR, VALOMBRE, "lieu", "La Tour d''Ébène"),
                element(VESNA, VALOMBRE, "pnj", "Vesna"),
                element(PHARE, BRUME, "lieu", "Le Phare"),
                grid(CARTE),
                grid(DONJON),
            ))
            .await
            .expect("the fixtures");
        Scratch { name, url, client }
    }

    async fn serve(&self) -> Served {
        let planted = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let reads = Reads::open(&self.url).await.expect("the reads open");
        served(planted, reads)
    }

    /// The places `scene` links, by rank: `(place, rank)`.
    async fn links(&self, scene: &str) -> Vec<(String, i32)> {
        self.client
            .query(
                "SELECT entity_id::text, position FROM scene_entity
                 WHERE scene_id = ($1::text)::uuid ORDER BY position, entity_id",
                &[&scene],
            )
            .await
            .expect("the links")
            .iter()
            .map(|row| (row.get(0), row.get(1)))
            .collect()
    }

    /// The media `session_media_view` attaches to `session`, and the place
    /// each one is attached through.
    async fn media(&self, session: &str) -> Vec<(String, Option<String>)> {
        self.client
            .query(
                "SELECT media_id::text, via_place_id::text FROM session_media_view
                 WHERE session_id = ($1::text)::uuid ORDER BY media_id",
                &[&session],
            )
            .await
            .expect("the session's media")
            .iter()
            .map(|row| (row.get(0), row.get(1)))
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

#[tokio::test]
async fn session_media_by_place_the_tavern_joins_the_scene_at_the_next_rank_and_its_two_media_follow()
 {
    let scratch = Scratch::new("media").await;
    let s = scratch.serve().await;
    assert_eq!(
        scratch.media(SEANCE_4).await,
        [(DONJON.to_owned(), Some(TOUR.to_owned()))],
        "only the tower is in session 4"
    );
    let (result, _) = s
        .added(json!({ "session": SEANCE_4, "scene": ARRIVEE, "place": TAVERNE }))
        .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        scratch.links(ARRIVEE).await,
        [(TOUR.to_owned(), 1), (TAVERNE.to_owned(), 2)]
    );
    let tavern = Some(TAVERNE.to_owned());
    assert_eq!(
        scratch.media(SEANCE_4).await,
        [
            (CARTE.to_owned(), tavern.clone()),
            (ENSEIGNE.to_owned(), tavern),
            (DONJON.to_owned(), Some(TOUR.to_owned())),
        ],
        "the battlemap and the illustration of the tavern, never the medium with no place"
    );
    scratch.drop().await;
}

#[tokio::test]
async fn with_no_scene_named_the_place_joins_the_session_s_first() {
    let scratch = Scratch::new("first").await;
    let s = scratch.serve().await;
    let (result, partition) = s
        .added(json!({ "session": SEANCE_4, "place": TAVERNE }))
        .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(partition, format!("Session/{SEANCE_4}:{ARRIVEE}:{TAVERNE}"));
    assert_eq!(
        scratch.links(ARRIVEE).await,
        [(TOUR.to_owned(), 1), (TAVERNE.to_owned(), 2)]
    );
    assert!(scratch.links(ROUTE).await.is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn a_place_the_scene_links_already_keeps_its_single_link_and_its_rank() {
    let scratch = Scratch::new("twice").await;
    let s = scratch.serve().await;
    // The fixtures linked the tower; then the tavern, twice.
    for place in [TOUR, TAVERNE, TAVERNE] {
        let (result, _) = s
            .added(json!({ "session": SEANCE_4, "scene": ARRIVEE, "place": place }))
            .await;
        assert_eq!(result.status, State::Applied, "{place} {result:?}");
    }
    assert_eq!(
        scratch.links(ARRIVEE).await,
        [(TOUR.to_owned(), 1), (TAVERNE.to_owned(), 2)]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn scene_links_same_campaign_leaves_the_tables_as_they_were() {
    let scratch = Scratch::new("refused").await;
    let s = scratch.serve().await;
    for (scene, place) in [(ARRIVEE, PHARE), (PROLOGUE, TAVERNE)] {
        let (result, _) = s
            .added(json!({ "session": SEANCE_4, "scene": scene, "place": place }))
            .await;
        assert_eq!(result.status, State::Rejected, "{result:?}");
        assert_eq!(result.violations, ["scene-links-same-campaign"]);
    }
    assert_eq!(scratch.links(ARRIVEE).await, [(TOUR.to_owned(), 1)]);
    assert!(scratch.links(PROLOGUE).await.is_empty());
    // A PNJ is no place: refused before it is queued.
    let (status, answer, _) = s
        .add(
            json!({ "session": SEANCE_4, "scene": ARRIVEE, "place": VESNA }),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    scratch.drop().await;
}

#[tokio::test]
async fn a_link_queued_before_a_start_is_judged_again_and_one_the_table_lost_is_added_again() {
    let scratch = Scratch::new("start").await;
    {
        let s = scratch.serve().await;
        let (status, answer) = s
            .post(
                "/commands",
                json!({
                    "dataCapability": "session.addPlaceToSession@1",
                    "payload": { "session": SEANCE_4, "scene": ROUTE, "place": TAVERNE },
                    "caller": { "by": "dm", "from": "MediaLibrary", "roles": ["role:approver"] },
                }),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        // The server stops before its applier runs.
    }
    let s = scratch.serve().await;
    let settled = s.engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    assert_eq!(scratch.links(ROUTE).await, [(TAVERNE.to_owned(), 1)]);
    drop(s);

    scratch
        .client
        .batch_execute(&format!(
            "DELETE FROM scene_entity WHERE scene_id = '{ROUTE}'"
        ))
        .await
        .expect("the projection lost");
    drop(scratch.serve().await);
    assert_eq!(scratch.links(ROUTE).await, [(TAVERNE.to_owned(), 1)]);
    scratch.drop().await;
}
