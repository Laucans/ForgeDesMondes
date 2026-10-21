//! live.showOnPlayerView over PostgreSQL (`DATABASE_URL`), as the server
//! plants it: a send is prepared against the live session, the instance and
//! the media the tables hold, judged by the DataGuard and projected onto the
//! session's `diffusion` row in the transaction that applies it. The first
//! send makes the row, a later one replaces its base on that same row; an
//! instance of another session, or a media that is no illustration of the
//! campaign, is rejected under base-coherent and the row stays as it was;
//! two sends in a row are both queued and applied, in order. Each test works
//! in a scratch database of its own, migrated as the server migrates it,
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
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};
use show_on_player_view::{NOT_AN_ILLUSTRATION, NOT_OF_THE_SESSION};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's session before the live one, closed.
const SEANCE_1: &str = "51000000-0000-4000-8000-000000000001";
/// Valombre's live session.
const SEANCE: &str = "51000000-0000-4000-8000-000000000002";
/// Brume's session, still in preparation: Brume has no live session.
const SEANCE_BRUME: &str = "51000000-0000-4000-8000-000000000003";
const SCENE_1: &str = "5c000000-0000-4000-8000-000000000001";
const SCENE: &str = "5c000000-0000-4000-8000-000000000002";
/// Valombre's battlemap of the ford.
const CARTE: &str = "a1000000-0000-4000-8000-000000000001";
/// An illustration of Valombre, 800 × 1200.
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
/// An illustration of Brume.
const PHARE: &str = "d1000000-0000-4000-8000-000000000003";
/// The ford's instance in the live session.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
/// The ford's instance in the session before.
const AUTRE: &str = "b1000000-0000-4000-8000-000000000002";

/// What the engine adds, judging the state the send would leave at once.
const AT_RISK: &str = "invariant(s) at risk: base-coherent";

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
/// one, a scene in each, its battlemap of the ford instanced in both, and
/// its portrait; Brume with a session in preparation and its lighthouse.
/// No session has a diffusion row.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("show_on_player_view_test_{name}_{}", std::process::id());
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
                     ('{SCENE_1}', '{SEANCE_1}', 1, 'Le Gué'),
                     ('{SCENE}', '{SEANCE}', 1, 'Le Gué, encore');
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES
                     ('{CARTE}', '{VALOMBRE}', 'battlemap', 'Le Gué', 'media/gue.png', 960, 640),
                     ('{PORTRAIT}', '{VALOMBRE}', 'illustration', 'Morlox', 'media/morlox.png', 800, 1200),
                     ('{PHARE}', '{BRUME}', 'illustration', 'Le Phare', 'media/phare.png', 800, 600);
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
                     VALUES ('{CARTE}', 30, 12, 8, 'pointe', '#202020', 0.4);
                 INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) VALUES
                     ('{INSTANCE}', '{CARTE}', '{SEANCE}', '{SCENE}'),
                     ('{AUTRE}', '{CARTE}', '{SEANCE_1}', '{SCENE_1}');"
            ))
            .await
            .expect("Valombre, Brume, their sessions and their media");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database: the aggregate's projection in the apply
    /// transaction, the tables lent to the Prepare.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins {
            [aggregate_diffusion::plugin(), show_on_player_view::plugin()]
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
    /// set.
    async fn diffusions(&self) -> Vec<Value> {
        self.client
            .query(
                "SELECT to_jsonb(d)::text FROM diffusion d ORDER BY session_id",
                &[],
            )
            .await
            .expect("the diffusion table")
            .iter()
            .map(|row| {
                let mut row: Value = serde_json::from_str(row.get::<_, &str>(0)).expect("JSON");
                let stamp = row
                    .as_object_mut()
                    .expect("a row")
                    .remove("updated_at")
                    .expect("its stamp");
                assert!(stamp.is_string(), "{stamp}");
                row
            })
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

/// A send of `payload`, by the DM, approver of their own campaign's critical
/// fields — the session and the kind of its base.
async fn send(engine: &Engine, payload: Value) -> Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "live.showOnPlayerView@1",
        "payload": payload,
        "caller": { "by": "dm", "from": "table.Battlemap", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

/// The live session's row showing `kind`, its `media` or its `instance`,
/// framed at `zoom` and `pan`, no window open.
fn showing(
    kind: &str,
    media: Option<&str>,
    instance: Option<&str>,
    zoom: f64,
    pan: (f64, f64),
) -> Value {
    json!({
        "session_id": SEANCE, "base_kind": kind,
        "base_media_id": media, "base_instance_id": instance,
        "window_media_id": null, "window_x": null, "window_y": null, "window_width": null,
        "view_zoom": zoom, "view_pan_x": pan.0, "view_pan_y": pan.1,
    })
}

#[tokio::test]
async fn the_first_send_makes_the_live_session_s_row_and_a_battlemap_then_replaces_its_base() {
    let scratch = Scratch::new("first").await;
    let engine = scratch.serve().await;

    let submitted = send(
        &engine,
        json!({ "campaign": VALOMBRE, "kind": "illustration", "media": PORTRAIT }),
    )
    .await
    .expect("queued");
    assert_eq!(submitted.partition, format!("Diffusion/{SEANCE}"));
    assert!(submitted.warnings.is_empty(), "{submitted:?}");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        scratch.diffusions().await,
        [showing(
            "illustration",
            Some(PORTRAIT),
            None,
            1.0,
            (0.0, 0.0)
        )]
    );

    // The Battlemap names no campaign: the instance's is the one.
    send(
        &engine,
        json!({ "kind": "battlemap", "instance": INSTANCE.to_uppercase(),
                "zoom": 1.5, "panX": 120.5, "panY": -40 }),
    )
    .await
    .expect("queued");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        scratch.diffusions().await,
        [showing(
            "battlemap",
            None,
            Some(INSTANCE),
            1.5,
            (120.5, -40.0)
        )]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn an_instance_of_another_session_is_rejected_under_base_coherent_and_the_row_stays() {
    let scratch = Scratch::new("other_session").await;
    let engine = scratch.serve().await;
    send(
        &engine,
        json!({ "campaign": VALOMBRE, "kind": "illustration", "media": PORTRAIT }),
    )
    .await
    .expect("queued");
    engine.run_once().await.expect("the applier ran");
    let before = scratch.diffusions().await;

    let submitted = send(
        &engine,
        json!({ "campaign": VALOMBRE, "kind": "battlemap", "instance": AUTRE, "zoom": 2 }),
    )
    .await
    .expect("queued, for the DataGuard to judge");
    assert_eq!(
        submitted.warnings,
        [NOT_OF_THE_SESSION, AT_RISK],
        "{submitted:?}"
    );
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["base-coherent"], "{result:?}");
    assert_eq!(scratch.diffusions().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_media_that_is_no_illustration_of_the_campaign_is_rejected_under_base_coherent() {
    let scratch = Scratch::new("other_campaign").await;
    let engine = scratch.serve().await;
    for media in [PHARE, CARTE] {
        let submitted = send(
            &engine,
            json!({ "campaign": VALOMBRE, "kind": "illustration", "media": media }),
        )
        .await
        .expect("queued, for the DataGuard to judge");
        assert_eq!(
            submitted.warnings,
            [NOT_AN_ILLUSTRATION, AT_RISK],
            "{media}"
        );
        let result = engine.run_once().await.expect("the applier ran").remove(0);
        assert_eq!(result.status, State::Rejected, "{media}: {result:?}");
        assert_eq!(result.violations, ["base-coherent"], "{media}: {result:?}");
    }
    assert!(scratch.diffusions().await.is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn two_sends_in_a_row_are_both_queued_and_applied_in_order() {
    let scratch = Scratch::new("in_a_row").await;
    let engine = scratch.serve().await;
    let first = send(
        &engine,
        json!({ "kind": "illustration", "media": PORTRAIT }),
    )
    .await
    .expect("the first queued");
    let second = send(
        &engine,
        json!({ "kind": "battlemap", "instance": INSTANCE, "zoom": 0.75, "panX": 3, "panY": 4 }),
    )
    .await
    .expect("the second queued behind it");
    for submitted in [&first, &second] {
        assert_eq!(submitted.result.status, State::Queued, "{submitted:?}");
        assert_eq!(submitted.partition, format!("Diffusion/{SEANCE}"));
    }

    let applied = engine.run_once().await.expect("the applier ran");
    let order: Vec<_> = applied
        .iter()
        .map(|result| (result.command_id.clone(), result.status))
        .collect();
    assert_eq!(
        order,
        [
            (first.command_id, State::Applied),
            (second.command_id, State::Applied)
        ]
    );
    assert_eq!(
        scratch.diffusions().await,
        [showing("battlemap", None, Some(INSTANCE), 0.75, (3.0, 4.0))]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_send_keeps_the_window_a_fixture_s_row_has_open() {
    let scratch = Scratch::new("window").await;
    scratch
        .client
        .batch_execute(&format!(
            "INSERT INTO diffusion (session_id, base_kind, window_media_id, window_x, window_y,
                 window_width, view_zoom, view_pan_x, view_pan_y)
             VALUES ('{SEANCE}', 'cover', '{PORTRAIT}', 0.1, 0.2, 0.3, 1, 0, 0)"
        ))
        .await
        .expect("the fixture's diffusion");
    let engine = scratch.serve().await;
    send(
        &engine,
        json!({ "kind": "battlemap", "instance": INSTANCE, "zoom": 2 }),
    )
    .await
    .expect("queued");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    let mut expected = showing("battlemap", None, Some(INSTANCE), 2.0, (0.0, 0.0));
    for (column, value) in [
        ("window_media_id", json!(PORTRAIT)),
        ("window_x", json!(0.1)),
        ("window_y", json!(0.2)),
        ("window_width", json!(0.3)),
    ] {
        expected[column] = value;
    }
    assert_eq!(scratch.diffusions().await, [expected]);
    scratch.drop().await;
}

#[tokio::test]
async fn a_send_no_live_session_can_show_is_refused_and_not_queued() {
    let scratch = Scratch::new("refused").await;
    let engine = scratch.serve().await;
    let unknown = "b1000000-0000-4000-8000-00000000ffff";
    for (payload, why) in [
        (
            json!({ "campaign": BRUME, "kind": "illustration", "media": PHARE }),
            "aucune séance n'est en cours",
        ),
        (
            json!({ "kind": "battlemap", "instance": unknown }),
            "instance n'existe pas",
        ),
        (
            json!({ "kind": "cover" }),
            "on envoie aux joueurs une battlemap ou une illustration",
        ),
    ] {
        match send(&engine, payload).await {
            Err(Error::Refused(refusal)) => {
                assert_eq!(refusal.kind, RefusalKind::Unprocessable, "{why}");
                assert_eq!(refusal.message, why);
            }
            other => panic!("{why}: {other:?}"),
        }
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert!(scratch.diffusions().await.is_empty());
    scratch.drop().await;
}
