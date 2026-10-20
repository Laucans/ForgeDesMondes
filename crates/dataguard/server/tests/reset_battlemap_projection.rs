//! table.resetBattlemap over PostgreSQL (`DATABASE_URL`): « Réinitialiser la
//! carte », once applied, leaves the instance with no token, no combat and
//! no initiative entry, and its `reset_at` stamped — each row a tombstone the
//! engine keeps on its partition (a soft delete), the foreign keys following
//! the aggregate's relations — while the instance's row, its battlemap's,
//! its grid's and every row of another instance of the same battlemap stay
//! as they were (`instance-isolated`). What the instance holds is read
//! before the reset is queued, the queue's rows with the tables': nothing to
//! wipe, or no instance, queues nothing. A reset the tables lost is made
//! again at the next start. Each test works in a scratch database of its own,
//! migrated by the Data layer, dropped when it ends.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::command::State;
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
const BAGARRE: &str = "5c000000-0000-4000-8000-000000000002";
/// A scene with no instance yet.
const FUITE: &str = "5c000000-0000-4000-8000-000000000003";
const INSTANCE: &str = "b1a00000-0000-4000-8000-000000000001";
/// The instance of another scene, of the same battlemap.
const AUTRE_INSTANCE: &str = "b1a00000-0000-4000-8000-000000000002";
/// A combat of the instance ended before the engine ran: the tables' alone.
const ANCIEN_COMBAT: &str = "c0000000-0000-4000-8000-0000000000aa";

/// An instance with no token, no combat and no entry.
const EMPTY: [Vec<String>; 3] = [Vec::new(), Vec::new(), Vec::new()];

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
            "INSERT INTO character (entity_id, token_color, hp_max, dexterity) \
             VALUES ('{id}', '#888', {hp}, 12);"
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
    /// Migrated by the Data layer, holding two instances of the tavern's
    /// battlemap — one per scene —, the Bandit, an NPC of 11 HP, Vesna, the
    /// bandits' chief, of 22, and an ended combat of the first instance.
    async fn new(name: &str) -> Scratch {
        let name = format!("reset_battlemap_test_{name}_{}", std::process::id());
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
                 INSERT INTO scene (id, session_id, position, title) VALUES
                     ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée'), ('{BAGARRE}', '{SEANCE}', 2, 'Bagarre'),
                     ('{FUITE}', '{SEANCE}', 3, 'Fuite');
                 INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) VALUES
                     ('{INSTANCE}', '{CARTE}', '{SEANCE}', '{ARRIVEE}'),
                     ('{AUTRE_INSTANCE}', '{CARTE}', '{SEANCE}', '{BAGARRE}');
                 INSERT INTO combat (id, instance_id, round, started_at, ended_at)
                 VALUES ('{ANCIEN_COMBAT}', '{INSTANCE}', 3, now() - interval '1 hour', now());",
                element(BANDIT, "pnj", "Bandit", Some(11)),
                element(VESNA, "pnj", "Chef bandit", Some(22)),
                element(TAVERNE, "lieu", "La Taverne du Sanglier", None),
            ))
            .await
            .expect("the tavern's instances");
        let url = naming(&name);
        Scratch { client, name, url }
    }

    /// The engine as the server plants it over this database — repairing
    /// what the tables lost — and its wire protocol.
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

    /// [`Scratch::serve`], with the test-only DataCapability `test.addEntry`
    /// writing an initiative entry whatever its combat.
    async fn serve_writing_entries(&self) -> (Arc<Engine>, Router) {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let manifest = json!({
            "dataCapability": "test.addEntry",
            "owner": "dataguard",
            "version": 1,
            "effect": "insert",
            "target": { "aggregate": "BattlemapInstance", "id": "$initiativeEntryId" },
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

    /// The partitions the reset's deletions are queued on.
    async fn deleted(&self) -> Vec<String> {
        self.client
            .query(
                "SELECT partition FROM dataguard_queue
                 WHERE split_part(data_capability, '@', 1) = 'table.resetBattlemap'
                 ORDER BY partition",
                &[],
            )
            .await
            .expect("the queue")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    /// One JSON text per row of `sql`, whose one column is that text.
    async fn texts(&self, sql: &str, instance: &str) -> Vec<String> {
        self.client
            .query(sql, &[&instance])
            .await
            .expect("the rows")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    /// The tokens, the combats and the entries the tables hold of
    /// `instance`, whole.
    async fn map(&self, instance: &str) -> [Vec<String>; 3] {
        [
            self.texts(
                "SELECT to_jsonb(t)::text FROM token t WHERE instance_id = $1::text::uuid ORDER BY id",
                instance,
            )
            .await,
            self.texts(
                "SELECT to_jsonb(c)::text FROM combat c WHERE instance_id = $1::text::uuid ORDER BY id",
                instance,
            )
            .await,
            self.texts(
                "SELECT to_jsonb(e)::text FROM initiative_entry e
                 JOIN combat c ON c.id = e.combat_id
                 WHERE c.instance_id = $1::text::uuid ORDER BY e.id",
                instance,
            )
            .await,
        ]
    }

    /// The instance's row, the battlemap's and its grid's, whole.
    async fn originals(&self, instance: &str) -> Vec<String> {
        self.texts(
            "SELECT to_jsonb(b)::text FROM battlemap_instance b WHERE id = $1::text::uuid
             UNION ALL SELECT to_jsonb(m)::text FROM media m
                 WHERE m.id = (SELECT battlemap_media_id FROM battlemap_instance WHERE id = $1::text::uuid)
             UNION ALL SELECT to_jsonb(g)::text FROM battlemap_grid g
                 WHERE g.media_id = (SELECT battlemap_media_id FROM battlemap_instance WHERE id = $1::text::uuid)",
            instance,
        )
        .await
    }

    /// The instance's `reset_at`, as text.
    async fn reset_at(&self, instance: &str) -> Option<String> {
        self.client
            .query_one(
                "SELECT reset_at::text FROM battlemap_instance WHERE id = $1::text::uuid",
                &[&instance],
            )
            .await
            .expect("the instance")
            .get(0)
    }

    /// The ids of the tokens, the combats and the entries of `instance`.
    async fn ids(&self, instance: &str) -> Vec<String> {
        self.texts(
            "SELECT id::text FROM token WHERE instance_id = $1::text::uuid
             UNION ALL SELECT id::text FROM combat WHERE instance_id = $1::text::uuid
             UNION ALL SELECT e.id::text FROM initiative_entry e
                 JOIN combat c ON c.id = e.combat_id WHERE c.instance_id = $1::text::uuid",
            instance,
        )
        .await
    }

    /// How many of the engine's rows of `ids` it holds tombstoned, and how
    /// many live.
    async fn tombstones(&self, ids: &[String]) -> (i64, i64) {
        let partitions: Vec<String> = ids
            .iter()
            .map(|id| format!("BattlemapInstance/{id}"))
            .collect();
        let row = self
            .client
            .query_one(
                "SELECT count(*) FILTER (WHERE tombstoned), count(*) FILTER (WHERE NOT tombstoned)
                 FROM dataguard_rows WHERE partition = ANY($1)",
                &[&partitions],
            )
            .await
            .expect("the engine's rows");
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

/// `POST /commands` of `data_capability` on `instance`, by the DM holding
/// `roles`, under the idempotency key `key` when one is given: its status
/// and answer.
async fn sent(
    app: &Router,
    data_capability: &str,
    instance: &str,
    payload: Value,
    roles: Value,
    key: Option<&str>,
) -> (StatusCode, Value) {
    let mut body = json!({
        "dataCapability": data_capability,
        "payload": payload,
        "target": { "id": instance },
        "caller": { "by": "dm", "from": "table.Battlemap", "roles": roles },
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

/// [`sent`] by the DM, the aggregate's approver: the ids a reset deletes
/// are critical fields.
async fn command(
    app: &Router,
    data_capability: &str,
    instance: &str,
    payload: Value,
) -> (StatusCode, Value) {
    sent(
        app,
        data_capability,
        instance,
        payload,
        json!(["role:approver"]),
        None,
    )
    .await
}

/// table.resetBattlemap of `instance`.
async fn reset(app: &Router, instance: &str) -> (StatusCode, Value) {
    command(app, "table.resetBattlemap@1", instance, json!({})).await
}

/// Runs the applier once, every result applied: how many commands it ran.
async fn applied(engine: &Engine) -> usize {
    let results = engine.run_once().await.expect("the applier ran");
    for result in &results {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    results.len()
}

/// `count` Bandits and one Chef bandit dropped on `instance`, queued.
async fn dropped(app: &Router, instance: &str, count: usize) {
    for (character, count) in [(BANDIT, count), (VESNA, 1)] {
        let (status, body) = command(
            app,
            "table.placeTokens@1",
            instance,
            json!({ "character": character, "q": 5, "r": 5, "count": count }),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    }
}

/// The combat of `instance` started, queued.
async fn started(app: &Router, instance: &str) {
    let (status, body) = command(app, "table.startCombat@1", instance, json!({})).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
}

/// Both instances set: three Bandits and the chief in a combat on the first
/// — beside its ended one —, two Bandits and the chief in a combat on the
/// other, every row applied.
async fn tables_set(engine: &Engine, app: &Router) {
    dropped(app, INSTANCE, 3).await;
    dropped(app, AUTRE_INSTANCE, 2).await;
    assert_eq!(applied(engine).await, 7);
    started(app, INSTANCE).await;
    started(app, AUTRE_INSTANCE).await;
    // A combat and an entry per token, each.
    assert_eq!(applied(engine).await, 1 + 4 + 1 + 3);
}

#[tokio::test]
async fn a_reset_leaves_no_token_combat_nor_entry_and_stamps_the_instance_alone() {
    let s = Scratch::new("wipe").await;
    let (engine, app) = s.serve().await;
    tables_set(&engine, &app).await;
    let [tokens, combats, entries] = s.map(INSTANCE).await;
    assert_eq!((tokens.len(), combats.len(), entries.len()), (4, 2, 4));
    let wiped = s.ids(INSTANCE).await;
    let other = s.map(AUTRE_INSTANCE).await;
    let originals = s.originals(INSTANCE).await;
    let other_originals = s.originals(AUTRE_INSTANCE).await;
    assert_eq!(s.reset_at(INSTANCE).await, None);

    let (status, body) = reset(&app, INSTANCE).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["result"]["status"], "queued", "{body}");
    // Its tokens are queued last: the answer is a token's deletion.
    let last = body["partition"].as_str().expect("a partition");
    assert!(
        tokens.iter().any(|token| {
            let token: Value = serde_json::from_str(token).expect("JSON");
            Some(last)
                == token["id"]
                    .as_str()
                    .map(|id| format!("BattlemapInstance/{id}"))
                    .as_deref()
        }),
        "{last}"
    );
    // One deletion per row: 4 tokens, 2 combats — the ended one too — and 4
    // entries.
    assert_eq!(applied(&engine).await, 10);

    // state-persisted-reset: no token, no combat, no entry; reset_at set.
    assert_eq!(s.map(INSTANCE).await, EMPTY);
    assert!(s.reset_at(INSTANCE).await.is_some());
    // A soft delete: the engine keeps each row it held, tombstoned — the
    // ended combat, which only the tables held, has no row of the engine.
    assert_eq!(s.tombstones(&wiped).await, (10, 0));
    // instance-isolated: the instance's row, but its stamp, its battlemap,
    // its grid, and the other instance of the battlemap, as they were.
    let stamped: Vec<String> = s
        .originals(INSTANCE)
        .await
        .into_iter()
        .map(|row| {
            let mut row: Value = serde_json::from_str(&row).expect("JSON");
            if row.get("reset_at").is_some() {
                row["reset_at"] = Value::Null;
            }
            row.to_string()
        })
        .collect();
    let unstamped: Vec<String> = originals
        .iter()
        .map(|row| {
            serde_json::from_str::<Value>(row)
                .expect("JSON")
                .to_string()
        })
        .collect();
    assert_eq!(stamped, unstamped);
    assert_eq!(s.map(AUTRE_INSTANCE).await, other);
    assert_eq!(s.originals(AUTRE_INSTANCE).await, other_originals);
    assert_eq!(s.reset_at(AUTRE_INSTANCE).await, None);
    s.drop().await;
}

#[tokio::test]
async fn rows_queued_and_not_applied_are_wiped_too_and_a_map_wiped_takes_tokens_again() {
    let s = Scratch::new("queued").await;
    let (engine, app) = s.serve().await;
    // Dropped and started, nothing applied yet: the reset queues behind.
    dropped(&app, INSTANCE, 2).await;
    let (status, body) = reset(&app, INSTANCE).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // The 3 tokens, then their 3 deletions, which no author confirms (an
    // overwrite: what the rows hold by then goes), and the ended combat's.
    assert_eq!(applied(&engine).await, 3 + 3 + 1);
    assert_eq!(s.map(INSTANCE).await, EMPTY);
    let first = s.reset_at(INSTANCE).await.expect("stamped");
    // A wiped map is a map: tokens are dropped on it again, and a second
    // reset replaces the stamp, later.
    dropped(&app, INSTANCE, 1).await;
    assert_eq!(applied(&engine).await, 2);
    assert_eq!(s.map(INSTANCE).await[0].len(), 2);
    let (status, body) = reset(&app, INSTANCE).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 2);
    assert_eq!(s.map(INSTANCE).await[0].len(), 0);
    let second = s.reset_at(INSTANCE).await.expect("stamped");
    assert!(second > first, "{first} then {second}");
    s.drop().await;
}

#[tokio::test]
async fn nothing_to_wipe_no_instance_or_no_uuid_is_refused_and_nothing_is_queued() {
    let s = Scratch::new("refused").await;
    let (engine, app) = s.serve().await;
    for (instance, expected, error) in [
        (
            AUTRE_INSTANCE,
            StatusCode::CONFLICT,
            "la carte n'a rien à réinitialiser",
        ),
        (
            "b1a00000-0000-4000-8000-0000000000ff",
            StatusCode::NOT_FOUND,
            "aucune instance de carte de ce nom",
        ),
        (
            "taverne",
            StatusCode::UNPROCESSABLE_ENTITY,
            "instance n'est pas un identifiant",
        ),
    ] {
        let (status, body) = reset(&app, instance).await;
        assert_eq!(status, expected, "{instance} {body}");
        assert_eq!(body, json!({ "error": error }), "{instance}");
    }
    assert_eq!(applied(&engine).await, 0);
    assert_eq!(s.reset_at(AUTRE_INSTANCE).await, None);
    s.drop().await;
}

#[tokio::test]
async fn a_caller_who_is_no_approver_has_each_deletion_reviewed_first() {
    let s = Scratch::new("review").await;
    let (engine, app) = s.serve().await;
    dropped(&app, INSTANCE, 1).await;
    assert_eq!(applied(&engine).await, 2);
    started(&app, INSTANCE).await;
    // A combat and an entry per token.
    assert_eq!(applied(&engine).await, 3);
    let (status, body) = sent(
        &app,
        "table.resetBattlemap@1",
        INSTANCE,
        json!({}),
        json!([]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["result"]["status"], "awaiting_review", "{body}");
    // Held until an approver decides: the applier runs nothing, the map
    // stays and the instance is not stamped.
    assert_eq!(applied(&engine).await, 0);
    let [tokens, combats, entries] = s.map(INSTANCE).await;
    assert_eq!((tokens.len(), combats.len(), entries.len()), (2, 1, 2));
    assert_eq!(s.reset_at(INSTANCE).await, None);
    // The held deletion of the combat may be refused: it is still open, and
    // none other starts (one-active-combat).
    let (status, body) = command(&app, "table.startCombat@1", INSTANCE, json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    s.drop().await;
}

#[tokio::test]
async fn a_reset_whose_last_deletion_is_withdrawn_stamps_nothing_until_a_reset_wipes_the_map() {
    let s = Scratch::new("withdrawn").await;
    let (engine, app) = s.serve().await;
    dropped(&app, INSTANCE, 1).await;
    assert_eq!(applied(&engine).await, 2);
    let (status, body) = reset(&app, INSTANCE).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // The answer is the last deletion's: withdrawn, a token stays.
    let cancel = Request::post(format!(
        "/commands/{}/cancel",
        body["commandId"].as_str().unwrap()
    ))
    .header("content-type", "application/json")
    .body(Body::from(json!({ "by": "dm" }).to_string()))
    .unwrap();
    let response = app.clone().oneshot(cancel).await.expect("an answer");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(applied(&engine).await, 1);
    assert_eq!(s.map(INSTANCE).await[0].len(), 1);
    // The first deletion applied: state-persisted-reset holds not yet.
    assert_eq!(s.reset_at(INSTANCE).await, None);
    let (status, body) = reset(&app, INSTANCE).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 1);
    assert_eq!(s.map(INSTANCE).await[0].len(), 0);
    assert!(s.reset_at(INSTANCE).await.is_some());
    s.drop().await;
}

#[tokio::test]
async fn a_reset_sent_again_under_its_key_is_replayed_and_queues_nothing_more() {
    let s = Scratch::new("replay").await;
    let (engine, app) = s.serve().await;
    dropped(&app, INSTANCE, 1).await;
    assert_eq!(applied(&engine).await, 2);
    let send = || {
        sent(
            &app,
            "table.resetBattlemap@1",
            INSTANCE,
            json!({}),
            json!(["role:approver"]),
            Some("reset-1"),
        )
    };
    let (status, first) = send().await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let (status, again) = send().await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["commandId"], first["commandId"], "{again}");
    assert_eq!(again["replayed"], true, "{again}");
    // The two tokens and the ended combat, once.
    assert_eq!(applied(&engine).await, 3);
    s.drop().await;
}

#[tokio::test]
async fn a_reset_the_tables_lost_is_made_again_at_the_next_start() {
    let s = Scratch::new("repair").await;
    let lost = {
        let (engine, app) = s.serve().await;
        tables_set(&engine, &app).await;
        let before = s.map(INSTANCE).await;
        let (status, body) = reset(&app, INSTANCE).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(applied(&engine).await, 10);
        before
    };
    let stamp = s.reset_at(INSTANCE).await.expect("stamped");
    let other = s.map(AUTRE_INSTANCE).await;
    // The tables lose the reset: a token, a combat started by the engine and
    // the stamp come back.
    let [tokens, combats, _] = lost;
    let token: Value = serde_json::from_str(&tokens[0]).expect("JSON");
    let combat = combats
        .iter()
        .map(|row| serde_json::from_str::<Value>(row).expect("JSON"))
        .find(|row| row["id"] != ANCIEN_COMBAT)
        .expect("the engine's combat");
    s.client
        .batch_execute(&format!(
            "INSERT INTO token (id, instance_id, character_id, number, name, q, r, visible,
                 hp_current, hp_max, conditions)
             SELECT id, instance_id, character_id, number, name, q, r, visible, hp_current, hp_max,
                 conditions
             FROM jsonb_populate_record(NULL::token, '{token}');
             INSERT INTO combat (id, instance_id, round, started_at)
             VALUES ('{}', '{INSTANCE}', 1, now());
             UPDATE battlemap_instance SET reset_at = NULL WHERE id = '{INSTANCE}';",
            combat["id"].as_str().expect("an id"),
        ))
        .await
        .expect("a reset lost");
    assert_eq!(s.ids(INSTANCE).await.len(), 2);
    let _ = s.serve().await;
    assert_eq!(s.map(INSTANCE).await, EMPTY);
    assert_eq!(s.reset_at(INSTANCE).await, Some(stamp));
    assert_eq!(s.map(AUTRE_INSTANCE).await, other);
    s.drop().await;
}

#[tokio::test]
async fn each_deletion_but_the_last_is_keyed_by_its_row_so_a_retry_half_queued_finds_its_own() {
    let s = Scratch::new("keys").await;
    let (engine, app) = s.serve().await;
    dropped(&app, INSTANCE, 1).await;
    assert_eq!(applied(&engine).await, 2);
    let (status, body) = sent(
        &app,
        "table.resetBattlemap@1",
        INSTANCE,
        json!({}),
        json!(["role:approver"]),
        Some("k"),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let keys: Vec<(String, Option<String>)> = s
        .client
        .query(
            "SELECT partition, idempotency_key FROM dataguard_queue
             WHERE split_part(data_capability, '@', 1) = 'table.resetBattlemap'
             ORDER BY position",
            &[],
        )
        .await
        .expect("the queue")
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    assert_eq!(keys.len(), 3, "{keys:?}");
    let (last, others) = keys.split_last().expect("deletions");
    assert_eq!(last.1.as_deref(), Some("k"), "{keys:?}");
    for (partition, key) in others {
        let id = partition.trim_start_matches("BattlemapInstance/");
        assert_eq!(key.as_deref(), Some(format!("k#{id}").as_str()), "{keys:?}");
    }
    assert_eq!(applied(&engine).await, 3);
    s.drop().await;
}

#[tokio::test]
async fn while_its_deletion_is_queued_a_combat_is_none_to_end_and_a_new_one_may_start() {
    let s = Scratch::new("restart").await;
    let (engine, app) = s.serve().await;
    dropped(&app, INSTANCE, 1).await;
    assert_eq!(applied(&engine).await, 2);
    started(&app, INSTANCE).await;
    assert_eq!(applied(&engine).await, 3);
    let (status, body) = reset(&app, INSTANCE).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // Queued, not applied: no end is written behind the combat's tombstone.
    let (status, body) = command(&app, "table.endCombat@1", INSTANCE, json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    // Two tokens dropped again and a new combat started after the reset:
    // they stay once it applied, the old ones go.
    dropped(&app, INSTANCE, 1).await;
    started(&app, INSTANCE).await;
    // 2 tokens, 2 entries and 2 combats deleted — the ended one too —, then
    // 2 tokens, their 2 entries and the combat.
    assert_eq!(applied(&engine).await, 6 + 5);
    let [tokens, combats, entries] = s.map(INSTANCE).await;
    assert_eq!((tokens.len(), combats.len(), entries.len()), (2, 1, 2));
    s.drop().await;
}

#[tokio::test]
async fn an_instance_whose_creation_is_queued_is_one_with_nothing_to_wipe_and_no_other_insert_makes_one()
 {
    let s = Scratch::new("created").await;
    let (engine, app) = s.serve().await;
    // A drop queued on the first instance: an insert, of another partition.
    dropped(&app, INSTANCE, 1).await;
    let (status, body) = command(
        &app,
        "table.createBattlemapInstance@1",
        INSTANCE,
        json!({ "session": SEANCE, "scene": FUITE, "battlemap": CARTE }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let created = body["partition"]
        .as_str()
        .and_then(|partition| partition.strip_prefix("BattlemapInstance/"))
        .expect("the instance's partition")
        .to_string();
    // Queued, not applied: the tables hold it not, the queue does.
    let (status, body) = reset(&app, &created).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body,
        json!({ "error": "la carte n'a rien à réinitialiser" })
    );
    let (status, body) = reset(&app, "b1a00000-0000-4000-8000-0000000000ff").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    // The drop's 2 tokens and the creation, and no deletion.
    assert_eq!(applied(&engine).await, 3);
    assert_eq!(s.deleted().await, Vec::<String>::new());
    s.drop().await;
}

#[tokio::test]
async fn an_entry_queued_of_a_token_of_the_instance_is_wiped_whatever_its_combat() {
    let s = Scratch::new("stray").await;
    let (engine, app) = s.serve_writing_entries().await;
    dropped(&app, INSTANCE, 1).await;
    assert_eq!(applied(&engine).await, 2);
    let token = s
        .client
        .query_one("SELECT id::text FROM token WHERE name = 'Bandit 1'", &[])
        .await
        .expect("the token")
        .get::<_, String>(0);
    // An entry of the Bandit's in a combat no table nor queue holds, queued.
    let stray = "1e000000-0000-4000-8000-0000000000aa";
    let (status, body) = command(
        &app,
        "test.addEntry",
        INSTANCE,
        json!({
            "initiativeEntryId": stray,
            "initiativeEntryCombatId": "c0000000-0000-4000-8000-0000000000ff",
            "initiativeEntryTokenId": token,
            "roll": 10,
            "initiative": 11,
            "dexScore": 12,
            "tiebreak": 0.5,
            "rank": 1,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let (status, body) = reset(&app, INSTANCE).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert!(
        s.deleted()
            .await
            .contains(&format!("BattlemapInstance/{stray}")),
        "{:?}",
        s.deleted().await
    );
    s.drop().await;
}
