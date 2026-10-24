//! live.showWindowOnPlayerView over PostgreSQL (`DATABASE_URL`), as the
//! server plants it: a window is prepared against the live session and the
//! media the tables hold, judged by the DataGuard and projected onto the
//! session's `diffusion` row in the transaction that applies it. An
//! illustration opens over the base, which stays as it was, and replaces the
//! one a window showed; a session with no committed diffusion is refused, a
//! window making no row; a media that is no illustration of the campaign is
//! rejected under window-proportional; a campaign with no live session, or a media the
//! tables do not hold, is refused and nothing is queued. Each test works in
//! a scratch database of its own, migrated as the server migrates it,
//! dropped when it ends.

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
use show_window_on_player_view::{NOT_AN_ILLUSTRATION, WIDTH, X, Y};

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
/// An illustration of Valombre, 1200 × 800.
const PAYSAGE: &str = "d1000000-0000-4000-8000-000000000003";
/// An illustration of Valombre whose file gave no width: no proportions.
const SANS_TAILLE: &str = "d1000000-0000-4000-8000-000000000004";
/// An illustration of Brume.
const BRUMEUX: &str = "d1000000-0000-4000-8000-000000000005";
/// The ford's instance in the live session.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";

/// The stamp a fixture's diffusion row was written at, long before any
/// window.
const LONG_AGO: &str = "2000-01-01T00:00:00+00:00";

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
/// and its illustrations; Brume with a session in preparation and an
/// illustration. No session has a diffusion row.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("show_window_test_{name}_{}", std::process::id());
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
                     ('{PORTRAIT}', '{VALOMBRE}', 'illustration', 'Morlox', 'media/morlox.png', 800, 1200),
                     ('{PAYSAGE}', '{VALOMBRE}', 'illustration', 'La vallée', 'media/vallee.png', 1200, 800),
                     ('{SANS_TAILLE}', '{VALOMBRE}', 'illustration', 'Esquisse', 'media/esquisse.png', 0, 600),
                     ('{BRUMEUX}', '{BRUME}', 'illustration', 'Le phare', 'media/phare.png', 800, 1200);
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
            [
                aggregate_diffusion::plugin(),
                show_window_on_player_view::plugin(),
            ]
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

    /// Every `diffusion` row, as JSON, its stamp aside — which is checked
    /// written after `LONG_AGO` when `stamped`.
    async fn diffusions(&self, stamped: bool) -> Vec<Value> {
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
                assert_eq!(row.get::<_, bool>(1), stamped, "stamped by a window");
                serde_json::from_str(row.get::<_, &str>(0)).expect("JSON")
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

/// A window opening `media` on `campaign`'s player view, by the DM from a
/// Micro-UI — no approver's role, as every Micro-UI sends it —, naming
/// `target` as the element it is sent from.
async fn window(
    engine: &Engine,
    target: Option<&str>,
    campaign: &str,
    media: &str,
) -> Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "live.showWindowOnPlayerView@1",
        "targetId": target,
        "payload": { "campaign": campaign, "media": media },
        "caller": { "by": "dm", "from": "table.TableSheet", "roles": [] },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

/// The live session's row, its base, its window and its framing as given.
fn diffusion(base: [Value; 3], window: [Value; 4], zoom: f64, pan: (f64, f64)) -> Value {
    let [kind, base_media, instance] = base;
    let [media, x, y, width] = window;
    json!({
        "session_id": SEANCE, "base_kind": kind,
        "base_media_id": base_media, "base_instance_id": instance,
        "window_media_id": media, "window_x": x, "window_y": y, "window_width": width,
        "view_zoom": zoom, "view_pan_x": pan.0, "view_pan_y": pan.1,
    })
}

/// A window open on `media` at its default place and width.
fn opened(media: &str) -> [Value; 4] {
    [json!(media), json!(X), json!(Y), json!(WIDTH)]
}

fn battlemap() -> [Value; 3] {
    [json!("battlemap"), Value::Null, json!(INSTANCE)]
}

#[tokio::test]
async fn a_window_opens_over_a_battlemap_replacing_the_one_open_the_base_unchanged() {
    let scratch = Scratch::new("battlemap").await;
    scratch
        .diffusing(&format!(
            "'battlemap', NULL, '{INSTANCE}', '{PAYSAGE}', 0.1, 0.2, 0.5, 1.5, 120.5, -40"
        ))
        .await;
    let engine = scratch.serve().await;

    // The element a window names as its target is not the row it writes:
    // the session's is. The window touches no critical field: the DM needs
    // no approver.
    let submitted = window(
        &engine,
        Some(PORTRAIT),
        &VALOMBRE.to_uppercase(),
        &PORTRAIT.to_uppercase(),
    )
    .await
    .expect("queued");
    assert_eq!(submitted.partition, format!("Diffusion/{SEANCE}"));
    assert_eq!(submitted.result.status, State::Queued, "{submitted:?}");
    assert!(submitted.warnings.is_empty(), "{submitted:?}");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        scratch.diffusions(true).await,
        [diffusion(
            battlemap(),
            opened(PORTRAIT),
            1.5,
            (120.5, -40.0)
        )]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_session_with_no_committed_diffusion_is_refused_and_nothing_is_queued() {
    let scratch = Scratch::new("no_row").await;
    let engine = scratch.serve().await;

    // Nothing sent to the player view yet: a window makes no row, the base
    // being no business of its own.
    match window(&engine, None, VALOMBRE, PORTRAIT).await {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::Unprocessable);
            assert_eq!(refusal.message, "la vue joueurs n'a encore rien reçu");
        }
        other => panic!("a window on a session with no diffusion: {other:?}"),
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert!(scratch.diffusions(true).await.is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn a_media_that_is_no_illustration_of_the_campaign_is_rejected_under_window_proportional() {
    let scratch = Scratch::new("proportional").await;
    scratch
        .diffusing(&format!(
            "'battlemap', NULL, '{INSTANCE}', NULL, NULL, NULL, NULL, 1, 0, 0"
        ))
        .await;
    let engine = scratch.serve().await;
    // A battlemap, an illustration with no width, one of another campaign.
    for media in [CARTE, SANS_TAILLE, BRUMEUX] {
        let submitted = window(&engine, None, VALOMBRE, media)
            .await
            .expect("queued, for the DataGuard to judge");
        assert_eq!(
            submitted.warnings.first().map(String::as_str),
            Some(NOT_AN_ILLUSTRATION),
            "{media}: {submitted:?}"
        );
        let result = engine.run_once().await.expect("the applier ran").remove(0);
        assert_eq!(result.status, State::Rejected, "{media}: {result:?}");
        assert_eq!(result.violations, ["window-proportional"], "{media}");
    }
    let closed = [Value::Null, Value::Null, Value::Null, Value::Null];
    assert_eq!(
        scratch.diffusions(false).await,
        [diffusion(battlemap(), closed, 1.0, (0.0, 0.0))]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn no_live_session_or_a_media_not_held_is_refused_and_nothing_is_queued() {
    let scratch = Scratch::new("refused").await;
    let engine = scratch.serve().await;
    let refused = |result: Result<Submitted>, why: &str| match result {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::Unprocessable, "{why}");
            assert_eq!(refusal.message, why);
        }
        other => panic!("{why}: {other:?}"),
    };
    refused(
        window(&engine, None, BRUME, BRUMEUX).await,
        "aucune séance n'est en cours",
    );
    refused(
        window(
            &engine,
            None,
            VALOMBRE,
            "d1000000-0000-4000-8000-00000000ffff",
        )
        .await,
        "ce média n'existe pas",
    );

    // The engine's soft deletes, their rows tombstoned, the tables' left as
    // they were: of the portrait, then of the live session.
    for (partition, why) in [
        (format!("Media/{PORTRAIT}"), "ce média n'existe pas"),
        (format!("Session/{SEANCE}"), "aucune séance n'est en cours"),
    ] {
        scratch
            .client
            .execute(
                "INSERT INTO dataguard_rows (partition, tombstoned) VALUES ($1, true)",
                &[&partition],
            )
            .await
            .expect("deleted");
        refused(window(&engine, None, VALOMBRE, PORTRAIT).await, why);
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert!(scratch.diffusions(true).await.is_empty());
    scratch.drop().await;
}
