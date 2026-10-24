//! The Session aggregate's presences over PostgreSQL (`DATABASE_URL`), as
//! the server's store runs its projection: a PC the engine applies present
//! on `Session/<session>.<pc>` reaches `session_present_character`, ticked
//! when its row says, and `session_roster_view` lists it « present »; a
//! character that is no live PC of the session's campaign
//! (`present-only-pj`) or a session the tables do not hold rejects the
//! command whole; a PC present already keeps its one row. The
//! DataCapability here is test-only: each write is the row as given. Each
//! test works in a scratch database of its own, migrated as the server
//! migrates it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use aggregate_session::presence_id;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::Plugins;
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use serde_json::{Value, json};

/// A presence written whole on the row the submission names, test-only.
const TICK: &str = r#"{"dataCapability":"test.tick","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Session"},"mode":"overwrite","callableBy":["*"]}"#;
/// A presence's row tombstoned, test-only: the PC unticked.
const UNTICK: &str = r#"{"dataCapability":"test.untick","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Session"},"mode":"overwrite","callableBy":["*"]}"#;

/// A campagne with its session, and another campaign.
const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's session, Elowen present since its creation, and a session
/// the engine keeps the tombstone of.
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
const OUBLIEE: &str = "51000000-0000-4000-8000-000000000002";
const NOWHERE: &str = "51000000-0000-4000-8000-0000000000ff";
/// Valombre's PCs with their sheets, its NPC, a « pj » of it with no
/// sheet, a PC of it the engine keeps the tombstone of, and Brume's PC.
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000002";
const GARDE: &str = "e1000000-0000-4000-8000-000000000003";
const ESQUISSE: &str = "e1000000-0000-4000-8000-000000000004";
const DISPARU: &str = "e1000000-0000-4000-8000-000000000006";
const MARIN: &str = "e1000000-0000-4000-8000-000000000005";

/// 2026-10-24T18:00:00Z, in milliseconds since the epoch.
const TICKED_AT: u64 = 1_761_328_800_000;

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

/// A database of its own, migrated as the server migrates it, holding — as
/// the fixtures write them — Valombre with its two sessions, Elowen present
/// at the first, and its characters, and Brume with its PC.
struct Scratch {
    name: String,
    url: String,
    client: Client,
    notifier: Arc<MemoryNotifier>,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("session_presences_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let mut client = connect(&url).await;
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
        data::migrate(&mut client, &migrations)
            .await
            .expect("migrated");
        let versions: String = [ELOWEN, KAEL, GARDE, ESQUISSE, DISPARU, MARIN]
            .iter()
            .map(|element| {
                format!(
                    "INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
                     VALUES ('{element}', 1, 'dm', '{{}}', '[]');"
                )
            })
            .collect();
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                     ('{BRUME}', 'La Brume', 'oneshot');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                     ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen'),
                     ('{KAEL}', '{VALOMBRE}', 'pj', 'Kael'),
                     ('{GARDE}', '{VALOMBRE}', 'pnj', 'Le garde'),
                     ('{ESQUISSE}', '{VALOMBRE}', 'pj', 'Une esquisse'),
                     ('{DISPARU}', '{VALOMBRE}', 'pj', 'Le disparu'),
                     ('{MARIN}', '{BRUME}', 'pj', 'Le marin');
                 {versions}
                 INSERT INTO character (entity_id, token_color) VALUES
                     ('{ELOWEN}', '#2a6'), ('{KAEL}', '#26a'), ('{GARDE}', '#a62'),
                     ('{DISPARU}', '#666'), ('{MARIN}', '#6a2');
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                 VALUES
                     ('{SEANCE}', '{VALOMBRE}', 1, 'La séance 1', 'prep', false),
                     ('{OUBLIEE}', '{VALOMBRE}', 2, 'L''oubliée', 'prep', false);
                 INSERT INTO session_present_character (session_id, character_id, added_at)
                 VALUES ('{SEANCE}', '{ELOWEN}', '2026-10-01T20:00:00Z');"
            ))
            .await
            .expect("Valombre, Brume, the sessions and their characters");
        Scratch {
            name,
            url,
            client,
            notifier: Arc::default(),
        }
    }

    /// The engine as the server plants this aggregate over this database:
    /// its projection in the apply transaction. The forgotten session's and
    /// the vanished PC's tombstones are the engine's; what it tells the
    /// authors is `self.notifier`'s.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins { std::iter::once(aggregate_session::plugin()).collect() };
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        self.client
            .batch_execute(&format!(
                "INSERT INTO dataguard_rows (partition, tombstoned) VALUES
                     ('Session/{OUBLIEE}', true), ('Entity/{DISPARU}', true)"
            ))
            .await
            .expect("the tombstones");
        let engine = Engine::new(
            Arc::new(store),
            self.notifier.clone(),
            Arc::new(SystemClock),
        )
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_aggregate(Aggregate::parse(aggregate_session::DOCUMENT).expect("contract I"))
        .with_plugins(plugins())
        .with_data_capability(DataCapability::parse(TICK).expect("contract F"))
        .with_data_capability(DataCapability::parse(UNTICK).expect("contract F"));
        engine.load().await.expect("the engine loads");
        engine
    }

    /// What `sql` answers, each row as JSON text, in order.
    async fn rows(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(&format!("SELECT to_jsonb(t)::text FROM ({sql}) t"), &[])
            .await
            .expect("the rows")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
    }

    /// Every PC present, and since when, in milliseconds since the epoch.
    async fn present(&self) -> Vec<Value> {
        self.rows(
            "SELECT session_id::text AS session, character_id::text AS character,
                (extract(epoch FROM added_at) * 1000)::bigint AS added_at
             FROM session_present_character ORDER BY session_id, character_id",
        )
        .await
    }

    /// Why the last command was rejected, as the authors are told.
    fn reason(&self) -> String {
        self.notifier
            .sent()
            .into_iter()
            .rev()
            .find_map(|message| message.reason)
            .unwrap_or_default()
    }

    /// Drops the scratch database. A role that is no superuser cannot end
    /// an autovacuum worker still on it: it is tried again a moment later.
    async fn drop(self) {
        drop(self.client);
        let admin = connect(&url()).await;
        let sql = format!("DROP DATABASE {} WITH (FORCE)", self.name);
        for _ in 0..20 {
            if admin.batch_execute(&sql).await.is_ok() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        admin
            .batch_execute(&sql)
            .await
            .expect("the scratch database dropped");
    }
}

/// Ticks `character` present at `session`, by the DM, who approves, on
/// their presence's row.
async fn tick(engine: &Engine, session: &str, character: &str) {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "test.tick",
        "targetId": presence_id(session, character),
        "payload": {
            "sessionPresentCharacterSessionId": session,
            "sessionPresentCharacterCharacterId": character,
            "addedAt": TICKED_AT,
        },
        // The critical fields' review is not what these tests judge.
        "caller": { "by": "dm", "from": "SceneBoard", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await.expect("queued");
}

/// What the applier made of the one command queued.
async fn applied(engine: &Engine) -> CommandResult {
    let mut applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    applied.remove(0)
}

#[tokio::test]
async fn a_pc_of_the_campaign_ticked_is_present_since_its_row_says_and_on_the_roster() {
    let scratch = Scratch::new("ticked").await;
    let engine = scratch.serve().await;
    tick(&engine, SEANCE, KAEL).await;
    let result = applied(&engine).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        scratch.present().await[1],
        json!({ "session": SEANCE, "character": KAEL, "added_at": TICKED_AT })
    );
    assert_eq!(
        scratch
            .rows(
                "SELECT character_id::text AS character, via FROM session_roster_view
                 ORDER BY character_id"
            )
            .await,
        [
            json!({ "character": ELOWEN, "via": "present" }),
            json!({ "character": KAEL, "via": "present" }),
        ]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn present_only_pj_rejects_anything_but_a_live_pc_of_the_campaign_whole() {
    let scratch = Scratch::new("strangers").await;
    let engine = scratch.serve().await;
    let before = scratch.present().await;
    for stranger in [GARDE, ESQUISSE, DISPARU, MARIN] {
        tick(&engine, SEANCE, stranger).await;
        let result = applied(&engine).await;
        assert_eq!(result.status, State::Rejected, "{stranger}: {result:?}");
        assert!(
            scratch.reason().starts_with("present-only-pj"),
            "{stranger}: {}",
            scratch.reason()
        );
    }
    assert_eq!(scratch.present().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_session_the_tables_do_not_hold_rejects_the_presence_whole() {
    let scratch = Scratch::new("sessions").await;
    let engine = scratch.serve().await;
    let before = scratch.present().await;
    for session in [OUBLIEE, NOWHERE] {
        tick(&engine, session, KAEL).await;
        let result = applied(&engine).await;
        assert_eq!(result.status, State::Rejected, "{session}: {result:?}");
        assert!(
            scratch
                .reason()
                .starts_with("sessionPresentCharacterSessionId"),
            "{session}: {}",
            scratch.reason()
        );
    }
    assert_eq!(scratch.present().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_pc_present_already_keeps_its_one_row_and_when_it_was_ticked() {
    let scratch = Scratch::new("once").await;
    let engine = scratch.serve().await;
    let before = scratch.present().await;
    tick(&engine, SEANCE, ELOWEN).await;
    let result = applied(&engine).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(scratch.present().await, before);
    scratch.drop().await;
}

/// Unticks `character` at `session`, by the DM, who approves: their
/// presence's row tombstoned.
async fn untick(engine: &Engine, session: &str, character: &str) {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "test.untick",
        "targetId": presence_id(session, character),
        "caller": { "by": "dm", "from": "SceneBoard", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await.expect("queued");
}

#[tokio::test]
async fn a_pc_ticked_then_unticked_leaves_the_roster_and_ticked_again_is_back_on_it() {
    let scratch = Scratch::new("again").await;
    let engine = scratch.serve().await;
    let before = scratch.present().await;
    let roster = || {
        scratch.rows(
            "SELECT character_id::text AS character FROM session_roster_view
             WHERE via = 'present' ORDER BY character_id",
        )
    };
    tick(&engine, SEANCE, KAEL).await;
    assert_eq!(applied(&engine).await.status, State::Applied);
    untick(&engine, SEANCE, KAEL).await;
    assert_eq!(applied(&engine).await.status, State::Applied);
    assert_eq!(scratch.present().await, before);
    assert_eq!(roster().await, [json!({ "character": ELOWEN })]);
    // Ticked again on the row its tombstone left: present once more.
    tick(&engine, SEANCE, KAEL).await;
    assert_eq!(applied(&engine).await.status, State::Applied);
    assert_eq!(
        roster().await,
        [json!({ "character": ELOWEN }), json!({ "character": KAEL })]
    );
    scratch.drop().await;
}
