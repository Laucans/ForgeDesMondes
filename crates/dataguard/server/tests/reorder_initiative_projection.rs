//! table.reorderInitiative over PostgreSQL (`DATABASE_URL`): the DM's order
//! of the instance's open combat rewrites every participant's rank, 1 to n
//! without gap or duplicate (`initiative-tiebreak`), a hidden one taking the
//! rank it is given (`hidden-keeps-rank`); an order that omits, repeats or
//! adds a participant is refused and nothing is queued. Once applied, the
//! ranks are `initiative_entry.rank` and the carousel's places follow them
//! (`initiative_order_view`); the turn does not move, and a rank the table
//! lost is written again at the next start. Each test works in a scratch
//! database of its own, migrated by the Data layer, dropped when it ends.
//!
//! The tokens are dropped by `table.placeTokens`; the combat and its entries
//! are written by a test-only DataCapability, and laid in the tables as a
//! start's projection lays them: a start (`table.startCombat`) is another
//! task's.

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
const BANDIT: &str = "e1000000-0000-4000-8000-000000000003";
const VESNA: &str = "e1000000-0000-4000-8000-000000000004";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000005";
const CARTE: &str = "6d000000-0000-4000-8000-000000000001";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1a00000-0000-4000-8000-000000000001";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
/// The entries of the four participants, ranked 1 to 4.
const ENTRIES: [&str; 4] = [
    "1e000000-0000-4000-8000-000000000001",
    "1e000000-0000-4000-8000-000000000002",
    "1e000000-0000-4000-8000-000000000003",
    "1e000000-0000-4000-8000-000000000004",
];
/// The four participants, in their launch order.
const PARTICIPANTS: [&str; 4] = ["Bandit 1", "Bandit 2", "Bandit 3", "Chef bandit 1"];

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
/// is written with, and its sheet for an NPC.
fn element(id: &str, kind: &str, name: &str, hp_max: Option<i16>) -> String {
    let sheet = if kind == "lieu" {
        String::new()
    } else {
        let hp = hp_max.map_or("NULL".to_string(), |hp| hp.to_string());
        format!(
            "INSERT INTO character (entity_id, token_color, hp_max) VALUES ('{id}', '#888', {hp});"
        )
    };
    format!(
        "INSERT INTO entity (id, campaign_id, type, name) VALUES ('{id}', '{VALOMBRE}', '{kind}', '{name}'); \
         {sheet} \
         INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
         VALUES ('{id}', 1, 'dm', '{{}}', '[]');"
    )
}

/// A database of its own.
struct Scratch {
    client: Client,
    name: String,
    url: String,
}

impl Scratch {
    /// Migrated by the Data layer, holding the tavern's instance, the
    /// Bandit, an NPC of 11 HP, and Vesna, the bandits' chief, of 22.
    async fn new(name: &str) -> Scratch {
        let name = format!("reorder_initiative_test_{name}_{}", std::process::id());
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
                 {}{}{}
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id)
                 VALUES ('{CARTE}', '{VALOMBRE}', 'battlemap', 'Taverne', 'carte.png', 2048, 1536, '{TAVERNE}');
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
                 VALUES ('{CARTE}', 64, 0, 0, 'pointe', '#202020', 0.4);
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                 VALUES ('{SEANCE}', '{VALOMBRE}', 1, 'Le Sanglier', 'live', true);
                 INSERT INTO scene (id, session_id, position, title) VALUES ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée');
                 INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id)
                 VALUES ('{INSTANCE}', '{CARTE}', '{SEANCE}', '{ARRIVEE}');",
                element(BANDIT, "pnj", "Bandit", Some(11)),
                element(VESNA, "pnj", "Chef bandit", Some(22)),
                element(TAVERNE, "lieu", "La Taverne du Sanglier", None),
            ))
            .await
            .expect("the tavern's instance");
        let url = naming(&name);
        Scratch { client, name, url }
    }

    /// The engine as the server plants it over this database — repairing
    /// what the tables lost — with the test-only DataCapability that writes
    /// a combat or an entry, and its wire protocol.
    async fn serve(&self) -> (Arc<Engine>, Router) {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let manifest = json!({
            "dataCapability": "test.seedCombat",
            "owner": "dataguard",
            "version": 1,
            "effect": "insert",
            "target": { "aggregate": "BattlemapInstance", "id": "$id" },
            "mode": "overwrite",
            "callableBy": ["*"],
        });
        let engine = Arc::new(engine.with_data_capability(
            DataCapability::parse(&manifest.to_string()).expect("contract F"),
        ));
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

    /// The carousel of the combat: each token's name, rank, place and
    /// whether the players see it, in order.
    async fn carousel(&self) -> Vec<(String, i32, i32, bool)> {
        self.client
            .query(
                "SELECT t.name, e.rank, o.order_index, o.shown_to_players
                 FROM initiative_order_view o
                 JOIN initiative_entry e ON e.id = o.entry_id
                 JOIN token t ON t.id = e.token_id
                 WHERE o.combat_id = $1::text::uuid ORDER BY o.order_index",
                &[&COMBAT],
            )
            .await
            .expect("the carousel")
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3)))
            .collect()
    }

    /// The ranks the engine holds of the entries, in [`ENTRIES`]' order.
    async fn engine_ranks(&self) -> Vec<Value> {
        let mut ranks = Vec::new();
        for entry in ENTRIES {
            let row = self
                .client
                .query_one(
                    "SELECT (fields -> 'rank')::text FROM dataguard_rows WHERE partition = $1",
                    &[&format!("BattlemapInstance/{entry}")],
                )
                .await
                .expect("the entry's row");
            ranks.push(serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"));
        }
        ranks
    }

    /// The combat as `combat` holds it: its round, and its current entry.
    async fn turn(&self) -> (i32, Option<String>) {
        let row = self
            .client
            .query_one(
                "SELECT round, current_entry_id::text FROM combat WHERE id = $1::text::uuid",
                &[&COMBAT],
            )
            .await
            .expect("the combat");
        (row.get(0), row.get(1))
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

/// `POST /commands` of `data_capability` on the instance, by the DM, under
/// `key` when one is given: its status and answer.
async fn sent(
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

async fn command(app: &Router, data_capability: &str, payload: Value) -> (StatusCode, Value) {
    sent(app, data_capability, payload, None).await
}

/// Runs the applier once, every result applied: how many commands it ran.
async fn applied(engine: &Engine) -> usize {
    let results = engine.run_once().await.expect("the applier ran");
    for result in &results {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    results.len()
}

/// `payload` written on `BattlemapInstance/<id>` by the test-only
/// DataCapability, queued.
async fn seed(engine: &Engine, id: &str, payload: Value) {
    let Value::Object(payload) = payload else {
        unreachable!("a payload is an object");
    };
    engine
        .submit(Submission {
            data_capability: "test.seedCombat".to_string(),
            payload: payload.into_iter().collect(),
            target_id: Some(id.to_string()),
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

/// Bandit 1 to 3 and Chef bandit 1 dropped on the instance, in combat in
/// that order at round 1, on the first — ended at `ended` when one is given
/// —, the engine's rows laid in the tables by the server's own projection.
async fn fight(s: &Scratch, engine: &Engine, app: &Router, ended: Option<&str>) {
    for (character, count) in [(BANDIT, 3), (VESNA, 1)] {
        let (status, body) = command(
            app,
            "table.placeTokens@1",
            json!({ "character": character, "q": 5, "r": 5, "count": count }),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(applied(engine).await, count);
    }
    seed(
        engine,
        COMBAT,
        json!({
            "combatId": COMBAT,
            "combatInstanceId": INSTANCE,
            "round": 1,
            "currentEntryId": ENTRIES[0],
            "startedAt": "2026-10-20T20:00:00.000Z",
            "endedAt": ended,
        }),
    )
    .await;
    for ((entry, name), rank) in ENTRIES.iter().zip(PARTICIPANTS).zip(1..) {
        let token = s.token(name).await;
        seed(
            engine,
            entry,
            json!({
                "initiativeEntryId": entry,
                "initiativeEntryCombatId": COMBAT,
                "initiativeEntryTokenId": token,
                "roll": null,
                "initiative": 20 - rank,
                "dexScore": null,
                "tiebreak": 0.5,
                "rank": rank,
            }),
        )
        .await;
    }
    assert_eq!(applied(engine).await, 5);
}

/// The tokens of `names`, by id, in that order.
async fn order(s: &Scratch, names: &[&str]) -> Vec<String> {
    let mut ids = Vec::new();
    for name in names {
        ids.push(s.token(name).await);
    }
    ids
}

/// A carousel's place: the token's name, its rank, its place, shown or not.
fn place(name: &str, rank: i32, shown: bool) -> (String, i32, i32, bool) {
    (name.to_string(), rank, rank, shown)
}

#[tokio::test]
async fn initiative_tiebreak_four_participants_the_fourth_moved_first_are_ranked_1_to_4() {
    let s = Scratch::new("fourth").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    let new = order(&s, &["Chef bandit 1", "Bandit 1", "Bandit 2", "Bandit 3"]).await;
    let (status, body) = command(&app, "table.reorderInitiative@1", json!({ "order": new })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // One command per entry, every rank rewritten.
    assert_eq!(applied(&engine).await, 4);
    assert_eq!(
        s.carousel().await,
        [
            place("Chef bandit 1", 1, true),
            place("Bandit 1", 2, true),
            place("Bandit 2", 3, true),
            place("Bandit 3", 4, true),
        ]
    );
    assert_eq!(
        s.engine_ranks().await,
        [json!(2), json!(3), json!(4), json!(1)]
    );
    // The turn does not move.
    assert_eq!(s.turn().await, (1, Some(ENTRIES[0].to_string())));
    s.drop().await;
}

#[tokio::test]
async fn an_order_that_omits_or_repeats_a_participant_is_refused_and_nothing_is_queued() {
    let s = Scratch::new("refused").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    let before = s.carousel().await;
    for names in [
        vec!["Bandit 3", "Bandit 1", "Bandit 2"],
        vec!["Chef bandit 1", "Bandit 1", "Bandit 2", "Bandit 1"],
    ] {
        let (status, body) = command(
            &app,
            "table.reorderInitiative@1",
            json!({ "order": order(&s, &names).await }),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{names:?} {body}");
        assert_eq!(
            body["error"],
            "l'ordre nomme chaque participant du combat une fois, et eux seuls"
        );
    }
    assert_eq!(applied(&engine).await, 0);
    assert_eq!(s.carousel().await, before);
    s.drop().await;
}

#[tokio::test]
async fn hidden_keeps_rank_a_hidden_participant_takes_the_rank_it_is_given() {
    let s = Scratch::new("hidden").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    let bandit_3 = s.token("Bandit 3").await;
    let (status, body) = command(
        &app,
        "table.setTokenVisibility@1",
        json!({ "token": bandit_3, "visible": false }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 1);
    let new = order(&s, &["Bandit 3", "Chef bandit 1", "Bandit 1", "Bandit 2"]).await;
    let (status, body) = command(&app, "table.reorderInitiative@1", json!({ "order": new })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 4);
    // Hidden, first, and still hidden: the reorder writes no visibility.
    assert_eq!(
        s.carousel().await,
        [
            place("Bandit 3", 1, false),
            place("Chef bandit 1", 2, true),
            place("Bandit 1", 3, true),
            place("Bandit 2", 4, true),
        ]
    );
    s.drop().await;
}

#[tokio::test]
async fn a_token_off_the_map_is_out_of_the_order_and_the_others_close_up() {
    let s = Scratch::new("removed").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    let bandit_2 = s.token("Bandit 2").await;
    let (status, body) = command(&app, "table.removeToken@1", json!({ "token": bandit_2 })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 1);
    // Named still, it is one participant too many.
    let with = order(&s, &["Chef bandit 1", "Bandit 1", "Bandit 3"]).await;
    let mut too_many = with.clone();
    too_many.push(bandit_2);
    let (status, body) = command(
        &app,
        "table.reorderInitiative@1",
        json!({ "order": too_many }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, body) = command(&app, "table.reorderInitiative@1", json!({ "order": with })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 3);
    // The gap the removal left closes up: 1 to 3.
    assert_eq!(
        s.carousel().await,
        [
            place("Chef bandit 1", 1, true),
            place("Bandit 1", 2, true),
            place("Bandit 3", 3, true),
        ]
    );
    s.drop().await;
}

#[tokio::test]
async fn an_instance_with_no_open_combat_is_refused_409() {
    let s = Scratch::new("none").await;
    let (engine, app) = s.serve().await;
    let (status, body) = command(
        &app,
        "table.reorderInitiative@1",
        json!({ "order": [ENTRIES[0]] }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "cette instance n'a pas de combat en cours");
    fight(&s, &engine, &app, Some("2026-10-20T21:00:00.000Z")).await;
    let (status, body) = command(
        &app,
        "table.reorderInitiative@1",
        json!({ "order": order(&s, &PARTICIPANTS).await }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(applied(&engine).await, 0);
    s.drop().await;
}

#[tokio::test]
async fn a_reorder_sent_again_under_its_key_is_the_engine_s_to_replay() {
    let s = Scratch::new("replay").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    let new = order(&s, &["Bandit 2", "Bandit 1", "Bandit 3", "Chef bandit 1"]).await;
    let payload = json!({ "order": new });
    let (status, first) = sent(
        &app,
        "table.reorderInitiative@1",
        payload.clone(),
        Some("k"),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let (status, again) = sent(
        &app,
        "table.reorderInitiative@1",
        payload.clone(),
        Some("k"),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["commandId"], first["commandId"]);
    assert_eq!(applied(&engine).await, 4);
    assert_eq!(
        s.engine_ranks().await,
        [json!(2), json!(1), json!(3), json!(4)]
    );
    // Bandit 3 leaves the map: sent again under its key, the reorder is still
    // the one the engine holds, replayed as it was — not judged anew against
    // the three left, which would refuse it.
    let bandit_3 = s.token("Bandit 3").await;
    let (status, body) = command(&app, "table.removeToken@1", json!({ "token": bandit_3 })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 1);
    let (status, late) = sent(&app, "table.reorderInitiative@1", payload, Some("k")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{late}");
    assert_eq!(late["commandId"], first["commandId"]);
    assert_eq!(applied(&engine).await, 0);
    s.drop().await;
}

#[tokio::test]
async fn a_rank_the_table_lost_is_written_again_at_the_next_start() {
    let s = Scratch::new("repair").await;
    {
        let (engine, app) = s.serve().await;
        fight(&s, &engine, &app, None).await;
        let new = order(&s, &["Bandit 3", "Bandit 2", "Bandit 1", "Chef bandit 1"]).await;
        let (status, body) =
            command(&app, "table.reorderInitiative@1", json!({ "order": new })).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(applied(&engine).await, 4);
    }
    let after = s.carousel().await;
    s.client
        .execute(
            "UPDATE initiative_entry SET rank = 9 WHERE id = $1::text::uuid",
            &[&ENTRIES[2]],
        )
        .await
        .expect("a rank lost");
    assert_ne!(s.carousel().await, after);
    let _ = s.serve().await;
    assert_eq!(s.carousel().await, after);
    assert_eq!(after[0], place("Bandit 3", 1, true));
    s.drop().await;
}

#[tokio::test]
async fn a_removal_queued_on_another_token_leaves_the_others_in_the_order() {
    let s = Scratch::new("queued_removal").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    let bandit_2 = s.token("Bandit 2").await;
    // Queued, not applied: the lineup reads it as the queue will leave it.
    let (status, body) = command(&app, "table.removeToken@1", json!({ "token": bandit_2 })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let rest = order(&s, &["Bandit 3", "Chef bandit 1", "Bandit 1"]).await;
    let (status, body) = command(&app, "table.reorderInitiative@1", json!({ "order": rest })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    s.drop().await;
}

#[tokio::test]
async fn a_reorder_queued_ahead_still_counts_every_participant() {
    let s = Scratch::new("queued_reorder").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    let first = order(&s, &["Bandit 3", "Bandit 2", "Bandit 1", "Chef bandit 1"]).await;
    let (status, body) =
        command(&app, "table.reorderInitiative@1", json!({ "order": first })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // Sent again before the first one is applied: its ranks are queued, not
    // applied, and no participant is lost for it.
    let second = order(&s, &["Chef bandit 1", "Bandit 1", "Bandit 2", "Bandit 3"]).await;
    let (status, body) = command(
        &app,
        "table.reorderInitiative@1",
        json!({ "order": second }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // Judged against the four, not refused: the engine parks its ranks for
    // their author, one per entry, behind the first reorder's.
    assert_eq!(body["result"]["status"], "awaiting_confirmation");
    assert_eq!(applied(&engine).await, 4);
    assert_eq!(
        s.carousel().await,
        [
            place("Bandit 3", 1, true),
            place("Bandit 2", 2, true),
            place("Bandit 1", 3, true),
            place("Chef bandit 1", 4, true),
        ]
    );
    s.drop().await;
}

#[tokio::test]
async fn an_entry_queued_into_another_combat_is_not_a_participant() {
    let s = Scratch::new("queued_move").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    let token = s.token("Chef bandit 1").await;
    // The engine holds the entry in the combat; the queue moves it out.
    seed(
        &engine,
        ENTRIES[3],
        json!({
            "initiativeEntryId": ENTRIES[3],
            "initiativeEntryCombatId": "c0000000-0000-4000-8000-000000000002",
            "initiativeEntryTokenId": token,
            "roll": null,
            "initiative": 16,
            "dexScore": null,
            "tiebreak": 0.5,
            "rank": 4,
        }),
    )
    .await;
    let rest = order(&s, &["Bandit 3", "Bandit 1", "Bandit 2"]).await;
    let (status, body) = command(&app, "table.reorderInitiative@1", json!({ "order": rest })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    s.drop().await;
}
