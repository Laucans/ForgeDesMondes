//! table.addParticipant over PostgreSQL (`DATABASE_URL`): reinforcements
//! join a running combat and fall into their place of the order. An NPC rolls
//! 1 to 20 and adds its DEX modifier, a PC takes the DM's figure; the
//! newcomer's entry reaches `initiative_entry` once applied, and every
//! participant it lands ahead of moves down one rank — in the engine's row of
//! its entry, and in the table — so the carousel (`initiative_order_view`)
//! reads the order Initiative@1 gives, the turn and the round untouched. Two
//! joining before the applier ran compose; a token in the combat already, or
//! an instance with no open combat, is refused, nothing queued; an entry the
//! table lost is projected again at the next start. Each test works in a
//! scratch database of its own, migrated by the Data layer, dropped when it
//! ends. The combat is set in the tables, as a launch (`table.startCombat`)
//! leaves it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::State;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000002";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000003";
const VESNA: &str = "e1000000-0000-4000-8000-000000000004";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000005";
const CARTE: &str = "6d000000-0000-4000-8000-000000000001";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1a00000-0000-4000-8000-000000000001";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

fn migrations() -> PathBuf {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/migrations"
    ))
    .to_path_buf()
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

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// The SQL adding element `id` of Valombre, with the version every element
/// is written with, and its sheet — its DEX and its HP max — for a character.
fn element(id: &str, kind: &str, name: &str, dex: Option<i16>, hp_max: Option<i16>) -> String {
    let sheet = if kind == "lieu" {
        String::new()
    } else {
        let or_null = |n: Option<i16>| n.map_or("NULL".to_string(), |n| n.to_string());
        format!(
            "INSERT INTO character (entity_id, token_color, dexterity, hp_max)
             VALUES ('{id}', '#888', {}, {});",
            or_null(dex),
            or_null(hp_max)
        )
    };
    format!(
        "INSERT INTO entity (id, campaign_id, type, name) VALUES ('{id}', '{VALOMBRE}', '{kind}', '{name}'); \
         {sheet} \
         INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
         VALUES ('{id}', 1, 'dm', '{{}}', '[]');"
    )
}

/// One place of the carousel: the token's name, its entry's rank, roll,
/// initiative, DEX and draw, and its place.
#[derive(Debug, Clone, PartialEq)]
struct Place {
    name: String,
    rank: i32,
    roll: Option<i16>,
    initiative: i16,
    dex: Option<i16>,
    draw: f64,
    index: i32,
}

/// A database of its own.
struct Scratch {
    client: Client,
    name: String,
    url: String,
}

impl Scratch {
    /// Migrated by the Data layer, holding the tavern's instance; Elowen
    /// (DEX 18) and Kael (DEX 14), PCs present at the session; the Bandit
    /// (DEX 12) and Vesna, the bandits' chief (DEX 16), NPCs.
    async fn new(name: &str) -> Scratch {
        let name = format!("add_participant_test_{name}_{}", std::process::id());
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let mut client = connect(config.clone().dbname(&name)).await;
        data::migrate(&mut client, &migrations())
            .await
            .expect("migrated by the Data layer");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 {}{}{}{}{}
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id)
                 VALUES ('{CARTE}', '{VALOMBRE}', 'battlemap', 'Taverne', 'carte.png', 2048, 1536, '{TAVERNE}');
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
                 VALUES ('{CARTE}', 64, 0, 0, 'pointe', '#202020', 0.4);
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                 VALUES ('{SEANCE}', '{VALOMBRE}', 1, 'Le Sanglier', 'live', true);
                 INSERT INTO session_present_character (session_id, character_id)
                 VALUES ('{SEANCE}', '{ELOWEN}'), ('{SEANCE}', '{KAEL}');
                 INSERT INTO scene (id, session_id, position, title) VALUES ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée');
                 INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id)
                 VALUES ('{INSTANCE}', '{CARTE}', '{SEANCE}', '{ARRIVEE}');",
                element(ELOWEN, "pj", "Elowen", Some(18), None),
                element(KAEL, "pj", "Kael", Some(14), None),
                element(BANDIT, "pnj", "Bandit", Some(12), Some(11)),
                element(VESNA, "pnj", "Chef bandit", Some(16), Some(22)),
                element(TAVERNE, "lieu", "La Taverne du Sanglier", None, None),
            ))
            .await
            .expect("the tavern's instance");
        let url = naming(&name);
        Scratch { client, name, url }
    }

    /// The engine as the server plants it over this database — repairing
    /// what the tables lost — with the test-only DataCapabilities that write
    /// a combat's row and an entry's, and its wire protocol.
    async fn serve(&self) -> (Arc<Engine>, Router) {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let mut engine = engine;
        for (name, id) in [
            ("test.setCombat", "combatId"),
            ("test.setEntry", "initiativeEntryId"),
        ] {
            let manifest = json!({
                "dataCapability": name,
                "owner": "dataguard",
                "version": 1,
                "effect": "upsert",
                "target": { "aggregate": "BattlemapInstance", "id": format!("${id}") },
                "mode": "overwrite",
                "callableBy": ["*"],
            });
            engine = engine.with_data_capability(
                DataCapability::parse(&manifest.to_string()).expect("contract F"),
            );
        }
        let engine = Arc::new(engine);
        let app = router(
            engine.clone(),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        (engine, app)
    }

    /// The token named `name`'s id.
    async fn token(&self, name: &str) -> String {
        self.client
            .query_one("SELECT id::text FROM token WHERE name = $1", &[&name])
            .await
            .expect("the token")
            .get(0)
    }

    /// The combat as a launch leaves it, at round 2, the turn on Bandit 1:
    /// Elowen at 25, Bandit 2 at 3 (rolled 2), Bandit 1 at 2 (rolled 1) —
    /// below every roll the chief can make, above none of Elowen.
    async fn combat(&self) {
        let elowen = self.token("Elowen").await;
        let bandit_1 = self.token("Bandit 1").await;
        let bandit_2 = self.token("Bandit 2").await;
        self.client
            .batch_execute(&format!(
                "INSERT INTO combat (id, instance_id, round, started_at)
                 VALUES ('{COMBAT}', '{INSTANCE}', 2, now());
                 INSERT INTO initiative_entry (combat_id, token_id, roll, initiative, dex_score, tiebreak, rank)
                 VALUES ('{COMBAT}', '{elowen}', NULL, 25, 18, 40, 1),
                        ('{COMBAT}', '{bandit_2}', 2, 3, 12, 60, 2),
                        ('{COMBAT}', '{bandit_1}', 1, 2, 12, 70, 3);
                 UPDATE combat SET current_entry_id =
                     (SELECT id FROM initiative_entry WHERE token_id = '{bandit_1}')
                 WHERE id = '{COMBAT}';"
            ))
            .await
            .expect("the combat");
    }

    /// The carousel of the combat, in order.
    async fn carousel(&self) -> Vec<Place> {
        self.client
            .query(
                "SELECT t.name, e.rank, e.roll, e.initiative, e.dex_score, e.tiebreak, o.order_index
                 FROM initiative_order_view o
                 JOIN initiative_entry e ON e.id = o.entry_id
                 JOIN token t ON t.id = e.token_id
                 WHERE o.combat_id = $1::text::uuid ORDER BY o.order_index",
                &[&COMBAT],
            )
            .await
            .expect("the carousel")
            .iter()
            .map(|row| Place {
                name: row.get(0),
                rank: row.get(1),
                roll: row.get(2),
                initiative: row.get(3),
                dex: row.get(4),
                draw: row.get(5),
                index: row.get(6),
            })
            .collect()
    }

    /// The combat's round, and the name of the token whose turn it is.
    async fn turn(&self) -> (i32, Option<String>) {
        let row = self
            .client
            .query_one(
                "SELECT c.round, t.name FROM combat c
                 LEFT JOIN initiative_entry e ON e.id = c.current_entry_id
                 LEFT JOIN token t ON t.id = e.token_id
                 WHERE c.id = $1::text::uuid",
                &[&COMBAT],
            )
            .await
            .expect("the combat");
        (row.get(0), row.get(1))
    }

    /// The rank the engine holds of the entry of the token named `name`.
    async fn held_rank(&self, name: &str) -> Option<i64> {
        let token = self.token(name).await;
        self.client
            .query_opt(
                "SELECT (fields ->> 'rank')::bigint FROM dataguard_rows
                 WHERE partition = 'BattlemapInstance/' || lower(fields ->> 'initiativeEntryId')
                     AND lower(fields ->> 'initiativeEntryTokenId') = $1",
                &[&token],
            )
            .await
            .expect("the engine's row")
            .map(|row| row.get(0))
    }

    async fn drop(self) {
        drop(self.client);
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        let sql = format!("DROP DATABASE {} WITH (FORCE)", self.name);
        for _ in 0..20 {
            match admin.batch_execute(&sql).await {
                Ok(()) => return,
                Err(e) if e.code() == Some(&SqlState::INSUFFICIENT_PRIVILEGE) => {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                Err(e) => panic!("the scratch database dropped: {e:?}"),
            }
        }
        panic!("the scratch database {} is still in use", self.name);
    }
}

/// `POST /commands` of `data_capability` on the instance, by the DM: its
/// status and answer.
async fn command(app: &Router, data_capability: &str, payload: Value) -> (StatusCode, Value) {
    keyed(app, data_capability, payload, None).await
}

/// [`command`], under the idempotency key `key` when one is given.
async fn keyed(
    app: &Router,
    data_capability: &str,
    payload: Value,
    key: Option<&str>,
) -> (StatusCode, Value) {
    let mut body = json!({
        "dataCapability": data_capability,
        "payload": payload,
        "target": { "id": INSTANCE },
        "caller": { "by": "dm", "from": "table.Battlemap", "roles": ["role:approver"] },
    });
    if let Some(key) = key {
        body["idempotencyKey"] = json!(key);
    }
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
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

/// Runs the applier once, every result applied: how many commands it ran.
async fn applied(engine: &Engine) -> usize {
    let results = engine.run_once().await.expect("the applier ran");
    for result in &results {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    results.len()
}

/// Elowen, Kael, Bandit 1, 2 and 3 and Chef bandit 1 dropped on the
/// instance and applied, then the combat set.
async fn ambush(scratch: &Scratch, engine: &Engine, app: &Router) {
    for (character, count) in [(ELOWEN, 1), (KAEL, 1), (BANDIT, 3), (VESNA, 1)] {
        let (status, body) = command(
            app,
            "table.placeTokens@1",
            json!({ "character": character, "q": 5, "r": 5, "count": count }),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(applied(engine).await, count);
    }
    scratch.combat().await;
}

/// `row` written by the test-only DataCapability `data_capability`, by the
/// DM, queued and not applied.
async fn queue(engine: &Engine, data_capability: &str, row: Value) {
    let Value::Object(payload) = row else {
        panic!("a payload is an object");
    };
    engine
        .submit(Submission {
            data_capability: data_capability.to_string(),
            payload: payload.into_iter().collect(),
            target_id: None,
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "table.battlemap".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
}

/// `token` joining the combat: the answer's status.
async fn join(app: &Router, token: &str, initiative: Option<i64>) -> (StatusCode, Value) {
    let mut payload = json!({ "token": token });
    if let Some(initiative) = initiative {
        payload["initiative"] = json!(initiative);
    }
    command(app, "table.addParticipant@1", payload).await
}

/// `places` in the order Initiative@1 gives them: initiative descending,
/// then DEX descending, none last, then the draw descending — the order
/// given kept on a full tie.
fn by_initiative(places: &[Place]) -> Vec<String> {
    let mut sorted = places.to_vec();
    sorted.sort_by(|a, b| {
        b.initiative
            .cmp(&a.initiative)
            .then_with(|| match (a.dex, b.dex) {
                (Some(a), Some(b)) => b.cmp(&a),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
            .then_with(|| b.draw.total_cmp(&a.draw))
    });
    sorted.into_iter().map(|place| place.name).collect()
}

fn names(places: &[Place]) -> Vec<&str> {
    places.iter().map(|place| place.name.as_str()).collect()
}

/// Whether the ranks and the places read 1, 2, 3… in order.
fn contiguous(places: &[Place]) -> bool {
    places
        .iter()
        .zip(1..)
        .all(|(place, n)| place.rank == n && place.index == n)
}

#[tokio::test]
async fn an_npc_joining_rolls_1_to_20_adds_its_dex_and_lands_in_its_place() {
    let scratch = Scratch::new("npc").await;
    let (engine, app) = scratch.serve().await;
    ambush(&scratch, &engine, &app).await;
    let chef = scratch.token("Chef bandit 1").await;
    let (status, body) = join(&app, &chef, None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // The chief's entry, and Bandit 2 and Bandit 1 moved down.
    assert_eq!(applied(&engine).await, 3);
    let carousel = scratch.carousel().await;
    assert_eq!(
        names(&carousel),
        ["Elowen", "Chef bandit 1", "Bandit 2", "Bandit 1"]
    );
    assert!(contiguous(&carousel), "{carousel:?}");
    let joined = &carousel[1];
    let roll = joined.roll.expect("an NPC rolls");
    assert!((1..=20).contains(&roll), "{joined:?}");
    // DEX 16: +3 (npc-initiative-bounds).
    assert_eq!(joined.initiative, roll + 3);
    assert_eq!(joined.dex, Some(16));
    // Drawn on table.startCombat's scale, 0 to 2^31 - 1.
    assert!((0.0..2_147_483_648.0).contains(&joined.draw), "{joined:?}");
    // The others keep everything but their rank.
    assert_eq!(
        (carousel[2].roll, carousel[2].initiative, carousel[2].draw),
        (Some(2), 3, 60.0)
    );
    // The engine holds the moves, as the table does.
    assert_eq!(scratch.held_rank("Bandit 2").await, Some(3));
    assert_eq!(scratch.held_rank("Bandit 1").await, Some(4));
    assert_eq!(scratch.turn().await, (2, Some("Bandit 1".to_string())));
    drop(engine);
    scratch.drop().await;
}

#[tokio::test]
async fn two_joining_before_the_applier_runs_compose_into_one_order() {
    let scratch = Scratch::new("compose").await;
    let (engine, app) = scratch.serve().await;
    ambush(&scratch, &engine, &app).await;
    // The chief first, applied; then Bandit 3 and Kael, queued together:
    // each lands among what the queue will leave, moves stepping the same
    // entries compose.
    let chef = scratch.token("Chef bandit 1").await;
    assert_eq!(join(&app, &chef, None).await.0, StatusCode::ACCEPTED);
    applied(&engine).await;
    let bandit_3 = scratch.token("Bandit 3").await;
    let kael = scratch.token("Kael").await;
    let (status, body) = join(&app, &bandit_3, None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let (status, body) = join(&app, &kael, Some(11)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    applied(&engine).await;
    let carousel = scratch.carousel().await;
    assert_eq!(carousel.len(), 6, "{carousel:?}");
    assert!(contiguous(&carousel), "{carousel:?}");
    assert_eq!(names(&carousel), by_initiative(&carousel));
    let kael = carousel
        .iter()
        .find(|place| place.name == "Kael")
        .expect("Kael");
    // A PC rolls nothing: the DM's figure, its sheet's DEX.
    assert_eq!((kael.roll, kael.initiative, kael.dex), (None, 11, Some(14)));
    assert_eq!(scratch.turn().await, (2, Some("Bandit 1".to_string())));
    drop(engine);
    scratch.drop().await;
}

#[tokio::test]
async fn a_token_in_the_combat_or_an_instance_with_none_running_is_refused() {
    let scratch = Scratch::new("refused").await;
    let (engine, app) = scratch.serve().await;
    ambush(&scratch, &engine, &app).await;
    let elowen = scratch.token("Elowen").await;
    let (status, body) = join(&app, &elowen, Some(20)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    // Queued and not applied yet, it is in the combat too — but sent again
    // under its key, it is the engine's to replay, as it was.
    let chef = scratch.token("Chef bandit 1").await;
    let joining = json!({ "token": chef });
    let (status, first) = keyed(&app, "table.addParticipant@1", joining.clone(), Some("k")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    assert_eq!(join(&app, &chef, None).await.0, StatusCode::CONFLICT);
    let (status, again) = keyed(&app, "table.addParticipant@1", joining, Some("k")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["replayed"], true, "{again}");
    assert_eq!(again["commandId"], first["commandId"]);
    assert_eq!(applied(&engine).await, 3);
    // A PC sends the DM's figure; an NPC rolls its own.
    let kael = scratch.token("Kael").await;
    assert_eq!(
        join(&app, &kael, None).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let bandit_3 = scratch.token("Bandit 3").await;
    assert_eq!(
        join(&app, &bandit_3, Some(15)).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    // The combat ended: none running.
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE combat SET ended_at = now(), current_entry_id = NULL WHERE id = '{COMBAT}'"
        ))
        .await
        .expect("the combat ended");
    let (status, body) = join(&app, &bandit_3, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(applied(&engine).await, 0);
    assert_eq!(scratch.carousel().await.len(), 4);
    drop(engine);
    scratch.drop().await;
}

#[tokio::test]
async fn a_combat_whose_end_the_engine_holds_queued_or_applied_is_none_to_join() {
    let scratch = Scratch::new("ended").await;
    let (engine, app) = scratch.serve().await;
    ambush(&scratch, &engine, &app).await;
    // The table still runs the combat; the engine's row of it says it ended.
    queue(
        &engine,
        "test.setCombat",
        json!({
            "combatId": COMBAT,
            "combatInstanceId": INSTANCE,
            "round": 1,
            "startedAt": "2026-10-20T20:05:00Z",
            "endedAt": "2026-10-20T21:00:00Z",
        }),
    )
    .await;
    let chef = scratch.token("Chef bandit 1").await;
    let (status, body) = join(&app, &chef, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "queued: {body}");
    assert_eq!(applied(&engine).await, 1);
    let (status, body) = join(&app, &chef, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "applied: {body}");
    assert_eq!(applied(&engine).await, 0);
    drop(engine);
    scratch.drop().await;
}

#[tokio::test]
async fn an_entry_of_the_combat_that_cannot_be_read_lets_no_one_join() {
    let scratch = Scratch::new("unread").await;
    let (engine, app) = scratch.serve().await;
    ambush(&scratch, &engine, &app).await;
    // Queued, an entry of the combat that names its token and no rank: the
    // newcomer is ranked around no participant blindly.
    let kael = scratch.token("Kael").await;
    queue(
        &engine,
        "test.setEntry",
        json!({
            "initiativeEntryId": "1e000000-0000-4000-8000-000000000042",
            "initiativeEntryCombatId": COMBAT,
            "initiativeEntryTokenId": kael,
        }),
    )
    .await;
    let chef = scratch.token("Chef bandit 1").await;
    let (status, body) = join(&app, &chef, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    drop(engine);
    scratch.drop().await;
}

#[tokio::test]
async fn an_entry_and_a_rank_the_table_lost_are_projected_again_at_the_next_start() {
    let scratch = Scratch::new("repair").await;
    let (engine, app) = scratch.serve().await;
    ambush(&scratch, &engine, &app).await;
    let chef = scratch.token("Chef bandit 1").await;
    assert_eq!(join(&app, &chef, None).await.0, StatusCode::ACCEPTED);
    applied(&engine).await;
    let before = scratch.carousel().await;
    drop(engine);
    scratch
        .client
        .batch_execute(&format!(
            "DELETE FROM initiative_entry WHERE token_id = '{chef}';
             UPDATE initiative_entry SET rank = 2
                 WHERE token_id = (SELECT id FROM token WHERE name = 'Bandit 2');"
        ))
        .await
        .expect("the table loses them");
    let (engine, _app) = scratch.serve().await;
    assert_eq!(scratch.carousel().await, before);
    drop(engine);
    scratch.drop().await;
}

#[tokio::test]
async fn an_entry_only_the_table_holds_is_moved_whole_then_stepped_never_overwritten() {
    let scratch = Scratch::new("tabled").await;
    let (engine, app) = scratch.serve().await;
    ambush(&scratch, &engine, &app).await;
    // Kael at 3, DEX 14: ahead of Bandit 2 (3, DEX 12), whose entry and
    // Bandit 1's only the table holds — each written whole, one rank down.
    let kael = scratch.token("Kael").await;
    let (status, body) = join(&app, &kael, Some(3)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 3);
    assert_eq!(scratch.held_rank("Bandit 2").await, Some(3));
    assert_eq!(scratch.held_rank("Bandit 1").await, Some(4));
    // The chief, at 4 or more: ahead of Kael and both bandits, each now
    // held by the engine — stepped from the rank it holds, not set anew.
    let chef = scratch.token("Chef bandit 1").await;
    let (status, body) = join(&app, &chef, None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 4);
    let carousel = scratch.carousel().await;
    assert_eq!(
        names(&carousel),
        ["Elowen", "Chef bandit 1", "Kael", "Bandit 2", "Bandit 1"]
    );
    assert!(contiguous(&carousel), "{carousel:?}");
    // What the table alone held of them is kept, field by field.
    let kept = |place: &Place| (place.roll, place.initiative, place.dex, place.draw);
    assert_eq!(kept(&carousel[0]), (None, 25, Some(18), 40.0));
    assert_eq!(kept(&carousel[3]), (Some(2), 3, Some(12), 60.0));
    assert_eq!(kept(&carousel[4]), (Some(1), 2, Some(12), 70.0));
    // The newcomer's entry, on a partition of its own, lands as sent.
    assert_eq!(scratch.held_rank("Chef bandit 1").await, Some(2));
    assert_eq!(scratch.held_rank("Kael").await, Some(3));
    assert_eq!(scratch.held_rank("Bandit 2").await, Some(4));
    assert_eq!(scratch.held_rank("Bandit 1").await, Some(5));
    // Elowen, ahead of both, was never written.
    assert_eq!(scratch.held_rank("Elowen").await, None);
    assert_eq!(scratch.turn().await, (2, Some("Bandit 1".to_string())));
    drop(engine);
    scratch.drop().await;
}

#[tokio::test]
async fn a_start_and_a_join_sent_together_seat_no_token_twice_nor_two_at_one_rank() {
    let scratch = Scratch::new("race").await;
    let (engine, app) = scratch.serve().await;
    // The tokens dropped, no combat yet.
    for (character, count) in [(ELOWEN, 1), (KAEL, 1), (BANDIT, 3), (VESNA, 1)] {
        let placing = json!({ "character": character, "q": 5, "r": 5, "count": count });
        assert_eq!(
            command(&app, "table.placeTokens@1", placing).await.0,
            StatusCode::ACCEPTED
        );
        applied(&engine).await;
    }
    let chef = scratch.token("Chef bandit 1").await;
    let bandit_3 = scratch.token("Bandit 3").await;
    let starting = json!({ "initiative": { ELOWEN: 18, KAEL: 9 } });
    let (started, chef_joined, bandit_joined) = tokio::join!(
        command(&app, "table.startCombat@1", starting),
        join(&app, &chef, None),
        join(&app, &bandit_3, None),
    );
    assert_eq!(started.0, StatusCode::ACCEPTED, "{}", started.1);
    // Each join is judged before the start or after it, never in between:
    // no open combat, or the token seated by the start.
    for (status, body) in [chef_joined, bandit_joined] {
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
    }
    applied(&engine).await;
    let seats = scratch
        .client
        .query(
            "SELECT e.token_id::text, e.rank FROM initiative_entry e
             JOIN combat c ON c.id = e.combat_id
             WHERE c.instance_id = $1::text::uuid ORDER BY e.rank",
            &[&INSTANCE],
        )
        .await
        .expect("the entries");
    let tokens: Vec<String> = seats.iter().map(|row| row.get(0)).collect();
    let ranks: Vec<i32> = seats.iter().map(|row| row.get(1)).collect();
    let mut distinct = tokens.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), 6, "{tokens:?}");
    assert_eq!(tokens.len(), 6, "a token seated twice: {tokens:?}");
    assert_eq!(ranks, (1..=6).collect::<Vec<i32>>(), "{ranks:?}");
    drop(engine);
    scratch.drop().await;
}
