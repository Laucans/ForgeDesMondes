//! live.closeWindow over PostgreSQL (`DATABASE_URL`), as the server plants
//! it: a close is prepared against the live session the tables hold, judged
//! by the DataGuard and projected onto the session's `diffusion` row in the
//! transaction that applies it. The open window's media, x, y and width are
//! emptied together, the base and the framing kept; a window already closed
//! is closed again, the row unchanged but its stamp; a session nothing was
//! shown on, or a campaign with no live session, is refused and nothing is
//! queued. Each test works in a scratch database of its own, migrated as the
//! server migrates it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::tokio_postgres::error::SqlState;
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's session before the live one, closed.
const SEANCE_1: &str = "51000000-0000-4000-8000-000000000001";
/// Valombre's live session.
const SEANCE: &str = "51000000-0000-4000-8000-000000000002";
/// Brume's session, still in preparation: Brume has no live session.
const SEANCE_BRUME: &str = "51000000-0000-4000-8000-000000000003";
const SCENE: &str = "5c000000-0000-4000-8000-000000000002";
/// Valombre's battlemap of the ford.
const CARTE: &str = "a1000000-0000-4000-8000-000000000001";
/// An illustration of Valombre, 800 × 1200.
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
/// The ford's instance in the live session.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";

/// The stamp a fixture's diffusion row was written at, long before any
/// close.
const LONG_AGO: &str = "2000-01-01T00:00:00+00:00";

/// The live session's row showing the ford's instance, framed, with the
/// portrait open in its window — or with no window; or showing the portrait
/// as its base, with the portrait open in its window too.
const WINDOW_OPEN: &str = "'battlemap', NULL, 'b1000000-0000-4000-8000-000000000001',
    'd1000000-0000-4000-8000-000000000002', 0.6, 0.12, 0.3, 1.5, 120.5, -40";
const ILLUSTRATION_WINDOW_OPEN: &str =
    "'illustration', 'd1000000-0000-4000-8000-000000000002', NULL,
    'd1000000-0000-4000-8000-000000000002', 0.6, 0.12, 0.3, 1.5, 120.5, -40";
const NO_WINDOW: &str = "'battlemap', NULL, 'b1000000-0000-4000-8000-000000000001', NULL, NULL, NULL, NULL, 1.5, 120.5, -40";

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
/// the fixtures write them — Valombre with its closed session and its live
/// one, a scene in the live one, its battlemap of the ford instanced there,
/// and its portrait; Brume with a session in preparation. No session has a
/// diffusion row.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("close_window_test_{name}_{}", std::process::id());
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
                     ('{BRUME}', 'La Brume', 'campagne');
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                     ('{SEANCE_1}', '{VALOMBRE}', 1, 'Séance 1', 'closed', false),
                     ('{SEANCE}', '{VALOMBRE}', 2, 'Séance 2', 'live', false),
                     ('{SEANCE_BRUME}', '{BRUME}', 1, 'Séance 1', 'prep', false);
                 INSERT INTO scene (id, session_id, position, title) VALUES
                     ('{SCENE}', '{SEANCE}', 1, 'Le Gué');
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES
                     ('{CARTE}', '{VALOMBRE}', 'battlemap', 'Le Gué', 'media/gue.png', 960, 640),
                     ('{PORTRAIT}', '{VALOMBRE}', 'illustration', 'Morlox', 'media/morlox.png', 800, 1200);
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
                     VALUES ('{CARTE}', 30, 12, 8, 'pointe', '#202020', 0.4);
                 INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) VALUES
                     ('{INSTANCE}', '{CARTE}', '{SEANCE}', '{SCENE}');"
            ))
            .await
            .expect("Valombre, Brume, their sessions and their media");
        Scratch { name, url, client }
    }

    /// The live session's diffusion row, as a fixture writes it by SQL.
    async fn diffusing(&self, values: &str) {
        self.client
            .batch_execute(&format!(
                "INSERT INTO diffusion (session_id, base_kind, base_media_id, base_instance_id,
                     window_media_id, window_x, window_y, window_width, view_zoom, view_pan_x,
                     view_pan_y, updated_at)
                 VALUES ('{SEANCE}', {values}, '{LONG_AGO}')"
            ))
            .await
            .expect("the fixture's diffusion");
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database: the aggregate's projection in the apply
    /// transaction, the tables lent to the Prepare.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins {
            [aggregate_diffusion::plugin(), close_window::plugin()]
                .into_iter()
                .collect()
        };
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        let engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_aggregate(Aggregate::parse(aggregate_diffusion::DOCUMENT).expect("contract I"))
        .with_data_capability(
            DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
        )
        .with_plugins(plugins());
        engine.load().await.expect("the engine loads");
        engine
    }

    /// Every `diffusion` row, as JSON, its stamp aside — and whether a close
    /// stamped it, after `LONG_AGO`.
    async fn diffusions(&self) -> Vec<(Value, bool)> {
        self.client
            .query(
                "SELECT (to_jsonb(d) - 'updated_at')::text, d.updated_at > $1::text::timestamptz
                 FROM diffusion d ORDER BY session_id",
                &[&LONG_AGO],
            )
            .await
            .expect("the diffusion table")
            .iter()
            .map(|row| {
                (
                    serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"),
                    row.get::<_, bool>(1),
                )
            })
            .collect()
    }

    /// Drops the database. An autovacuum worker the writes woke may still be
    /// on it, which `FORCE` cannot end for a role that is not a superuser:
    /// the drop is tried again while the worker finishes.
    async fn drop(self) {
        drop(self.client);
        let admin = connect(&url()).await;
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

/// A close by the DM from their session bar, who holds no role — the
/// manifest touches no critical field — naming `target` as the element it is
/// sent from.
async fn close(engine: &Engine, target: Option<&str>, payload: Value) -> Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "live.closeWindow@1",
        "targetId": target,
        "payload": payload,
        "caller": { "by": "dm", "from": "LiveSessionBar", "roles": [] },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

/// The live session's row showing the ford's instance, framed, its window
/// as given.
fn showing(window: [Value; 4]) -> Value {
    let [media, x, y, width] = window;
    json!({
        "session_id": SEANCE, "base_kind": "battlemap",
        "base_media_id": null, "base_instance_id": INSTANCE,
        "window_media_id": media, "window_x": x, "window_y": y, "window_width": width,
        "view_zoom": 1.5, "view_pan_x": 120.5, "view_pan_y": -40.0,
    })
}

/// The ford's instance, framed, with no window.
fn closed() -> Value {
    showing([Value::Null, Value::Null, Value::Null, Value::Null])
}

#[tokio::test]
async fn a_close_of_the_open_window_empties_its_four_fields_the_battlemap_kept() {
    let scratch = Scratch::new("open").await;
    scratch.diffusing(WINDOW_OPEN).await;
    let engine = scratch.serve().await;

    // The element a close names as its target is not the row it writes: the
    // session's is (single-diffusion-source).
    let submitted = close(
        &engine,
        Some(PORTRAIT),
        json!({ "campaign": VALOMBRE.to_uppercase() }),
    )
    .await
    .expect("queued");
    assert_eq!(submitted.partition, format!("Diffusion/{SEANCE}"));
    assert_eq!(submitted.result.status, State::Queued, "no review asked");
    assert!(submitted.warnings.is_empty(), "{submitted:?}");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(scratch.diffusions().await, [(closed(), true)]);
    scratch.drop().await;
}

#[tokio::test]
async fn a_close_over_an_illustration_keeps_the_illustration_as_the_base() {
    let scratch = Scratch::new("illustration").await;
    scratch.diffusing(ILLUSTRATION_WINDOW_OPEN).await;
    let engine = scratch.serve().await;
    close(&engine, None, json!({ "campaign": VALOMBRE }))
        .await
        .expect("queued");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    let mut expected = closed();
    expected["base_kind"] = json!("illustration");
    expected["base_media_id"] = json!(PORTRAIT);
    expected["base_instance_id"] = Value::Null;
    assert_eq!(scratch.diffusions().await, [(expected, true)]);
    scratch.drop().await;
}

#[tokio::test]
async fn a_window_already_closed_is_closed_again_the_row_unchanged_but_its_stamp() {
    let scratch = Scratch::new("closed").await;
    scratch.diffusing(NO_WINDOW).await;
    let engine = scratch.serve().await;
    let submitted = close(&engine, None, json!({ "campaign": VALOMBRE }))
        .await
        .expect("queued");
    assert!(submitted.warnings.is_empty(), "{submitted:?}");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(scratch.diffusions().await, [(closed(), true)]);
    scratch.drop().await;
}

#[tokio::test]
async fn closes_in_a_row_are_queued_none_asked_to_confirm_and_each_applied() {
    let scratch = Scratch::new("in_a_row").await;
    scratch.diffusing(WINDOW_OPEN).await;
    let engine = scratch.serve().await;
    let mut commands = Vec::new();
    for _ in 0..2 {
        let submitted = close(&engine, None, json!({ "campaign": VALOMBRE }))
            .await
            .expect("queued behind the one before");
        assert_eq!(submitted.result.status, State::Queued, "{submitted:?}");
        assert!(submitted.warnings.is_empty(), "{submitted:?}");
        commands.push((submitted.command_id, State::Applied));
    }
    let applied: Vec<_> = engine
        .run_once()
        .await
        .expect("the applier ran")
        .into_iter()
        .map(|result| (result.command_id, result.status))
        .collect();
    assert_eq!(applied, commands, "each applied, in the order submitted");
    assert_eq!(scratch.diffusions().await, [(closed(), true)]);
    scratch.drop().await;
}

#[tokio::test]
async fn a_session_showing_nothing_yet_has_no_window_and_its_close_is_refused() {
    let scratch = Scratch::new("no_row").await;
    let engine = scratch.serve().await;
    match close(&engine, None, json!({ "campaign": VALOMBRE })).await {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::Unprocessable);
            assert_eq!(refusal.message, "la vue joueurs n'a encore rien reçu");
        }
        other => panic!("{other:?}"),
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert!(scratch.diffusions().await.is_empty(), "no row is made");
    scratch.drop().await;
}

#[tokio::test]
async fn a_campaign_no_live_session_shows_is_refused_and_nothing_is_queued() {
    let scratch = Scratch::new("refused").await;
    scratch.diffusing(WINDOW_OPEN).await;
    let engine = scratch.serve().await;
    for (payload, why) in [
        (json!({ "campaign": BRUME }), "aucune séance n'est en cours"),
        (
            json!({ "campaign": "c1000000-0000-4000-8000-00000000ffff" }),
            "aucune séance n'est en cours",
        ),
        (json!({}), "campaign manque"),
        (
            json!({ "campaign": "Valombre" }),
            "campaign n'est pas un identifiant",
        ),
    ] {
        match close(&engine, None, payload).await {
            Err(Error::Refused(refusal)) => {
                assert_eq!(refusal.kind, RefusalKind::Unprocessable, "{why}");
                assert_eq!(refusal.message, why);
            }
            other => panic!("{why}: {other:?}"),
        }
    }

    // The engine's soft delete of the live session: its row tombstoned, the
    // table's left as it was.
    scratch
        .client
        .batch_execute(&format!(
            "INSERT INTO dataguard_rows (partition, tombstoned) VALUES ('Session/{SEANCE}', true)"
        ))
        .await
        .expect("the live session deleted");
    match close(&engine, None, json!({ "campaign": VALOMBRE })).await {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.message, "aucune séance n'est en cours");
        }
        other => panic!("{other:?}"),
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(
        scratch.diffusions().await,
        [(
            showing([json!(PORTRAIT), json!(0.6), json!(0.12), json!(0.3)]),
            false
        )]
    );
    scratch.drop().await;
}
