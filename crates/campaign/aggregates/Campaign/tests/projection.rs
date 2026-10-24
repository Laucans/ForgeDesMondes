//! The Campaign aggregate's projection over PostgreSQL (`DATABASE_URL`), as
//! the store runs it inside the transaction that applies a write: a
//! creation's campaign reaches `campaign`, a oneshot's with its single
//! `session` (`oneshot-single-session`), once — a creation the tables lost
//! is written again when the store starts, what they refuse rejects the
//! command; an export's stamp reaches `campaign.last_exported_at` and no other column
//! of any table (`export-stamps-date`), never moved back; a stamp the table
//! lost is written again when the store starts; a campaign the fixtures
//! wrote into the table is the engine's row until the engine writes its
//! own, and one the engine deleted, or the table no longer holds, is none.
//! Each test works in a scratch database of its own, migrated as the server
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
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::postgres::{PgStore, SystemClock};
use harness_dataguard::store::Store;
use harness_dataguard::tokio_postgres::{Client, NoTls, connect as connected};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const CLAIRE: &str = "b1000000-0000-4000-8000-000000000001";
const LE_BAL: &str = "c1000000-0000-4000-8000-000000000003";
const ITS_SESSION: &str = "d1000000-0000-4000-8000-000000000003";

/// The product's export as the engine sees it once prepared: the stamp
/// alone (its `Prepare` is its crate's, not this aggregate's).
const EXPORT: &str = r#"{"dataCapability":"campaign.exportCampaign","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Campaign","id":"$id"},"touches":["Campaign.lastExportedAt"],"payload":{"id":"uuid"},"mode":"overwrite","invariants":["export-stamps-date"],"permissions":[],"callableBy":["*"],"idempotencyKey":"optional"}"#;
/// The product's creation as the engine sees it once prepared (its
/// `Prepare` is its crate's, not this aggregate's).
const CREATE_CAMPAIGN: &str = r#"{"dataCapability":"campaign.createCampaign","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"touches":["Campaign.id","Campaign.name","Campaign.kind","Campaign.pitch","Campaign.art","Campaign.glyph","Campaign.createdAt","Campaign.soundMasterVolume"],"payload":{"name":"string","type":"string"},"mode":"overwrite","invariants":["oneshot-single-session","master-volume-range"],"permissions":[],"callableBy":["*"],"idempotencyKey":"optional"}"#;
/// A campaign created under the id its payload names, test-only.
const CREATE: &str = r#"{"dataCapability":"test.createCampaign","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A campaign's fields changed, test-only.
const UPDATE: &str = r#"{"dataCapability":"test.updateCampaign","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
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

/// A database of its own, migrated as the server migrates it — or not at
/// all — and holding, as the fixtures write them, Valombre, Claire its
/// player, and Brume, another campaign.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Self {
        let scratch = Self::empty(name).await;
        let mut client = connect(&scratch.url).await;
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
        data::migrate(&mut client, &migrations)
            .await
            .expect("migrated");
        scratch
            .client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind, pitch, sound_master_volume)
                     VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne', 'Une cité sous la cendre.', 0.8),
                            ('{BRUME}', 'La Brume', 'oneshot', NULL, 1);
                 INSERT INTO player (id, campaign_id, name, notes) VALUES ('{CLAIRE}', '{VALOMBRE}', 'Claire', 'Joue Elowen.');"
            ))
            .await
            .expect("Valombre, Claire and Brume");
        scratch
    }

    /// A database of its own, no table of the product's migrated.
    async fn empty(name: &str) -> Self {
        let name = format!("campaign_projection_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let client = connect(&url).await;
        Self { name, url, client }
    }

    /// The store over this database, the aggregate's projection warmed, and
    /// the engine over it as the server plants the aggregate, with the
    /// test-only DataCapabilities.
    async fn plant(&self) -> (Arc<PgStore>, Engine) {
        let plugins = std::iter::once(aggregate_campaign::plugin()).collect();
        let store = Arc::new(
            PgStore::open(connect(&self.url).await)
                .await
                .expect("the store")
                .with_projections(&plugins)
                .await
                .expect("the projection warmed"),
        );
        let mut engine = Engine::new(
            store.clone(),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_aggregate(Aggregate::parse(aggregate_campaign::AGGREGATE_JSON).expect("contract I"))
        .with_plugins(plugins);
        for manifest in [EXPORT, CREATE, UPDATE, DELETE, CREATE_CAMPAIGN] {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        engine.load().await.expect("the engine loaded");
        (store, engine)
    }

    /// Every row of every table of the product, as JSON, by table: the
    /// DataGuard's own (`dataguard_*`) and the migrations' record aside.
    async fn tables(&self) -> BTreeMap<String, Vec<Value>> {
        let names = self
            .client
            .query(
                "SELECT tablename::text FROM pg_tables
                 WHERE schemaname = 'public' AND tablename NOT LIKE 'dataguard%'
                     AND tablename <> 'data_migrations'
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

    /// The rows `sql` selects, each as JSON.
    async fn rows(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(&format!("SELECT to_jsonb(q) FROM ({sql}) q"), &[])
            .await
            .expect("the tables")
            .iter()
            .map(|row| row.get::<_, Value>(0))
            .collect()
    }

    /// The campaign `id` as its table holds it, if it does.
    async fn campaign(&self, id: &str) -> Option<Value> {
        self.rows(&format!(
            "SELECT id::text, name, kind, pitch, art, glyph, created_at = '2026-10-21T09:00:00.123Z' AS created,
                 sound_master_volume::text AS volume
             FROM campaign WHERE id = '{id}'"
        ))
        .await
        .pop()
    }

    /// The sessions of the campaign `id`, as their table holds them.
    async fn sessions(&self, id: &str) -> Vec<Value> {
        self.rows(&format!(
            "SELECT id::text, position, title, status, microphone_on FROM session
             WHERE campaign_id = '{id}' ORDER BY position"
        ))
        .await
    }

    /// Whether `campaign`'s row holds no export.
    async fn never_exported(&self, campaign: &str) -> bool {
        self.client
            .query_one(
                "SELECT last_exported_at IS NULL FROM campaign WHERE id = ($1::text)::uuid",
                &[&campaign],
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

/// `data_capability` submitted on `Campaign/<target>`: what the applier made
/// of it.
async fn written(
    engine: &Engine,
    data_capability: &str,
    target: &str,
    payload: Value,
) -> CommandResult {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    engine
        .submit(Submission {
            data_capability: data_capability.to_string(),
            payload: payload.into_iter().collect(),
            target_id: Some(target.to_string()),
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "CampaignOverview".to_string(),
                permissions: Vec::new(),
                // The critical fields' review is not what these tests judge.
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    engine.run_once().await.expect("the applier ran").remove(0)
}

async fn exported(engine: &Engine, campaign: &str, at: &str) -> CommandResult {
    written(
        engine,
        "campaign.exportCampaign@1",
        campaign,
        json!({ "lastExportedAt": at }),
    )
    .await
}

#[tokio::test]
async fn an_export_of_a_campaign_the_fixtures_wrote_stamps_last_exported_at_and_nothing_else() {
    let scratch = Scratch::new("stamp").await;
    let (_, engine) = scratch.plant().await;
    let before = scratch.tables().await;
    assert!(before.contains_key("campaign") && before.contains_key("player"));

    let result = exported(&engine, VALOMBRE, "2026-10-18T09:30:00.123Z").await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert!(
        scratch
            .exported_at(VALOMBRE, "2026-10-18T09:30:00.123Z")
            .await
    );

    let mut after = scratch.tables().await;
    for campaign in after.get_mut("campaign").expect("the campaign table") {
        if campaign["id"] == VALOMBRE {
            campaign["last_exported_at"] = Value::Null;
        }
    }
    after
        .get_mut("campaign")
        .expect("the campaign table")
        .sort_by_key(Value::to_string);
    let mut expected = before;
    expected
        .get_mut("campaign")
        .expect("the campaign table")
        .sort_by_key(Value::to_string);
    assert_eq!(after, expected);
    assert!(scratch.never_exported(BRUME).await);
    scratch.drop().await;
}

#[tokio::test]
async fn a_stamp_is_never_moved_back_and_one_that_is_no_time_is_left_aside() {
    let scratch = Scratch::new("newest").await;
    let (_, engine) = scratch.plant().await;
    let later = "2026-10-18T09:30:00Z";
    assert_eq!(
        exported(&engine, VALOMBRE, later).await.status,
        State::Applied
    );
    // Written straight to the engine, an earlier stamp moves nothing.
    let earlier = written(
        &engine,
        "test.updateCampaign@1",
        VALOMBRE,
        json!({ "lastExportedAt": "2026-10-01T00:00:00Z" }),
    )
    .await;
    assert_eq!(earlier.status, State::Applied, "{earlier:?}");
    assert!(scratch.exported_at(VALOMBRE, later).await);
    // Nor does a stamp that is no time, which fails no write.
    let garbled = written(
        &engine,
        "test.updateCampaign@1",
        BRUME,
        json!({ "lastExportedAt": "pas une date" }),
    )
    .await;
    assert_eq!(garbled.status, State::Applied, "{garbled:?}");
    assert!(scratch.never_exported(BRUME).await);
    scratch.drop().await;
}

#[tokio::test]
async fn a_stamp_the_table_lost_is_written_again_when_the_store_starts_whatever_another_campaign_holds()
 {
    let scratch = Scratch::new("warm").await;
    let (_, engine) = scratch.plant().await;
    let at = "2026-10-18T09:30:00.123Z";
    assert_eq!(exported(&engine, VALOMBRE, at).await.status, State::Applied);
    let garbled = written(
        &engine,
        "test.updateCampaign@1",
        BRUME,
        json!({ "lastExportedAt": "pas une date" }),
    )
    .await;
    assert_eq!(garbled.status, State::Applied, "{garbled:?}");
    drop(engine);
    scratch
        .client
        .batch_execute("UPDATE campaign SET last_exported_at = NULL")
        .await
        .expect("the stamp lost");
    let (_store, _engine) = scratch.plant().await;
    assert!(scratch.exported_at(VALOMBRE, at).await);
    assert!(scratch.never_exported(BRUME).await);
    scratch.drop().await;
}

#[tokio::test]
async fn a_campaign_the_fixtures_wrote_is_hydrated_until_the_engine_writes_its_own() {
    let scratch = Scratch::new("hydrate").await;
    let (store, engine) = scratch.plant().await;
    let partition = format!("Campaign/{VALOMBRE}");
    let row = store.row(&partition).await.expect("the row");
    assert_eq!(row["id"], VALOMBRE);
    assert_eq!(row["name"], "Les Cendres de Valombre");
    assert_eq!(row["kind"], "campagne");
    assert_eq!(row["pitch"], "Une cité sous la cendre.");
    assert!(!row.contains_key("lastExportedAt"), "{row:?}");
    // Its id in capitals is the same campaign.
    let shouted = store
        .row(&format!("Campaign/{}", VALOMBRE.to_uppercase()))
        .await
        .expect("the row");
    assert_eq!(shouted["name"], "Les Cendres de Valombre");

    assert_eq!(
        exported(&engine, VALOMBRE, "2026-10-18T09:30:00Z")
            .await
            .status,
        State::Applied
    );
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE campaign SET name = 'Valombre renaît' WHERE id = '{VALOMBRE}'"
        ))
        .await
        .expect("renamed");
    // The engine's row is its own once written: what it applied, the table's
    // fields it was judged on with it.
    let row = store.row(&partition).await.expect("the row");
    assert_eq!(row["name"], "Les Cendres de Valombre");
    assert_eq!(row["kind"], "campagne");
    assert_eq!(row["lastExportedAt"], "2026-10-18T09:30:00Z");
    scratch.drop().await;
}

#[tokio::test]
async fn a_campaign_the_engine_deleted_or_the_table_no_longer_holds_is_exported_no_more() {
    let scratch = Scratch::new("deleted").await;
    let (store, engine) = scratch.plant().await;
    let deleted = written(&engine, "test.deleteCampaign@1", BRUME, json!({})).await;
    assert_eq!(deleted.status, State::Applied, "{deleted:?}");
    assert_eq!(
        store
            .row(&format!("Campaign/{BRUME}"))
            .await
            .expect("the row"),
        BTreeMap::new()
    );
    let result = exported(&engine, BRUME, "2026-10-18T09:30:00Z").await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert!(
        result.violations.iter().any(|v| v == "export-stamps-date"),
        "{result:?}"
    );
    assert!(scratch.never_exported(BRUME).await);

    scratch
        .client
        .batch_execute(&format!("DELETE FROM campaign WHERE id = '{VALOMBRE}'"))
        .await
        .expect("Valombre deleted");
    let result = exported(&engine, VALOMBRE, "2026-10-18T09:30:00Z").await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    scratch.drop().await;
}

#[tokio::test]
async fn a_database_without_the_campaign_table_starts_and_holds_no_campaign() {
    let scratch = Scratch::empty("unmigrated").await;
    let (store, engine) = scratch.plant().await;
    assert_eq!(
        store
            .row(&format!("Campaign/{VALOMBRE}"))
            .await
            .expect("the row"),
        BTreeMap::new()
    );
    let created = written(
        &engine,
        "test.createCampaign@1",
        VALOMBRE,
        json!({ "id": VALOMBRE, "kind": "campagne", "lastExportedAt": "2026-10-18T09:30:00Z" }),
    )
    .await;
    assert_eq!(created.status, State::Applied, "{created:?}");
    scratch.drop().await;
}

/// Le Bal, a oneshot, as `campaign.createCampaign` prepares it: with its one
/// session, of this campaign, at position 1, in prep, the microphone off.
fn le_bal() -> Value {
    json!({
        "id": LE_BAL,
        "name": "Le Bal",
        "kind": "oneshot",
        "pitch": "Une nuit masquée.",
        "art": "dusk",
        "glyph": null,
        "createdAt": "2026-10-21T09:00:00.123Z",
        "soundMasterVolume": 1.0,
        "sessionId": ITS_SESSION,
        "sessionCampaignId": LE_BAL,
        "sessionPosition": 1,
        "sessionTitle": "Le Bal",
        "sessionStatus": "prep",
        "microphoneOn": false,
    })
}

/// Le Bal as a campagne: with no session.
fn le_bal_campagne() -> Value {
    let mut campagne = le_bal();
    let fields = campagne.as_object_mut().expect("an object");
    fields.retain(|field, _| !field.starts_with("session") && field != "microphoneOn");
    fields.insert("kind".into(), json!("campagne"));
    campagne
}

async fn created(engine: &Engine, payload: Value) -> CommandResult {
    written(engine, "campaign.createCampaign@1", LE_BAL, payload).await
}

#[tokio::test]
async fn oneshot_single_session_a_created_oneshot_reaches_its_table_with_its_one_session_in_prep() {
    let scratch = Scratch::new("create_oneshot").await;
    let (_, engine) = scratch.plant().await;
    let result = created(&engine, le_bal()).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        scratch.campaign(LE_BAL).await,
        Some(json!({
            "id": LE_BAL, "name": "Le Bal", "kind": "oneshot", "pitch": "Une nuit masquée.",
            "art": "dusk", "glyph": null, "created": true, "volume": "1.00",
        }))
    );
    assert_eq!(
        scratch.sessions(LE_BAL).await,
        [json!({
            "id": ITS_SESSION, "position": 1, "title": "Le Bal", "status": "prep",
            "microphone_on": false,
        })]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn master_volume_range_a_created_campagne_reaches_its_table_in_range_with_no_session() {
    let scratch = Scratch::new("create_campagne").await;
    let (_, engine) = scratch.plant().await;
    let result = created(&engine, le_bal_campagne()).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let campaign = scratch.campaign(LE_BAL).await.expect("Le Bal");
    assert_eq!(campaign["kind"], "campagne");
    let volume: f64 = campaign["volume"]
        .as_str()
        .and_then(|volume| volume.parse().ok())
        .expect("a volume");
    assert!((0.0..=1.0).contains(&volume), "{volume}");
    assert_eq!(scratch.sessions(LE_BAL).await, Vec::<Value>::new());
    scratch.drop().await;
}

#[tokio::test]
async fn only_a_creation_writes_a_campaign_into_its_table() {
    let scratch = Scratch::new("create_only").await;
    let (_, engine) = scratch.plant().await;
    let before = scratch.tables().await;
    let result = written(&engine, "test.createCampaign@1", LE_BAL, le_bal()).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_creation_the_tables_lost_is_written_again_when_the_store_starts_and_a_session_deleted_never_comes_back()
 {
    let scratch = Scratch::new("create_warm").await;
    let (_, engine) = scratch.plant().await;
    assert_eq!(created(&engine, le_bal()).await.status, State::Applied);
    drop(engine);
    scratch
        .client
        .batch_execute(&format!("DELETE FROM campaign WHERE id = '{LE_BAL}'"))
        .await
        .expect("Le Bal lost, its session with it");
    let (_store, engine) = scratch.plant().await;
    assert!(scratch.campaign(LE_BAL).await.is_some());
    assert_eq!(scratch.sessions(LE_BAL).await.len(), 1);
    drop(engine);

    scratch
        .client
        .batch_execute(&format!(
            "DELETE FROM session WHERE campaign_id = '{LE_BAL}'"
        ))
        .await
        .expect("its session deleted");
    let (_store, _engine) = scratch.plant().await;
    assert_eq!(scratch.sessions(LE_BAL).await, Vec::<Value>::new());
    scratch.drop().await;
}

#[tokio::test]
async fn a_creation_the_tables_refuse_is_rejected_and_nothing_is_written() {
    let scratch = Scratch::new("create_refused").await;
    let (store, engine) = scratch.plant().await;
    let before = scratch.tables().await;
    // No name: `campaign.name` is NOT NULL.
    let mut nameless = le_bal();
    nameless.as_object_mut().expect("an object").remove("name");
    let result = created(&engine, nameless).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(scratch.tables().await, before);
    assert_eq!(
        store
            .row(&format!("Campaign/{LE_BAL}"))
            .await
            .expect("the row"),
        BTreeMap::new()
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_created_oneshot_whose_session_the_tables_cannot_hold_is_rejected_whole() {
    let scratch = Scratch::new("create_bad_session").await;
    let (_, engine) = scratch.plant().await;
    let before = scratch.tables().await;
    for (field, value) in [
        ("sessionId", json!("pas un uuid")),
        ("sessionTitle", Value::Null),
    ] {
        let mut payload = le_bal();
        payload[field] = value;
        let result = created(&engine, payload).await;
        assert_eq!(result.status, State::Rejected, "{field}: {result:?}");
    }
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_creation_whose_projection_cannot_run_fails_the_step_and_stays_queued() {
    let scratch = Scratch::new("create_failed").await;
    // A database whose session table is gone: the creation of a oneshot
    // cannot be written, which is no refusal of the command.
    scratch
        .client
        .batch_execute("DROP TABLE session CASCADE")
        .await
        .expect("the session table dropped");
    let (_, engine) = scratch.plant().await;
    let Value::Object(payload) = le_bal() else {
        panic!("a payload is an object");
    };
    engine
        .submit(Submission {
            data_capability: "campaign.createCampaign@1".to_string(),
            payload: payload.into_iter().collect(),
            target_id: Some(LE_BAL.to_string()),
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "CampaignList".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    for _ in 0..2 {
        let failed = engine.run_once().await;
        assert!(
            matches!(&failed, Err(harness_dataguard::Error::Store(message)) if message.contains("session")),
            "{failed:?}"
        );
    }
    assert_eq!(scratch.campaign(LE_BAL).await, None);
    scratch.drop().await;
}
