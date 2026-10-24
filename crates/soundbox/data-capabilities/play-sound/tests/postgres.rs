//! soundbox.playSound over PostgreSQL (`DATABASE_URL`), as the server
//! plants it: a sound the tables hold — one the fixtures wrote, which no
//! command wrote before — plays in its campaign's session, found in the
//! tables when the payload names none; its playback lands in
//! `sound_playback` beside the ones running, which keep running
//! (`simultaneous-playback`). A row the table refuses rejects the command,
//! and a later write of a playback is not this projection's. Each test
//! works in a scratch database of its own, migrated as the server migrates
//! it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::store::RandomIds;
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const CENDRE: &str = "c1000000-0000-4000-8000-000000000003";
const VIDE: &str = "c1000000-0000-4000-8000-000000000004";
/// Valombre's sessions: 3 closed, 4 live, 5 in prep.
const VALOMBRE_3: &str = "51000000-0000-4000-8000-000000000103";
const VALOMBRE_4: &str = "51000000-0000-4000-8000-000000000104";
const VALOMBRE_5: &str = "51000000-0000-4000-8000-000000000105";
/// Brume's two closed sessions.
const BRUME_1: &str = "51000000-0000-4000-8000-000000000201";
const BRUME_2: &str = "51000000-0000-4000-8000-000000000202";
/// Cendre's two sessions in prep.
const CENDRE_1: &str = "51000000-0000-4000-8000-000000000301";
const CENDRE_2: &str = "51000000-0000-4000-8000-000000000302";
/// « Bruit de foule », « Porte qui grince », « Pluie » (Valombre's); Brume's
/// wind, Cendre's fire, and the bell of Vide, which has no session.
const FOULE: &str = "50000000-0000-4000-8000-000000000001";
const PORTE: &str = "50000000-0000-4000-8000-000000000002";
const PLUIE: &str = "50000000-0000-4000-8000-000000000003";
const VENT: &str = "50000000-0000-4000-8000-000000000004";
const FEU: &str = "50000000-0000-4000-8000-000000000005";
const CLOCHE: &str = "50000000-0000-4000-8000-000000000006";
/// The rain, playing in session 4 since the fixtures.
const PLUIE_EN_COURS: &str = "b1000000-0000-4000-8000-000000000001";

/// A playback's stop, test-only: `soundbox.stopAll` is a unit of its own.
const STOP: &str = r#"{"dataCapability":"test.stopPlayback","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"SoundPlayback","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

/// A playback removed, test-only: no unit removes one.
const REMOVE: &str = r#"{"dataCapability":"test.removePlayback","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"SoundPlayback","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

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
/// the fixtures write them — four campaigns, their sessions and sounds, and
/// the rain playing in Valombre's session 4.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("play_sound_test_{name}_{}", std::process::id());
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
                "INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                     ('{BRUME}', 'Brume', 'campagne'),
                     ('{CENDRE}', 'Cendre', 'campagne'),
                     ('{VIDE}', 'Vide', 'oneshot');
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                     ('{VALOMBRE_3}', '{VALOMBRE}', 3, 'Le marché noir', 'closed', false),
                     ('{VALOMBRE_4}', '{VALOMBRE}', 4, 'L''embuscade du Gué', 'live', true),
                     ('{VALOMBRE_5}', '{VALOMBRE}', 5, 'La tour', 'prep', false),
                     ('{BRUME_1}', '{BRUME}', 1, 'Le phare', 'closed', false),
                     ('{BRUME_2}', '{BRUME}', 2, 'La falaise', 'closed', false),
                     ('{CENDRE_1}', '{CENDRE}', 1, 'Le bûcher', 'prep', false),
                     ('{CENDRE_2}', '{CENDRE}', 2, 'Les braises', 'prep', false);
                 INSERT INTO sound (id, campaign_id, name, file_path) VALUES
                     ('{FOULE}', '{VALOMBRE}', 'Bruit de foule', 'sons/foule.ogg'),
                     ('{PORTE}', '{VALOMBRE}', 'Porte qui grince', 'sons/porte.ogg'),
                     ('{PLUIE}', '{VALOMBRE}', 'Pluie', 'sons/pluie.ogg'),
                     ('{VENT}', '{BRUME}', 'Vent', 'sons/vent.ogg'),
                     ('{FEU}', '{CENDRE}', 'Feu', 'sons/feu.ogg'),
                     ('{CLOCHE}', '{VIDE}', 'Cloche', 'sons/cloche.ogg');
                 INSERT INTO sound_playback (id, session_id, sound_id, origin, started_at) VALUES
                     ('{PLUIE_EN_COURS}', '{VALOMBRE_4}', '{PLUIE}', 'soundbox', '2026-10-17 20:05');"
            ))
            .await
            .expect("the campaigns, their sessions, sounds and the rain playing");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database: the playback's projection in the apply
    /// transaction, the tables lent to the Prepare.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins {
            [aggregate_sound_playback::plugin(), play_sound::plugin()]
                .into_iter()
                .collect()
        };
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../aggregates/SoundPlayback/aggregate.json");
        let aggregate = std::fs::read_to_string(path).expect("SoundPlayback's aggregate.json");
        let mut engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_ids(Arc::new(RandomIds))
        .with_aggregate(Aggregate::parse(&aggregate).expect("contract I"));
        for manifest in [include_str!("../data-capability.json"), STOP, REMOVE] {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        let engine = engine.with_plugins(plugins());
        engine.load().await.expect("the engine loads");
        engine
    }

    /// What `sql` answers, each row as JSON, in order.
    async fn rows(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(&format!("SELECT to_jsonb(t)::text FROM ({sql}) t"), &[])
            .await
            .expect("the rows")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
    }

    /// The playbacks the table holds, by when they started: session, sound,
    /// origin, whether they run.
    async fn playbacks(&self) -> Vec<Value> {
        self.rows(
            "SELECT id::text, session_id::text AS session, sound_id::text AS sound, origin,
                    stopped_at IS NULL AS running
             FROM sound_playback ORDER BY started_at, id",
        )
        .await
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

/// A submission of `data_capability` on `target` with `payload`, by the DM.
async fn submit(
    engine: &Engine,
    data_capability: &str,
    target: &str,
    payload: Value,
) -> Result<Submitted> {
    engine
        .submit(
            serde_json::from_value::<Submission>(json!({
                "dataCapability": data_capability,
                "targetId": target,
                "payload": payload,
                "caller": { "by": "dm", "from": "soundbox.LiveSoundbox", "roles": ["role:approver"] },
            }))
            .expect("a submission"),
        )
        .await
}

/// One pass of the single applier, which settles one command.
async fn settled(engine: &Engine) -> CommandResult {
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

/// `sound` triggered with `payload` and applied: its playback's id.
async fn played(engine: &Engine, sound: &str, payload: Value) -> String {
    let submitted = submit(engine, "soundbox.playSound@1", sound, payload)
        .await
        .expect("queued");
    let applied = settled(engine).await;
    assert_eq!(applied.status, State::Applied, "{applied:?}");
    submitted
        .partition
        .strip_prefix("SoundPlayback/")
        .expect("a playback's partition")
        .to_owned()
}

#[tokio::test]
async fn two_sounds_play_together_in_the_live_session_beside_the_one_running() {
    let scratch = Scratch::new("together").await;
    let engine = scratch.serve().await;
    let foule = played(&engine, FOULE, json!({})).await;
    let porte = played(&engine, PORTE, json!({ "source": "suggestion" })).await;
    assert_eq!(
        scratch.playbacks().await,
        [
            json!({ "id": PLUIE_EN_COURS, "session": VALOMBRE_4, "sound": PLUIE, "origin": "soundbox", "running": true }),
            json!({ "id": foule, "session": VALOMBRE_4, "sound": FOULE, "origin": "soundbox", "running": true }),
            json!({ "id": porte, "session": VALOMBRE_4, "sound": PORTE, "origin": "pastille", "running": true }),
        ]
    );
    let started = scratch
        .rows(&format!(
            "SELECT started_at > now() - interval '1 minute' AS now
             FROM sound_playback WHERE id = '{foule}'"
        ))
        .await;
    assert_eq!(started, [json!({ "now": true })], "started now");
    scratch.drop().await;
}

#[tokio::test]
async fn the_session_is_the_live_one_else_the_latest_not_closed_else_the_latest() {
    let scratch = Scratch::new("session").await;
    let engine = scratch.serve().await;
    for (sound, session) in [(VENT, BRUME_2), (FEU, CENDRE_2)] {
        let id = played(&engine, sound, json!({})).await;
        let row = scratch
            .rows(&format!(
                "SELECT session_id::text AS session FROM sound_playback WHERE id = '{id}'"
            ))
            .await;
        assert_eq!(row, [json!({ "session": session })], "{sound}");
    }
    // The session the payload names, of the sound's campaign, even closed.
    let id = played(&engine, FOULE, json!({ "session": VALOMBRE_3 })).await;
    let row = scratch
        .rows(&format!(
            "SELECT session_id::text AS session FROM sound_playback WHERE id = '{id}'"
        ))
        .await;
    assert_eq!(row, [json!({ "session": VALOMBRE_3 })]);
    scratch.drop().await;
}

#[tokio::test]
async fn a_sound_with_no_session_or_another_campaign_s_is_refused_and_nothing_is_written() {
    let scratch = Scratch::new("refused").await;
    let engine = scratch.serve().await;
    let before = scratch.playbacks().await;
    for (sound, payload, kind, message) in [
        (
            CLOCHE,
            json!({}),
            RefusalKind::Unprocessable,
            "la campagne de ce son n'a aucune séance",
        ),
        (
            FOULE,
            json!({ "session": BRUME_1 }),
            RefusalKind::Unprocessable,
            "cette séance n'est pas de la campagne du son",
        ),
        (
            "50000000-0000-4000-8000-0000000000ff",
            json!({}),
            RefusalKind::NotFound,
            "ce son n'existe pas",
        ),
    ] {
        match submit(&engine, "soundbox.playSound@1", sound, payload).await {
            Err(Error::Refused(refusal)) if refusal.kind == kind => {
                assert_eq!(refusal.message, message);
            }
            other => panic!("{message}: {other:?}"),
        }
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.playbacks().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_sound_gone_before_its_playback_is_applied_rejects_it_and_the_table_keeps_none() {
    let scratch = Scratch::new("gone").await;
    let engine = scratch.serve().await;
    submit(&engine, "soundbox.playSound@1", FOULE, json!({}))
        .await
        .expect("queued");
    scratch
        .client
        .batch_execute(&format!("DELETE FROM sound WHERE id = '{FOULE}'"))
        .await
        .expect("the crowd removed");
    let result = settled(&engine).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(
        scratch.playbacks().await.len(),
        1,
        "the rain alone, still playing"
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_later_write_of_a_playback_the_table_holds_is_not_this_projection_s() {
    let scratch = Scratch::new("later").await;
    let engine = scratch.serve().await;
    let foule = played(&engine, FOULE, json!({})).await;
    submit(
        &engine,
        "test.stopPlayback",
        &foule,
        json!({ "stoppedAt": "2026-10-17T22:00:00.000Z" }),
    )
    .await
    .expect("queued");
    let stopped = settled(&engine).await;
    assert_eq!(stopped.status, State::Applied, "{stopped:?}");
    let row = scratch
        .rows(&format!(
            "SELECT stopped_at IS NULL AS running FROM sound_playback WHERE id = '{foule}'"
        ))
        .await;
    assert_eq!(row, [json!({ "running": true })], "stopAll's to project");
    scratch.drop().await;
}

#[tokio::test]
async fn a_playback_removed_writes_nothing_into_the_table() {
    let scratch = Scratch::new("removed").await;
    let engine = scratch.serve().await;
    let foule = played(&engine, FOULE, json!({})).await;
    // The table forgot it; its removal must not write it back.
    scratch
        .client
        .batch_execute(&format!("DELETE FROM sound_playback WHERE id = '{foule}'"))
        .await
        .expect("forgotten");
    submit(&engine, "test.removePlayback", &foule, json!({}))
        .await
        .expect("queued");
    let removed = settled(&engine).await;
    assert_eq!(removed.status, State::Applied, "{removed:?}");
    assert_eq!(scratch.playbacks().await.len(), 1, "the rain alone");
    scratch.drop().await;
}

#[tokio::test]
async fn a_table_that_fails_fails_the_step_and_rejects_nothing() {
    let scratch = Scratch::new("fails").await;
    let engine = scratch.serve().await;
    submit(&engine, "soundbox.playSound@1", FOULE, json!({}))
        .await
        .expect("queued");
    scratch
        .client
        .batch_execute("DROP TABLE sound_playback")
        .await
        .expect("the table gone");
    match engine.run_once().await {
        Err(Error::Store(_)) => {}
        other => panic!("the store's failure, not the command's: {other:?}"),
    }
    scratch.drop().await;
}

#[tokio::test]
async fn a_stop_of_a_playback_only_the_tables_hold_is_not_refused_by_this_projection() {
    let scratch = Scratch::new("fixture").await;
    let engine = scratch.serve().await;
    // The rain: the fixtures' playback, which no command wrote.
    submit(
        &engine,
        "test.stopPlayback",
        PLUIE_EN_COURS,
        json!({ "stoppedAt": "2026-10-17T22:00:00.000Z" }),
    )
    .await
    .expect("queued");
    let stopped = settled(&engine).await;
    assert_eq!(stopped.status, State::Applied, "{stopped:?}");
    assert_eq!(scratch.playbacks().await.len(), 1, "nothing inserted");
    scratch.drop().await;
}
