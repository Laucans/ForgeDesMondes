//! media.uploadMedia through the engine, as the server plants its unit and
//! its target aggregate: an upload is prepared against its campaign and its
//! place, queued on its new media's partition, judged by the DataGuard and
//! applied — a battlemap with its default grid. Over the engine's memory the
//! place is the engine's row and every campaign is held; over PostgreSQL
//! (`Context::rows`, here a stand-in for the tables) both are read from the
//! tables. The tables it reaches are `postgres.rs`'s.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::reads::Rows;
use harness_dataguard::store::{RandomIds, Store, Write};
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
const PHARE: &str = "e1000000-0000-4000-8000-000000000004";
const NOBODY: &str = "e1000000-0000-4000-8000-0000000000ff";

/// 2026-10-21T09:00:00.123Z, in milliseconds.
const NOW_MILLIS: u64 = 1_792_573_200_123;

/// An element as the engine holds it, under the Entity aggregate's fields.
fn element(campaign: &str, kind: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("type".to_owned(), json!(kind)),
        ("entityCampaignId".to_owned(), json!(campaign)),
    ])
}

/// The engine as the server plants this unit and its target aggregate, over
/// `store`, its clock at [`NOW_MILLIS`], its reads `rows` when given.
async fn planted(store: Arc<MemoryStore>, rows: Option<Arc<dyn Rows>>) -> Engine {
    let mut engine = Engine::new(
        store,
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at_millis(NOW_MILLIS)),
    )
    .with_ids(Arc::new(RandomIds))
    .with_aggregate(Aggregate::parse(aggregate_media::DOCUMENT).expect("contract I"))
    .with_data_capability(
        DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
    )
    .with_plugins(
        [aggregate_media::plugin(), upload_media::plugin()]
            .into_iter()
            .collect(),
    );
    if let Some(rows) = rows {
        engine = engine.with_rows(rows);
    }
    engine.load().await.expect("the engine loads");
    engine
}

/// The engine over its memory, holding Valombre's tavern (a lieu), Morlox (an
/// NPC) and La Brume's lighthouse (a lieu), and the store under it.
async fn served() -> (Engine, Arc<MemoryStore>) {
    let store = Arc::new(
        MemoryStore::default()
            .with_row(&format!("Entity/{TAVERNE}"), element(VALOMBRE, "lieu"))
            .with_row(&format!("Entity/{MORLOX}"), element(VALOMBRE, "pnj"))
            .with_row(&format!("Entity/{PHARE}"), element(BRUME, "lieu")),
    );
    (planted(store.clone(), None).await, store)
}

/// A submission of media.uploadMedia by the DM, approver of their own
/// campaign's critical fields.
async fn submit(engine: &Engine, payload: Value, key: Option<&str>) -> Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "media.uploadMedia@1",
        "payload": payload,
        "idempotencyKey": key,
        "caller": { "by": "dm", "from": "media.MediaLibrary", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

/// One pass of the single applier, which settles one command.
async fn settled(engine: &Engine) -> CommandResult {
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

/// Uploads `payload`: the media's partition, and what the applier made of it.
async fn upload(engine: &Engine, payload: Value) -> (String, CommandResult) {
    let submitted = submit(engine, payload, None).await.expect("queued");
    assert!(submitted.partition.starts_with("Media/"), "{submitted:?}");
    (submitted.partition, settled(engine).await)
}

/// Whether the queue holds nothing to apply.
async fn nothing_queued(engine: &Engine) -> bool {
    engine.run_once().await.expect("the applier ran").is_empty()
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// The refusal `answer` is, of `kind`: its message.
fn refused(answer: Result<Submitted>, kind: RefusalKind) -> String {
    match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a refusal of kind {kind:?}: {other:?}"),
    }
}

fn media(kind: &str, title: &str) -> Value {
    json!({
        "campaign": VALOMBRE,
        "kind": kind,
        "title": title,
        "file": "/Users/dm/cartes/taverne.png",
        "width": 2048,
        "height": 1536,
    })
}

/// The grid's fields, as the aggregate names them.
const GRID: [&str; 7] = [
    "mediaId",
    "hexSizePx",
    "offsetX",
    "offsetY",
    "orientation",
    "color",
    "opacity",
];

#[tokio::test]
async fn a_battlemap_is_applied_with_its_default_grid_stamped_to_the_millisecond() {
    let (engine, store) = served().await;
    let (partition, result) = upload(&engine, media("battlemap", "La Taverne")).await;
    applied(&result);
    let row = store.row_now(&partition);
    let id = partition.strip_prefix("Media/").expect("its id");
    assert_eq!(id.len(), 36, "a fresh uuid: {id}");
    assert_eq!(row["id"], id);
    assert_eq!(row["mediaId"], id);
    assert_eq!(row["kind"], "battlemap");
    assert_eq!(row["campaignId"], VALOMBRE);
    assert_eq!(row["filePath"], "/Users/dm/cartes/taverne.png");
    assert_eq!(
        (row["widthPx"].clone(), row["heightPx"].clone()),
        (json!(2048), json!(1536))
    );
    // grid-values-range: a hex above 0, an opacity from 0 to 1.
    assert!(
        row["hexSizePx"].as_f64().is_some_and(|size| size > 0.0),
        "{row:?}"
    );
    assert!(
        row["opacity"]
            .as_f64()
            .is_some_and(|opacity| (0.0..=1.0).contains(&opacity)),
        "{row:?}"
    );
    assert_eq!(row["placeId"], Value::Null);
    assert_eq!(row["uploadedAt"], "2026-10-21T09:00:00.123Z");
}

#[tokio::test]
async fn an_illustration_is_applied_with_no_grid() {
    let (engine, store) = served().await;
    let (partition, result) = upload(&engine, media("illustration", "Elowen")).await;
    applied(&result);
    let row = store.row_now(&partition);
    assert_eq!(row["kind"], "illustration");
    for field in GRID {
        assert!(
            row.get(field).is_none_or(Value::is_null),
            "{field}: {row:?}"
        );
    }
}

#[tokio::test]
async fn a_lieu_of_the_campaign_is_the_media_s_place_named_in_either_case() {
    let (engine, store) = served().await;
    for place in [TAVERNE.to_owned(), TAVERNE.to_uppercase()] {
        let mut payload = media("battlemap", "La Taverne");
        payload["place"] = json!(place);
        let (partition, result) = upload(&engine, payload).await;
        applied(&result);
        assert_eq!(store.row_now(&partition)["placeId"], place);
    }
}

#[tokio::test]
async fn media_place_is_lieu_refuses_a_non_lieu_another_campaign_s_lieu_or_no_element() {
    let (engine, _) = served().await;
    for place in [MORLOX, PHARE, NOBODY] {
        let mut payload = media("illustration", "Elowen");
        payload["place"] = json!(place);
        let error = refused(
            submit(&engine, payload, None).await,
            RefusalKind::Unprocessable,
        );
        assert!(error.starts_with("media-place-is-lieu"), "{place}: {error}");
        // Not queued: nothing is written.
        assert!(nothing_queued(&engine).await, "{place}");
    }
}

#[tokio::test]
async fn an_upload_the_tables_could_never_hold_is_refused_and_not_queued() {
    let (engine, _) = served().await;
    let error = refused(
        submit(&engine, media("carte", "La Taverne"), None).await,
        RefusalKind::Unprocessable,
    );
    assert_eq!(error, "un média est une battlemap ou une illustration");
    assert!(nothing_queued(&engine).await);
}

#[tokio::test]
async fn the_same_upload_twice_under_one_idempotency_key_is_one_media() {
    let (engine, _) = served().await;
    let payload = media("battlemap", "La Taverne");
    let first = submit(&engine, payload.clone(), Some("upload-1"))
        .await
        .expect("queued");
    let second = submit(&engine, payload, Some("upload-1"))
        .await
        .expect("replayed");
    assert!(second.replayed, "{second:?}");
    assert_eq!(second.command_id, first.command_id);
    assert_eq!(second.partition, first.partition);
    applied(&settled(&engine).await);
    assert!(nothing_queued(&engine).await);
}

#[tokio::test]
async fn two_uploads_are_two_media() {
    let (engine, _) = served().await;
    let (one, _) = upload(&engine, media("battlemap", "La Taverne")).await;
    let (other, _) = upload(&engine, media("battlemap", "La Taverne")).await;
    assert_ne!(one, other);
}

#[tokio::test]
async fn a_replay_is_answered_as_the_upload_was_queued_though_its_place_was_retyped_since() {
    let (engine, store) = served().await;
    let mut payload = media("battlemap", "La Taverne");
    payload["place"] = json!(TAVERNE);
    let first = submit(&engine, payload.clone(), Some("upload-1"))
        .await
        .expect("queued");
    applied(&settled(&engine).await);
    // The tavern is an NPC now: a new upload naming it is refused.
    store
        .apply(
            &Write {
                partition: format!("Entity/{TAVERNE}"),
                values: element(VALOMBRE, "pnj"),
                tombstone: false,
            },
            &CommandResult {
                command_id: "retyped".to_owned(),
                status: State::Applied,
                data_version: None,
                violations: Vec::new(),
                review_id: None,
                attachment: None,
            },
        )
        .await
        .expect("the tavern retyped");
    let mut fresh = media("battlemap", "La Taverne");
    fresh["place"] = json!(TAVERNE);
    refused(
        submit(&engine, fresh, None).await,
        RefusalKind::Unprocessable,
    );

    let second = submit(&engine, payload, Some("upload-1"))
        .await
        .expect("replayed");
    assert!(second.replayed, "{second:?}");
    assert_eq!(second.command_id, first.command_id);
    assert!(nothing_queued(&engine).await);
}

/// The tables as the Prepare reads them over PostgreSQL: the campaigns they
/// hold and their elements, by id — or no answer at all.
struct Tables {
    campaigns: Vec<&'static str>,
    elements: BTreeMap<&'static str, (&'static str, &'static str)>,
    down: bool,
}

#[async_trait]
impl Rows for Tables {
    async fn rows(&self, sql: &str, parameters: &[Value]) -> Result<Value> {
        if self.down {
            return Err(Error::Store("the tables are down".to_owned()));
        }
        let id = parameters
            .first()
            .and_then(Value::as_str)
            .expect("one id")
            .to_ascii_lowercase();
        let found: Vec<Value> = if sql.contains("FROM campaign") {
            self.campaigns
                .iter()
                .filter(|campaign| **campaign == id)
                .map(|campaign| json!({ "id": campaign }))
                .collect()
        } else {
            assert!(sql.contains("FROM entity"), "{sql}");
            self.elements
                .get(id.as_str())
                .map(|(kind, campaign)| json!({ "type": kind, "entityCampaignId": campaign }))
                .into_iter()
                .collect()
        };
        Ok(Value::Array(found))
    }
}

/// The engine over tables holding Valombre with its tavern and Morlox, and La
/// Brume with its lighthouse — elements the engine never wrote.
async fn over_tables(down: bool) -> (Engine, Arc<MemoryStore>) {
    let tables = Tables {
        campaigns: vec![VALOMBRE, BRUME],
        elements: BTreeMap::from([
            (TAVERNE, ("lieu", VALOMBRE)),
            (MORLOX, ("pnj", VALOMBRE)),
            (PHARE, ("lieu", BRUME)),
        ]),
        down,
    };
    let store = Arc::new(MemoryStore::default());
    (planted(store.clone(), Some(Arc::new(tables))).await, store)
}

#[tokio::test]
async fn over_the_tables_a_lieu_they_hold_is_the_place_and_any_other_is_refused() {
    let (engine, store) = over_tables(false).await;
    let mut payload = media("battlemap", "La Taverne");
    payload["place"] = json!(TAVERNE.to_uppercase());
    let (partition, result) = upload(&engine, payload).await;
    applied(&result);
    assert_eq!(store.row_now(&partition)["placeId"], TAVERNE.to_uppercase());
    for place in [MORLOX, PHARE, NOBODY] {
        let mut payload = media("illustration", place);
        payload["place"] = json!(place);
        let error = refused(
            submit(&engine, payload, None).await,
            RefusalKind::Unprocessable,
        );
        assert!(error.starts_with("media-place-is-lieu"), "{place}: {error}");
    }
    assert!(nothing_queued(&engine).await);
}

#[tokio::test]
async fn over_the_tables_a_campaign_they_do_not_hold_is_refused_and_not_queued() {
    let (engine, _) = over_tables(false).await;
    let mut payload = media("battlemap", "La Taverne");
    payload["campaign"] = json!("c1000000-0000-4000-8000-0000000000ff");
    let error = refused(
        submit(&engine, payload, None).await,
        RefusalKind::Unprocessable,
    );
    assert_eq!(error, "la campagne du média n'existe pas");
    // A campaign they hold, named in another case, is held.
    let mut payload = media("illustration", "Elowen");
    payload["campaign"] = json!(BRUME.to_uppercase());
    let (_, result) = upload(&engine, payload).await;
    applied(&result);
}

#[tokio::test]
async fn tables_that_do_not_answer_refuse_the_upload_and_queue_nothing() {
    let (engine, _) = over_tables(true).await;
    let error = refused(
        submit(&engine, media("battlemap", "La Taverne"), None).await,
        RefusalKind::Unprocessable,
    );
    assert!(error.contains("the tables are down"), "{error}");
    assert!(nothing_queued(&engine).await);
}

#[test]
fn stamp_is_rfc_3339_in_utc_to_the_millisecond() {
    for (millis, text) in [
        (0, "1970-01-01T00:00:00.000Z"),
        (951_782_400_000, "2000-02-29T00:00:00.000Z"),
        (1_709_251_199_999, "2024-02-29T23:59:59.999Z"),
        (NOW_MILLIS, "2026-10-21T09:00:00.123Z"),
        (4_102_444_799_000, "2099-12-31T23:59:59.000Z"),
        // A century that is no leap year, a fourth one that is, an era's end.
        (4_107_542_399_000, "2100-02-28T23:59:59.000Z"),
        (4_107_542_400_001, "2100-03-01T00:00:00.001Z"),
        (4_233_686_400_000, "2104-02-29T00:00:00.000Z"),
        (13_569_465_599_000, "2399-12-31T23:59:59.000Z"),
        (13_574_606_400_000, "2400-02-29T12:00:00.000Z"),
        (13_574_649_600_000, "2400-03-01T00:00:00.000Z"),
    ] {
        assert_eq!(upload_media::stamp(millis), text, "{millis}");
    }
}
