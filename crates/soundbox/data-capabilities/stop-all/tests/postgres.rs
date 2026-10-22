//! soundbox.stopAll over PostgreSQL (`DATABASE_URL`), as the server plants
//! it beside its aggregate: « Tout arrêter » on a campaign gives every
//! playback still playing in it — the fixtures wrote them, no command did —
//! its stop time in `sound_playback.stopped_at`, one command per playback,
//! and changes no other row of any table: a playback stopped keeps the
//! instant it stopped at, another campaign's keep playing, a session named
//! keeps the campaign's other sessions playing, and with nothing playing it
//! succeeds and writes nothing. A stop the table lost is written again at
//! the next start. Each test works in a scratch database of its own,
//! migrated as the server migrates it, dropped when it ends.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{FollowUp, Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier};
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly};
use harness_dataguard::tokio_postgres::{Client, NoTls, connect as connected};
use serde_json::{Value, json};
use stop_all::DATA_CAPABILITY;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";

/// Valombre's two sessions, and Brume's one.
const FIRST: &str = "5e000000-0000-4000-8000-000000000001";
const SECOND: &str = "5e000000-0000-4000-8000-000000000002";
const FOG: &str = "5e000000-0000-4000-8000-000000000003";

/// Valombre's playbacks: two playing in its first session, one in its
/// second, one stopped already; Brume's, playing.
const CROWD: &str = "b1000000-0000-4000-8000-000000000001";
const DOOR: &str = "b1000000-0000-4000-8000-000000000002";
const RAIN: &str = "b1000000-0000-4000-8000-000000000003";
const DONE: &str = "b1000000-0000-4000-8000-000000000004";
const FOGHORN: &str = "b1000000-0000-4000-8000-000000000005";

/// Now, for the engine: 2026-10-22T08:00:00.123Z.
const NOW: u64 = 1_792_656_000_123;
const STOPPED: &str = "2026-10-22T08:00:00.123+00:00";

/// When `DONE` stopped, in the fixtures.
const EARLIER: &str = "2026-10-01T20:00:00+00:00";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(url: &str) -> Client {
    let (client, connection) = connected(url, NoTls).await.expect("PostgreSQL");
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

/// A database of its own, migrated, holding as the fixtures write them
/// Valombre and Brume, their sessions, a sound each, and their playbacks.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Self {
        let name = format!("stop_all_test_{name}_{}", std::process::id());
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
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind, pitch)
                     VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne', NULL),
                            ('{BRUME}', 'La Brume', 'oneshot', NULL);
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                     VALUES ('{FIRST}', '{VALOMBRE}', 1, 'La taverne', 'live', false),
                            ('{SECOND}', '{VALOMBRE}', 2, 'La route', 'prep', false),
                            ('{FOG}', '{BRUME}', 1, 'Le port', 'live', false);
                 INSERT INTO sound (id, campaign_id, name, file_path)
                     VALUES ('a0000000-0000-4000-8000-000000000001', '{VALOMBRE}', 'Foule', 'foule.ogg'),
                            ('a0000000-0000-4000-8000-000000000002', '{BRUME}', 'Corne', 'corne.ogg');
                 INSERT INTO sound_playback (id, session_id, sound_id, origin, started_at, stopped_at)
                     VALUES ('{CROWD}', '{FIRST}', 'a0000000-0000-4000-8000-000000000001', 'soundbox', '2026-10-22T07:00:00Z', NULL),
                            ('{DOOR}', '{FIRST}', 'a0000000-0000-4000-8000-000000000001', 'pastille', '2026-10-22T07:01:00Z', NULL),
                            ('{RAIN}', '{SECOND}', 'a0000000-0000-4000-8000-000000000001', 'soundbox', '2026-10-22T07:02:00Z', NULL),
                            ('{DONE}', '{FIRST}', 'a0000000-0000-4000-8000-000000000001', 'soundbox', '2026-10-01T19:00:00Z', '{EARLIER}'),
                            ('{FOGHORN}', '{FOG}', 'a0000000-0000-4000-8000-000000000002', 'soundbox', '2026-10-22T07:03:00Z', NULL);"
            ))
            .await
            .expect("the fixtures");
        // The tables' times read in UTC, whatever the server's zone.
        client
            .batch_execute("SET TIME ZONE 'UTC'")
            .await
            .expect("UTC");
        Self { name, url, client }
    }

    /// The engine as the server plants the DataCapability and its aggregate
    /// over this database, reading its tables, at [`NOW`].
    async fn plant(&self) -> Engine {
        let plugins: Plugins = [stop_all::plugin(), aggregate_sound_playback::plugin()]
            .into_iter()
            .collect();
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins)
            .await
            .expect("the projections warmed");
        // The aggregate's manifest, read where its crate keeps it.
        let aggregate = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../aggregates/SoundPlayback/aggregate.json"),
        )
        .expect("the aggregate's manifest");
        let engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(FixedClock::at_millis(NOW)),
        )
        .with_aggregate(Aggregate::parse(&aggregate).expect("contract I"))
        .with_data_capability(
            DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
        )
        .with_plugins(plugins)
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)));
        engine.load().await.expect("the engine loaded");
        engine
    }

    /// Every row of every table of the product, as JSON, by table: the
    /// DataGuard's own (`dataguard_*`) aside.
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
                    &format!("SELECT to_jsonb(t) FROM {name} t ORDER BY to_jsonb(t)::text"),
                    &[],
                )
                .await
                .expect("a table's rows")
                .iter()
                .map(|row| row.get::<_, Value>(0))
                .collect();
            tables.insert(name, rows);
        }
        tables
    }

    /// Each playback's stop, as its column holds it, by id: `None` while it
    /// plays.
    async fn stops(&self) -> BTreeMap<String, Option<String>> {
        self.client
            .query(
                "SELECT id::text, to_char(stopped_at AT TIME ZONE 'UTC',
                     'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"+00:00\"')
                 FROM sound_playback",
                &[],
            )
            .await
            .expect("the playbacks")
            .iter()
            .map(|row| (row.get(0), row.get(1)))
            .collect()
    }

    /// The database dropped, again while an autovacuum worker the test's
    /// role cannot end still holds it.
    async fn drop(self) {
        drop(self.client);
        let admin = connect(&url()).await;
        let sql = format!("DROP DATABASE {} WITH (FORCE)", self.name);
        for _ in 0..20 {
            if admin.batch_execute(&sql).await.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        admin
            .batch_execute(&sql)
            .await
            .expect("the scratch database dropped");
    }
}

fn submission(payload: Value, target: Option<&str>) -> Submission {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    Submission {
        data_capability: DATA_CAPABILITY.to_owned(),
        payload: payload.into_iter().collect(),
        target_id: target.map(str::to_owned),
        based_on: None,
        idempotency_key: None,
        caller: Caller {
            by: "dm".to_owned(),
            from: "LiveSoundbox".to_owned(),
            permissions: Vec::new(),
            roles: Vec::new(),
        },
    }
}

/// What « Tout arrêter » with `payload` became once the single applier ran:
/// the command submitted, each follow-up queued, every one applied.
async fn stopped(engine: &Engine, payload: Value) -> Submitted {
    let submitted = engine
        .submit(submission(payload, None))
        .await
        .expect("queued");
    let results = engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), 1 + submitted.then.len(), "{results:?}");
    for result in &results {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    submitted
}

/// The partitions a stop-all's commands were queued on: the one submitted,
/// then its follow-ups.
fn partitions(submitted: &Submitted) -> Vec<String> {
    let follow_ups = submitted.then.iter().map(|follow_up| match follow_up {
        FollowUp::Submitted(one) => one.partition.clone(),
        FollowUp::Refused { error, .. } => panic!("a follow-up refused: {error}"),
    });
    std::iter::once(submitted.partition.clone())
        .chain(follow_ups)
        .collect()
}

/// `tables` with the playbacks `ids` stopped at [`STOPPED`], in the order
/// [`Scratch::tables`] reads them.
fn with_stopped(
    mut tables: BTreeMap<String, Vec<Value>>,
    ids: &[&str],
) -> BTreeMap<String, Vec<Value>> {
    let playbacks = tables.get_mut("sound_playback").expect("the playbacks");
    for playback in playbacks.iter_mut() {
        if ids.iter().any(|id| playback["id"] == *id) {
            playback["stopped_at"] = json!(STOPPED);
        }
    }
    playbacks.sort_by_key(ToString::to_string);
    tables
}

fn playing(stops: &BTreeMap<String, Option<String>>) -> Vec<&str> {
    stops
        .iter()
        .filter(|(_, stop)| stop.is_none())
        .map(|(id, _)| id.as_str())
        .collect()
}

#[tokio::test]
async fn every_playback_still_playing_in_the_campaign_stops_and_nothing_else_changes() {
    let scratch = Scratch::new("campaign").await;
    let engine = scratch.plant().await;
    let before = scratch.tables().await;

    let submitted = stopped(&engine, json!({ "campaign": VALOMBRE })).await;

    assert_eq!(
        partitions(&submitted),
        [CROWD, DOOR, RAIN].map(|id| format!("SoundPlayback/{id}")),
        "one command per playback playing, by the order they started in"
    );
    let stops = scratch.stops().await;
    for id in [CROWD, DOOR, RAIN] {
        assert_eq!(stops[id].as_deref(), Some(STOPPED), "{id}");
    }
    assert_eq!(
        stops[DONE].as_deref(),
        Some("2026-10-01T20:00:00.000+00:00")
    );
    assert_eq!(playing(&stops), [FOGHORN], "Brume's keeps playing");
    assert_eq!(
        scratch.tables().await,
        with_stopped(before, &[CROWD, DOOR, RAIN])
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_session_named_stops_its_playbacks_alone() {
    let scratch = Scratch::new("session").await;
    let engine = scratch.plant().await;

    let submitted = stopped(
        &engine,
        json!({ "campaign": VALOMBRE, "session": SECOND.to_uppercase() }),
    )
    .await;

    assert_eq!(partitions(&submitted), [format!("SoundPlayback/{RAIN}")]);
    assert!(submitted.then.is_empty());
    assert_eq!(playing(&scratch.stops().await), [CROWD, DOOR, FOGHORN]);
    scratch.drop().await;
}

#[tokio::test]
async fn with_nothing_playing_a_stop_all_succeeds_and_writes_nothing() {
    let scratch = Scratch::new("silence").await;
    let engine = scratch.plant().await;
    stopped(&engine, json!({ "campaign": VALOMBRE })).await;
    let before = scratch.tables().await;

    // A second click: everything of Valombre is stopped already.
    let submitted = stopped(&engine, json!({ "campaign": VALOMBRE })).await;

    assert_eq!(
        submitted.partition,
        format!("SoundPlayback/stop-all-{VALOMBRE}")
    );
    assert!(submitted.then.is_empty());
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_stop_queued_and_not_yet_applied_is_never_queued_twice() {
    let scratch = Scratch::new("twice").await;
    let engine = scratch.plant().await;
    let first = engine
        .submit(submission(json!({ "campaign": VALOMBRE }), None))
        .await
        .expect("queued");
    assert_eq!(first.then.len(), 2);

    let second = engine
        .submit(submission(json!({ "campaign": VALOMBRE }), None))
        .await
        .expect("queued");

    assert_eq!(
        second.partition,
        format!("SoundPlayback/stop-all-{VALOMBRE}")
    );
    assert!(second.then.is_empty(), "{:?}", second.then);
    let results = engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), 4, "{results:?}");
    assert!(
        results.iter().all(|r| r.status == State::Applied),
        "{results:?}"
    );
    assert_eq!(playing(&scratch.stops().await), [FOGHORN]);
    scratch.drop().await;
}

#[tokio::test]
async fn a_request_naming_no_campaign_is_refused_before_anything_queues() {
    let scratch = Scratch::new("refused").await;
    let engine = scratch.plant().await;
    let before = scratch.tables().await;

    for payload in [
        json!({}),
        json!({ "campaign": "valombre" }),
        json!({ "campaign": VALOMBRE, "session": "la-taverne" }),
    ] {
        let refused = engine.submit(submission(payload.clone(), None)).await;
        assert!(
            matches!(refused, Err(Error::Refused(ref r)) if r.kind == RefusalKind::Unprocessable),
            "{payload}: {refused:?}"
        );
    }

    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_campaign_named_as_the_target_stops_its_playbacks() {
    let scratch = Scratch::new("target").await;
    let engine = scratch.plant().await;

    let submitted = engine
        .submit(submission(json!({}), Some(BRUME)))
        .await
        .expect("queued");
    engine.run_once().await.expect("the applier ran");

    assert_eq!(submitted.partition, format!("SoundPlayback/{FOGHORN}"));
    assert_eq!(playing(&scratch.stops().await), [CROWD, DOOR, RAIN]);
    scratch.drop().await;
}

#[tokio::test]
async fn a_stop_the_table_lost_is_written_again_at_the_next_start() {
    let scratch = Scratch::new("lost").await;
    let engine = scratch.plant().await;
    stopped(&engine, json!({ "campaign": BRUME })).await;
    drop(engine);
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE sound_playback SET stopped_at = NULL WHERE id = '{FOGHORN}'"
        ))
        .await
        .expect("the stop lost");

    let _restarted = scratch.plant().await;

    let stops = scratch.stops().await;
    assert_eq!(stops[FOGHORN].as_deref(), Some(STOPPED));
    assert_eq!(playing(&stops), [CROWD, DOOR, RAIN]);
    scratch.drop().await;
}

#[tokio::test]
async fn a_playback_the_table_holds_stopped_keeps_its_instant_at_the_next_start() {
    let scratch = Scratch::new("kept").await;
    let engine = scratch.plant().await;
    stopped(&engine, json!({ "campaign": BRUME })).await;
    drop(engine);
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE sound_playback SET stopped_at = '{EARLIER}' WHERE id = '{FOGHORN}'"
        ))
        .await
        .expect("an earlier stop");

    let _restarted = scratch.plant().await;

    assert_eq!(
        scratch.stops().await[FOGHORN].as_deref(),
        Some("2026-10-01T20:00:00.000+00:00")
    );
    scratch.drop().await;
}

/// A playback added under the id its payload names, test-only.
const ADD: &str = r#"{"dataCapability":"test.addPlayback","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"SoundPlayback","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

#[tokio::test]
async fn a_write_on_a_partition_naming_no_playback_projects_nothing_and_is_applied() {
    let scratch = Scratch::new("no_playback").await;
    let engine = scratch
        .plant()
        .await
        .with_data_capability(DataCapability::parse(ADD).expect("contract F"));
    let before = scratch.tables().await;

    let mut added = submission(json!({ "id": "new_1", "origin": "soundbox" }), None);
    added.data_capability = "test.addPlayback".to_owned();
    let submitted = engine.submit(added).await.expect("queued");
    let results = engine.run_once().await.expect("the applier ran");

    assert_eq!(submitted.partition, "SoundPlayback/new_1");
    assert_eq!(results.len(), 1, "{results:?}");
    assert_eq!(results[0].status, State::Applied, "{results:?}");
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}
