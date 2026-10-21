//! campaign.exportCampaign over PostgreSQL (`DATABASE_URL`), as the server
//! plants it beside its aggregate: an export of a campaign the tables hold —
//! one the fixtures wrote, which no command wrote before — stamps it, and
//! its command's result, once applied and never before, answers the archive
//! of what the tables hold: the campaign, its players, its elements with
//! their sheets, aliases and versions. An export of a campaign they do not
//! hold is refused before anything is queued, and changes nothing. Each
//! test works in a scratch database of its own, migrated as the server
//! migrates it, dropped when it ends.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier};
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::postgres::{PgStore, ReadOnly};
use harness_dataguard::tokio_postgres::{Client, NoTls, connect as connected};
use harness_dataguard::{Error, plugins::Plugins};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const NOWHERE: &str = "c1000000-0000-4000-8000-0000000000ff";
const CLAIRE: &str = "b1000000-0000-4000-8000-000000000001";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const TOWER: &str = "e1000000-0000-4000-8000-000000000002";
/// 2026-10-18T09:30:00.123Z.
const NOW_MILLIS: u64 = 1_792_315_800_123;
const NOW: &str = "2026-10-18T09:30:00.123Z";

/// A campaign deleted, test-only: the engine's soft delete, a tombstone.
const DELETE: &str = r#"{"dataCapability":"test.deleteCampaign","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

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

/// A database of its own, migrated, holding — as the fixtures write them —
/// Valombre, Claire its player, Elowen her PC with her sheet, two aliases
/// and two versions, the Tour d'Ambre, a place, and Brume, another
/// campaign.
struct Scratch {
    name: String,
    url: String,
    client: Client,
    clock: Arc<FixedClock>,
}

impl Scratch {
    async fn new(name: &str) -> Self {
        let name = format!("export_campaign_test_{name}_{}", std::process::id());
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
                "INSERT INTO campaign (id, name, kind, pitch, sound_master_volume)
                     VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne', 'Une cité sous la cendre.', 0.8),
                            ('{BRUME}', 'La Brume', 'oneshot', NULL, 1);
                 INSERT INTO player (id, campaign_id, name, notes) VALUES ('{CLAIRE}', '{VALOMBRE}', 'Claire', 'Joue Elowen.');
                 INSERT INTO entity (id, campaign_id, type, name, notes, created_at)
                     VALUES ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen Ventargent', 'A retrouvé Aerin.', '2026-10-01T00:00:00Z'),
                            ('{TOWER}', '{VALOMBRE}', 'lieu', 'La Tour d''Ambre', NULL, '2026-10-02T00:00:00Z');
                 INSERT INTO character (entity_id, player_id, hp_max, token_color)
                     VALUES ('{ELOWEN}', '{CLAIRE}', 44, 'success');
                 INSERT INTO entity_alias (entity_id, alias) VALUES ('{ELOWEN}', 'la Rôdeuse'), ('{ELOWEN}', 'Elowen');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                     VALUES ('{ELOWEN}', 1, 'dm', '{{\"notes\": \"Cherche son frère.\"}}', '[]'),
                            ('{ELOWEN}', 2, 'dm', '{{\"notes\": \"A retrouvé Aerin.\"}}', '[]'),
                            ('{TOWER}', 1, 'dm', '{{}}', '[]');"
            ))
            .await
            .expect("the fixtures");
        Self {
            name,
            url,
            client,
            clock: Arc::new(FixedClock::at_millis(NOW_MILLIS)),
        }
    }

    /// The engine as the server plants the export and its aggregate over this
    /// database, reading its tables, with the test-only deletion.
    async fn plant(&self) -> Engine {
        let plugins: Plugins = [export_campaign::plugin(), aggregate_campaign::plugin()]
            .into_iter()
            .collect();
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins)
            .await
            .expect("the projection warmed");
        let engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            self.clock.clone(),
        )
        .with_aggregate(Aggregate::parse(aggregate_campaign::AGGREGATE_JSON).expect("contract I"))
        .with_data_capability(
            DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
        )
        .with_data_capability(DataCapability::parse(DELETE).expect("contract F"))
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

    /// Whether `campaign`'s last export is the instant `at` names.
    async fn exported_at(&self, campaign: &str, at: &str) -> bool {
        self.client
            .query_one(
                "SELECT last_exported_at IS NOT DISTINCT FROM ($2::text)::timestamptz
                 FROM campaign WHERE id = ($1::text)::uuid",
                &[&campaign, &at],
            )
            .await
            .expect("the campaign")
            .get(0)
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

/// `data_capability` submitted as the CampaignOverview panel does, on the
/// target `target_id` names: what the engine answers.
async fn submitted(
    engine: &Engine,
    data_capability: &str,
    payload: Value,
    target_id: Option<&str>,
) -> harness_dataguard::Result<harness_dataguard::api::Submitted> {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    engine
        .submit(Submission {
            data_capability: data_capability.to_owned(),
            payload: payload.into_iter().collect(),
            target_id: target_id.map(str::to_owned),
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_owned(),
                from: "CampaignOverview".to_owned(),
                permissions: Vec::new(),
                roles: Vec::new(),
            },
        })
        .await
}

/// An export queued, applied: its result.
async fn exported(engine: &Engine, payload: Value, target_id: Option<&str>) -> CommandResult {
    let queued = submitted(engine, "campaign.exportCampaign@1", payload, target_id)
        .await
        .expect("queued");
    assert_eq!(queued.result.attachment, None, "not before it is applied");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        engine
            .status(&queued.command_id)
            .await
            .expect("its status")
            .attachment,
        result.attachment,
        "answered once applied"
    );
    result
}

#[tokio::test]
async fn the_archive_holds_the_campaign_its_players_and_its_elements_with_sheets_aliases_and_versions()
 {
    let scratch = Scratch::new("archive").await;
    let engine = scratch.plant().await;

    let archive = exported(&engine, json!({ "id": VALOMBRE }), None)
        .await
        .attachment
        .expect("the archive");
    assert_eq!(
        archive["file"],
        "les-cendres-de-valombre-2026-10-18T09-30-00-123Z.json"
    );
    assert!(scratch.exported_at(VALOMBRE, NOW).await);
    let content = &archive["content"];
    assert_eq!(content["format"], "campaign-archive");
    assert_eq!(content["version"], 1);
    assert_eq!(content["origin_campaign_id"], VALOMBRE);
    assert_eq!(content["exported_at"], NOW);
    assert_eq!(content["campaign"]["id"], VALOMBRE);
    assert_eq!(content["campaign"]["name"], "Les Cendres de Valombre");
    assert_eq!(content["campaign"]["pitch"], "Une cité sous la cendre.");
    // As it was before this export: never exported.
    assert_eq!(content["campaign"]["last_exported_at"], Value::Null);

    let players = content["players"].as_array().expect("players");
    assert_eq!(players.len(), 1, "{players:?}");
    assert_eq!(players[0]["id"], CLAIRE);
    assert_eq!(players[0]["notes"], "Joue Elowen.");

    let elements = content["elements"].as_array().expect("elements");
    let ids: Vec<&Value> = elements.iter().map(|e| &e["id"]).collect();
    assert_eq!(ids, [ELOWEN, TOWER], "oldest first");
    let pc = &elements[0];
    assert_eq!(pc["type"], "pj");
    assert_eq!(pc["notes"], "A retrouvé Aerin.");
    assert_eq!(pc["character"]["player_id"], CLAIRE);
    assert_eq!(pc["character"]["hp_max"], 44);
    assert_eq!(pc["aliases"], json!(["Elowen", "la Rôdeuse"]));
    let versions = pc["versions"].as_array().expect("versions");
    let numbers: Vec<&Value> = versions.iter().map(|v| &v["version_number"]).collect();
    assert_eq!(numbers, [1, 2]);
    assert_eq!(versions[0]["snapshot"]["notes"], "Cherche son frère.");
    let place = &elements[1];
    assert_eq!(place["type"], "lieu");
    assert_eq!(place["character"], Value::Null);
    assert_eq!(place["aliases"], json!([]));
    assert_eq!(place["versions"].as_array().map(Vec::len), Some(1));

    // Another campaign's rows are not this one's.
    let brume = exported(&engine, json!({ "id": BRUME }), None)
        .await
        .attachment
        .expect("the archive");
    assert_eq!(brume["content"]["players"], json!([]));
    assert_eq!(brume["content"]["elements"], json!([]));
    scratch.drop().await;
}

#[tokio::test]
async fn an_export_named_by_its_target_or_in_any_case_stamps_the_one_campaign_and_each_export_its_own_time()
 {
    let scratch = Scratch::new("target").await;
    let engine = scratch.plant().await;
    // As the rules' tests name it: the target, and an empty payload.
    let first = exported(&engine, json!({}), Some(VALOMBRE)).await;
    assert_eq!(
        first.attachment.expect("an archive")["content"]["exported_at"],
        NOW
    );
    scratch.clock.set_millis(NOW_MILLIS + 5);
    let second = exported(&engine, json!({ "id": VALOMBRE.to_uppercase() }), None)
        .await
        .attachment
        .expect("an archive");
    let content = &second["content"];
    assert_eq!(content["origin_campaign_id"], VALOMBRE);
    assert_eq!(content["exported_at"], "2026-10-18T09:30:00.128Z");
    assert!(
        scratch
            .exported_at(VALOMBRE, "2026-10-18T09:30:00.128Z")
            .await
    );
    // The archive holds the campaign as the export before left it.
    let shown = content["campaign"]["last_exported_at"]
        .as_str()
        .expect("the previous export");
    let previous: bool = scratch
        .client
        .query_one(
            "SELECT ($1::text)::timestamptz = ($2::text)::timestamptz",
            &[&shown, &NOW],
        )
        .await
        .expect("compared")
        .get(0);
    assert!(previous, "{shown} is {NOW}");
    scratch.drop().await;
}

#[tokio::test]
async fn an_export_of_a_campaign_the_tables_do_not_hold_is_refused_and_changes_nothing() {
    let scratch = Scratch::new("refused").await;
    let engine = scratch.plant().await;
    let before = scratch.tables().await;
    // A campaign the tables do not hold, or none named: refused, not queued.
    for (payload, target) in [
        (json!({ "id": NOWHERE }), None),
        (json!({ "id": "pas-un-uuid" }), None),
        (json!({}), None),
        (json!({}), Some(NOWHERE)),
    ] {
        match submitted(&engine, "campaign.exportCampaign@1", payload, target).await {
            Err(Error::Refused(refusal)) => {
                assert_eq!(refusal.kind, RefusalKind::NotFound);
                assert_eq!(refusal.message, "cette campagne n'existe pas");
            }
            other => panic!("refused, not {other:?}"),
        }
    }
    assert_eq!(
        engine.run_once().await.expect("the applier ran"),
        Vec::new()
    );
    assert_eq!(scratch.tables().await, before);

    // Nor is one the engine deleted.
    submitted(&engine, "test.deleteCampaign@1", json!({}), Some(BRUME))
        .await
        .expect("queued");
    let deleted = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(deleted.status, State::Applied, "{deleted:?}");
    assert!(matches!(
        submitted(
            &engine,
            "campaign.exportCampaign@1",
            json!({ "id": BRUME }),
            None
        )
        .await,
        Err(Error::Refused(refusal)) if refusal.kind == RefusalKind::NotFound
    ));
    scratch.drop().await;
}
