//! campaign.exportCampaign as the engine runs its `Prepare`, over its
//! memory: what the tables answer (`Context::rows`, here a stand-in that
//! answers what a test gives it) made the stamp the command writes and the
//! archive its result answers once applied — and every export the tables
//! cannot make an archive of refused before anything is queued.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use export_campaign::{ARCHIVE, plugin, timestamp};
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::reads::Rows;
use harness_dataguard::{Error, plugins::RefusalKind};
use serde_json::{Value, json};

/// A campaign created under the id its payload names, test-only: no
/// DataCapability of the product creates one yet.
const CREATE: &str = r#"{"dataCapability":"test.createCampaign","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// 2026-10-18T09:30:00.123Z.
const NOW_MILLIS: u64 = 1_792_315_800_123;
const NOW: &str = "2026-10-18T09:30:00.123Z";

/// The tables, as a test makes them answer: the rows of every query, or a
/// failure; and what they were asked.
struct Tables {
    answer: Result<Value, String>,
    asked: Mutex<Vec<(String, Vec<Value>)>>,
}

impl Tables {
    fn answering(answer: Value) -> Arc<Self> {
        Arc::new(Self {
            answer: Ok(answer),
            asked: Mutex::default(),
        })
    }

    fn failing() -> Arc<Self> {
        Arc::new(Self {
            answer: Err("connection reset".to_owned()),
            asked: Mutex::default(),
        })
    }

    fn asked(&self) -> Vec<(String, Vec<Value>)> {
        self.asked.lock().expect("asked").clone()
    }
}

#[async_trait]
impl Rows for Tables {
    async fn rows(&self, sql: &str, parameters: &[Value]) -> harness_dataguard::Result<Value> {
        self.asked
            .lock()
            .expect("asked")
            .push((sql.to_owned(), parameters.to_vec()));
        self.answer.clone().map_err(Error::Store)
    }
}

/// Valombre as `ARCHIVE` answers it: one row, its one column.
fn valombre() -> Value {
    json!([{
        "archive": {
            "campaign": { "id": VALOMBRE, "name": "Les Cendres de Valombre", "last_exported_at": null },
            "players": [{ "id": "b1", "name": "Claire" }],
            "elements": [{ "id": "e1", "type": "lieu", "aliases": [], "versions": [] }],
        }
    }])
}

/// The engine as the server plants this DataCapability and its aggregate,
/// over its memory, at `NOW`, holding Valombre; its tables `tables`, when it
/// has some.
async fn engine(tables: Option<Arc<Tables>>) -> Engine {
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at_millis(NOW_MILLIS)),
    )
    .with_aggregate(Aggregate::parse(aggregate_campaign::AGGREGATE_JSON).expect("contract I"))
    .with_data_capability(
        DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
    )
    .with_data_capability(DataCapability::parse(CREATE).expect("contract F"))
    .with_plugins(
        [plugin(), aggregate_campaign::plugin()]
            .into_iter()
            .collect(),
    );
    let engine = match tables {
        Some(tables) => engine.with_rows(tables),
        None => engine,
    };
    engine.load().await.expect("loaded");
    let mut created = exporting(
        json!({ "id": VALOMBRE, "name": "Les Cendres de Valombre", "kind": "campagne" }),
        None,
    );
    created.data_capability = "test.createCampaign@1".to_owned();
    // The critical fields' review is not what these tests judge.
    created.caller.roles = vec!["role:approver".to_owned()];
    engine.submit(created).await.expect("queued");
    let created = engine.run_once().await.expect("the applier ran");
    assert_eq!(created[0].status, State::Applied, "{created:?}");
    engine
}

fn exporting(payload: Value, target_id: Option<&str>) -> Submission {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    Submission {
        data_capability: "campaign.exportCampaign@1".to_owned(),
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
    }
}

/// The refusal an export meets: its kind and its message.
async fn refused(engine: &Engine, submission: Submission) -> (RefusalKind, String) {
    match engine.submit(submission).await {
        Err(Error::Refused(refusal)) => (refusal.kind, refusal.message),
        other => panic!("refused, not {other:?}"),
    }
}

#[tokio::test]
async fn an_export_stamps_its_time_on_its_campaign_and_answers_its_archive_once_applied() {
    let tables = Tables::answering(valombre());
    let engine = engine(Some(tables.clone())).await;
    let submitted = engine
        .submit(exporting(json!({ "id": VALOMBRE }), None))
        .await
        .expect("queued");
    assert_eq!(submitted.partition, format!("Campaign/{VALOMBRE}"));
    assert_eq!(submitted.warnings, Vec::<String>::new());
    assert_eq!(
        submitted.result.attachment, None,
        "not before it is applied"
    );
    assert_eq!(
        tables.asked(),
        vec![(ARCHIVE.to_owned(), vec![json!(VALOMBRE)])]
    );

    let applied = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(applied.status, State::Applied, "{applied:?}");
    let archive = applied.attachment.expect("the archive, once applied");
    assert_eq!(
        archive["file"],
        "les-cendres-de-valombre-2026-10-18T09-30-00-123Z.json"
    );
    assert_eq!(archive["content"]["exported_at"], NOW);
    assert_eq!(archive["content"]["origin_campaign_id"], VALOMBRE);
    assert_eq!(
        archive["content"]["players"],
        valombre()[0]["archive"]["players"]
    );
    assert_eq!(
        engine
            .status(&submitted.command_id)
            .await
            .expect("its status")
            .attachment,
        Some(archive)
    );
}

#[tokio::test]
async fn the_campaign_is_its_target_else_its_payload_s_and_one_partition_whatever_its_case() {
    for (payload, target, asked) in [
        (
            json!({}),
            Some(VALOMBRE.to_uppercase()),
            VALOMBRE.to_uppercase(),
        ),
        (json!({ "campaignId": VALOMBRE }), None, VALOMBRE.to_owned()),
        (
            json!({ "id": "c1000000-0000-4000-8000-0000000000ff" }),
            Some(VALOMBRE.to_owned()),
            VALOMBRE.to_owned(),
        ),
    ] {
        let tables = Tables::answering(valombre());
        let engine = engine(Some(tables.clone())).await;
        let submitted = engine
            .submit(exporting(payload, target.as_deref()))
            .await
            .expect("queued");
        assert_eq!(tables.asked()[0].1, vec![json!(asked)]);
        assert_eq!(submitted.partition, format!("Campaign/{VALOMBRE}"));
    }
}

#[tokio::test]
async fn a_campaign_the_tables_do_not_hold_or_none_named_is_refused_404_and_nothing_is_queued() {
    let tables = Tables::answering(json!([]));
    let engine = engine(Some(tables.clone())).await;
    for submission in [
        exporting(
            json!({ "id": "c1000000-0000-4000-8000-0000000000ff" }),
            None,
        ),
        exporting(json!({}), None),
    ] {
        assert_eq!(
            refused(&engine, submission).await,
            (
                RefusalKind::NotFound,
                "cette campagne n'existe pas".to_owned()
            )
        );
    }
    // None named is asked for as no campaign at all.
    assert_eq!(tables.asked()[1].1, vec![json!("")]);
    assert_eq!(
        engine.run_once().await.expect("the applier ran"),
        Vec::new()
    );
}

#[tokio::test]
async fn over_the_engine_s_memory_no_campaign_is_held() {
    let engine = engine(None).await;
    assert_eq!(
        refused(&engine, exporting(json!({ "id": VALOMBRE }), None)).await,
        (
            RefusalKind::NotFound,
            "cette campagne n'existe pas".to_owned()
        )
    );
}

#[tokio::test]
async fn tables_that_cannot_be_read_or_rows_that_make_no_archive_are_refused_422() {
    let engine_failing = engine(Some(Tables::failing())).await;
    let (kind, message) =
        refused(&engine_failing, exporting(json!({ "id": VALOMBRE }), None)).await;
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(
        message.starts_with("campaign not read: ") && message.contains("connection reset"),
        "{message}"
    );

    let mut unreadable = valombre();
    unreadable[0]["archive"]["players"] = Value::Null;
    let engine = engine(Some(Tables::answering(unreadable))).await;
    assert_eq!(
        refused(&engine, exporting(json!({ "id": VALOMBRE }), None)).await,
        (
            RefusalKind::Unprocessable,
            "l'archive n'a pas pu être construite : players illisibles".to_owned()
        )
    );

    // A row with no archive column is no campaign.
    let engine = engine_with(json!([{ "other": {} }])).await;
    assert_eq!(
        refused(&engine, exporting(json!({ "id": VALOMBRE }), None))
            .await
            .0,
        RefusalKind::NotFound
    );
}

async fn engine_with(answer: Value) -> Engine {
    engine(Some(Tables::answering(answer))).await
}

#[test]
fn a_time_is_rfc_3339_in_utc_to_the_millisecond() {
    for (millis, text) in [
        (0, "1970-01-01T00:00:00.000Z"),
        (951_782_400_007, "2000-02-29T00:00:00.007Z"),
        (NOW_MILLIS, NOW),
        (4_102_444_799_999, "2099-12-31T23:59:59.999Z"),
        // A century that is no leap year, a fourth one that is, an era's end.
        (4_107_542_399_000, "2100-02-28T23:59:59.000Z"),
        (4_107_542_400_000, "2100-03-01T00:00:00.000Z"),
        (4_233_686_400_000, "2104-02-29T00:00:00.000Z"),
        (13_569_465_599_000, "2399-12-31T23:59:59.000Z"),
        (13_574_606_400_000, "2400-02-29T12:00:00.000Z"),
        (13_574_649_600_000, "2400-03-01T00:00:00.000Z"),
    ] {
        assert_eq!(timestamp(millis), text);
    }
}
