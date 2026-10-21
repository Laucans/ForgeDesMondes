//! media.setMediaPlace through the engine, as the server plants its unit and
//! its target aggregate: a change of place is judged against the media and
//! the place as the engine holds them — and, over PostgreSQL (here a
//! stand-in for the tables), the place as the tables hold it under the
//! engine's row — `media-place-is-lieu` before it is queued, and writes the
//! media's `placeId` alone. The table it reaches is `postgres.rs`'s.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::postgres::SystemClock;
use harness_dataguard::reads::Rows;
use harness_dataguard::store::{Store, Write};
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const CARTE: &str = "a1000000-0000-4000-8000-000000000001";
const NOWHERE: &str = "a1000000-0000-4000-8000-0000000000ff";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
const PHARE: &str = "e1000000-0000-4000-8000-000000000004";
const MOULIN: &str = "e1000000-0000-4000-8000-000000000005";
const CLAIRIERE: &str = "e1000000-0000-4000-8000-000000000006";

/// A media recorded, test-only: `media.uploadMedia` is a unit of its own.
const ADD_MEDIA: &str = r#"{"dataCapability":"test.addMedia","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Media","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

/// An element saved, test-only: `campaign.saveEntity` is another system's.
const SAVE_ELEMENT: &str = r#"{"dataCapability":"test.saveElement","owner":"dataguard","version":1,"effect":"upsert","target":{"aggregate":"Entity","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

/// An element as the engine holds it, under the Entity aggregate's fields.
fn element(campaign: &str, kind: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("type".to_owned(), json!(kind)),
        ("entityCampaignId".to_owned(), json!(campaign)),
    ])
}

/// The engine as the server plants this unit and its target aggregate over
/// `store`, with the test-only DataCapabilities, its reads `rows` when given.
async fn planted(store: Arc<MemoryStore>, rows: Option<Arc<dyn Rows>>) -> Engine {
    let mut engine = Engine::new(
        store,
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(aggregate_media::DOCUMENT).expect("contract I"))
    .with_plugins(
        [aggregate_media::plugin(), set_media_place::plugin()]
            .into_iter()
            .collect(),
    );
    for manifest in [
        include_str!("../data-capability.json"),
        ADD_MEDIA,
        SAVE_ELEMENT,
    ] {
        engine = engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
    }
    if let Some(rows) = rows {
        engine = engine.with_rows(rows);
    }
    engine.load().await.expect("the engine loads");
    engine
}

/// Valombre's battlemap of the ford, `CARTE`, recorded, beside its elements
/// the engine holds: the Taverne and the Moulin (lieux), Morlox (an NPC),
/// and Brume's lighthouse (a lieu of another campaign).
async fn valombre() -> (Engine, Arc<MemoryStore>) {
    let store = Arc::new(
        MemoryStore::default()
            .with_row(&format!("Entity/{TAVERNE}"), element(VALOMBRE, "lieu"))
            .with_row(&format!("Entity/{MOULIN}"), element(VALOMBRE, "lieu"))
            .with_row(&format!("Entity/{MORLOX}"), element(VALOMBRE, "pnj"))
            .with_row(&format!("Entity/{PHARE}"), element(BRUME, "lieu")),
    );
    let engine = planted(store.clone(), None).await;
    add_carte(&engine).await;
    (engine, store)
}

/// `CARTE` added through the test-only DataCapability, and applied.
async fn add_carte(engine: &Engine) {
    let submitted = submission(
        engine,
        json!({
            "dataCapability": "test.addMedia",
            "payload": {
                "id": CARTE,
                "campaignId": VALOMBRE,
                "kind": "battlemap",
                "title": "Le Gué des Corbeaux",
                "filePath": "media/gue.png",
                "widthPx": 960,
                "heightPx": 640,
                "hexSizePx": 30,
                "offsetX": 12,
                "offsetY": 8,
                "orientation": "pointe",
                "color": "#202020",
                "opacity": 0.4,
            },
            "caller": { "by": "dm", "from": "test", "roles": ["role:approver"] },
        }),
    )
    .await
    .expect("queued");
    assert_eq!(submitted.partition, format!("Media/{CARTE}"));
    assert_eq!(settled(engine).await.status, State::Applied);
}

/// `body`, as the wire carries it, submitted.
async fn submission(engine: &Engine, body: Value) -> Result<Submitted> {
    engine
        .submit(serde_json::from_value::<Submission>(body).expect("a submission"))
        .await
}

/// A submission of media.setMediaPlace with `payload`, on `target` when it
/// names one, as the MediaLibrary card sends it.
async fn place(
    engine: &Engine,
    target: Option<&str>,
    payload: Value,
    key: Option<&str>,
) -> Result<Submitted> {
    submission(
        engine,
        json!({
            "dataCapability": "media.setMediaPlace@1",
            "targetId": target,
            "payload": payload,
            "idempotencyKey": key,
            "caller": { "by": "dm", "from": "media.MediaLibrary", "roles": [] },
        }),
    )
    .await
}

/// One pass of the single applier, which settles one command.
async fn settled(engine: &Engine) -> CommandResult {
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

/// Sets the place of `CARTE` to `to` and applies it: the result.
async fn placed(engine: &Engine, to: Value) -> CommandResult {
    let submitted = place(engine, Some(CARTE), json!({ "place": to }), None)
        .await
        .expect("queued");
    assert_eq!(
        submitted.partition,
        format!("Media/{CARTE}"),
        "{submitted:?}"
    );
    settled(engine).await
}

/// Whether the queue holds nothing to apply.
async fn nothing_queued(engine: &Engine) -> bool {
    engine.run_once().await.expect("the applier ran").is_empty()
}

/// The refusal `answer` is, of `kind`: its message.
fn refused(answer: Result<Submitted>, kind: RefusalKind) -> String {
    match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a refusal of kind {kind:?}: {other:?}"),
    }
}

/// The battlemap's row as the engine holds it.
fn carte(store: &MemoryStore) -> BTreeMap<String, Value> {
    store.row_now(&format!("Media/{CARTE}"))
}

#[tokio::test]
async fn a_lieu_of_the_campaign_becomes_the_media_s_place_and_nothing_else_changes() {
    let (engine, store) = valombre().await;
    let before = carte(&store);
    let applied = placed(&engine, json!(TAVERNE)).await;
    assert_eq!(applied.status, State::Applied, "{applied:?}");
    assert!(applied.violations.is_empty(), "{applied:?}");
    let mut after = carte(&store);
    assert_eq!(after.remove("placeId"), Some(json!(TAVERNE)));
    assert_eq!(after, before, "no other field changed");

    // Changed for another lieu of the campaign, named in another case.
    let applied = self::place(
        &engine,
        Some(&CARTE.to_uppercase()),
        json!({ "place": MOULIN.to_uppercase() }),
        None,
    )
    .await
    .expect("queued");
    assert_eq!(applied.partition, format!("Media/{CARTE}"), "{applied:?}");
    assert_eq!(settled(&engine).await.status, State::Applied);
    assert_eq!(carte(&store)["placeId"], json!(MOULIN));
}

#[tokio::test]
async fn a_null_place_detaches_the_media_which_stays_valid() {
    let (engine, store) = valombre().await;
    placed(&engine, json!(TAVERNE)).await;
    let applied = placed(&engine, Value::Null).await;
    assert_eq!(applied.status, State::Applied, "{applied:?}");
    assert!(applied.violations.is_empty(), "{applied:?}");
    let row = carte(&store);
    assert_eq!(row["placeId"], Value::Null);
    assert_eq!(row["kind"], json!("battlemap"), "{row:?}");
}

#[tokio::test]
async fn the_payload_s_media_names_it_when_the_command_names_no_target() {
    let (engine, store) = valombre().await;
    let submitted = place(
        &engine,
        None,
        json!({ "media": CARTE, "place": TAVERNE }),
        None,
    )
    .await
    .expect("queued");
    assert_eq!(
        submitted.partition,
        format!("Media/{CARTE}"),
        "{submitted:?}"
    );
    assert_eq!(settled(&engine).await.status, State::Applied);
    assert_eq!(carte(&store)["placeId"], json!(TAVERNE));
}

#[tokio::test]
async fn a_place_that_is_no_lieu_of_the_campaign_is_refused_naming_media_place_is_lieu() {
    let (engine, store) = valombre().await;
    placed(&engine, json!(TAVERNE)).await;
    let before = carte(&store);
    for (place, why) in [
        (MORLOX, "an NPC"),
        (PHARE, "a lieu of another campaign"),
        (CLAIRIERE, "no element at all"),
    ] {
        let error = refused(
            self::place(&engine, Some(CARTE), json!({ "place": place }), None).await,
            RefusalKind::Unprocessable,
        );
        assert!(error.starts_with("media-place-is-lieu"), "{why}: {error}");
        assert!(nothing_queued(&engine).await, "{why}: nothing was queued");
        assert_eq!(carte(&store), before, "{why}: nothing changed");
    }
}

#[tokio::test]
async fn a_media_there_is_not_or_a_malformed_request_is_refused_and_not_queued() {
    let (engine, store) = valombre().await;
    for media in [NOWHERE, "bm-gue"] {
        let error = refused(
            place(&engine, Some(media), json!({ "place": TAVERNE }), None).await,
            RefusalKind::NotFound,
        );
        assert_eq!(error, "média introuvable", "{media}");
    }
    for payload in [
        json!({ "place": TAVERNE, "title": "Autre" }),
        json!({}),
        json!({ "place": "la-taverne" }),
    ] {
        refused(
            place(&engine, Some(CARTE), payload, None).await,
            RefusalKind::Unprocessable,
        );
    }
    refused(
        place(&engine, None, json!({ "place": TAVERNE }), None).await,
        RefusalKind::Unprocessable,
    );
    assert!(nothing_queued(&engine).await);
    assert!(!carte(&store).contains_key("placeId"));
}

#[tokio::test]
async fn a_command_replayed_with_its_idempotency_key_is_applied_once() {
    let (engine, store) = valombre().await;
    let payload = json!({ "place": TAVERNE });
    let first = place(&engine, Some(CARTE), payload.clone(), Some("attach-1"))
        .await
        .expect("queued");
    assert_eq!(settled(&engine).await.status, State::Applied);
    let once = carte(&store);
    assert_eq!(once["placeId"], json!(TAVERNE));

    let again = place(&engine, Some(CARTE), payload, Some("attach-1"))
        .await
        .expect("replayed");
    assert!(again.replayed, "{again:?}");
    assert_eq!(again.command_id, first.command_id);
    assert_eq!(again.result.status, State::Applied, "{again:?}");
    assert!(nothing_queued(&engine).await, "the replay queued nothing");
    assert_eq!(carte(&store), once);
}

#[tokio::test]
async fn a_replay_is_answered_as_it_was_queued_though_its_place_is_retyped_since() {
    let (engine, store) = valombre().await;
    let payload = json!({ "place": MOULIN });
    let first = place(&engine, Some(CARTE), payload.clone(), Some("attach-2"))
        .await
        .expect("queued");
    assert_eq!(settled(&engine).await.status, State::Applied);

    // The Moulin is no lieu any more.
    store
        .apply(
            &Write {
                partition: format!("Entity/{MOULIN}"),
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
        .expect("the Moulin retyped");
    refused(
        place(&engine, Some(CARTE), payload.clone(), None).await,
        RefusalKind::Unprocessable,
    );

    let again = place(&engine, Some(CARTE), payload, Some("attach-2"))
        .await
        .expect("replayed");
    assert!(again.replayed, "{again:?}");
    assert_eq!(again.command_id, first.command_id);
}

#[tokio::test]
async fn a_lieu_still_queued_is_a_place_as_the_queue_will_leave_it() {
    let (engine, store) = valombre().await;
    // The Clairière is saved, and not applied: only the queue holds it.
    submission(
        &engine,
        json!({
            "dataCapability": "test.saveElement",
            "payload": { "id": CLAIRIERE, "type": "lieu", "entityCampaignId": VALOMBRE },
            "caller": { "by": "dm", "from": "test", "roles": [] },
        }),
    )
    .await
    .expect("queued");
    place(&engine, Some(CARTE), json!({ "place": CLAIRIERE }), None)
        .await
        .expect("queued: a lieu as the queue will leave it");
    let settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 2, "{settled:?}");
    assert!(
        settled.iter().all(|result| result.status == State::Applied),
        "{settled:?}"
    );
    assert_eq!(carte(&store)["placeId"], json!(CLAIRIERE));
}

/// The tables as the Prepare reads them over PostgreSQL: their elements, by
/// id — or no answer at all.
struct Tables {
    elements: BTreeMap<&'static str, (&'static str, &'static str)>,
    down: bool,
}

#[async_trait]
impl Rows for Tables {
    async fn rows(&self, sql: &str, parameters: &[Value]) -> Result<Value> {
        if self.down {
            return Err(Error::Store("the tables are down".to_owned()));
        }
        assert!(sql.contains("FROM entity"), "{sql}");
        let id = parameters
            .first()
            .and_then(Value::as_str)
            .expect("one id")
            .to_ascii_lowercase();
        Ok(Value::Array(
            self.elements
                .get(id.as_str())
                .map(|(kind, campaign)| json!({ "type": kind, "entityCampaignId": campaign }))
                .into_iter()
                .collect(),
        ))
    }
}

/// `CARTE` recorded over tables that hold the Taverne and Morlox — elements
/// the engine never wrote — and the Moulin, a lieu the engine made an NPC.
async fn over_tables(down: bool) -> (Engine, Arc<MemoryStore>) {
    let tables = Tables {
        elements: BTreeMap::from([
            (TAVERNE, ("lieu", VALOMBRE)),
            (MORLOX, ("pnj", VALOMBRE)),
            (MOULIN, ("lieu", VALOMBRE)),
        ]),
        down,
    };
    let store = Arc::new(
        MemoryStore::default().with_row(&format!("Entity/{MOULIN}"), element(VALOMBRE, "pnj")),
    );
    let engine = planted(store.clone(), Some(Arc::new(tables))).await;
    add_carte(&engine).await;
    (engine, store)
}

#[tokio::test]
async fn over_the_tables_a_lieu_they_hold_is_a_place_and_the_engine_s_row_wins() {
    let (engine, store) = over_tables(false).await;
    let applied = placed(&engine, json!(TAVERNE)).await;
    assert_eq!(applied.status, State::Applied, "{applied:?}");
    assert_eq!(carte(&store)["placeId"], json!(TAVERNE));
    for (place, why) in [
        (MORLOX, "an NPC the tables hold"),
        (MOULIN, "a lieu the engine made an NPC"),
        (CLAIRIERE, "an element neither holds"),
    ] {
        let error = refused(
            self::place(&engine, Some(CARTE), json!({ "place": place }), None).await,
            RefusalKind::Unprocessable,
        );
        assert!(error.starts_with("media-place-is-lieu"), "{why}: {error}");
    }
    assert!(nothing_queued(&engine).await);
}

#[tokio::test]
async fn tables_that_do_not_answer_refuse_the_change_and_queue_nothing() {
    let (engine, store) = over_tables(true).await;
    let error = refused(
        place(&engine, Some(CARTE), json!({ "place": TAVERNE }), None).await,
        RefusalKind::Unprocessable,
    );
    assert!(error.contains("the tables are down"), "{error}");
    // Detaching reads no place.
    placed(&engine, Value::Null).await;
    assert_eq!(carte(&store)["placeId"], Value::Null);
}
