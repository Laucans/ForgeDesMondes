//! media.saveGridSettings through the engine over its memory, as the server
//! plants its unit and its target aggregate: a save is made the six fields
//! it writes on the battlemap's partition — named in whatever case — judged
//! by the DataGuard and applied; one it cannot make is refused before it is
//! queued. The tables it reaches are `postgres.rs`'s.

use std::collections::BTreeMap;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::postgres::SystemClock;
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};

const CARTE: &str = "d1000000-0000-4000-8000-00000000000a";

/// The tavern's battlemap as the engine holds it, with its default grid.
fn carte() -> BTreeMap<String, Value> {
    json!({
        "id": CARTE, "campaignId": "c1000000-0000-4000-8000-000000000001",
        "kind": "battlemap", "title": "La Taverne", "mediaId": CARTE,
        "hexSizePx": 32, "offsetX": 0, "offsetY": 0, "orientation": "pointe",
        "color": "text", "opacity": 0.45,
    })
    .as_object()
    .expect("an object")
    .clone()
    .into_iter()
    .collect()
}

/// The engine as the server plants this unit and its target aggregate,
/// holding the tavern's battlemap, and the store under it.
async fn served() -> (Engine, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::default().with_row(&format!("Media/{CARTE}"), carte()));
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(aggregate_media::DOCUMENT).expect("contract I"))
    .with_data_capability(
        DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
    )
    .with_plugins(
        [aggregate_media::plugin(), save_grid_settings::plugin()]
            .into_iter()
            .collect(),
    );
    engine.load().await.expect("the engine loads");
    (engine, store)
}

/// A save of `payload`, on `target` when it names one, as the GridSettings
/// panel sends it.
async fn save(engine: &Engine, target: Option<&str>, payload: Value) -> Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "media.saveGridSettings@1",
        "targetId": target,
        "payload": payload,
        "caller": { "by": "dm", "from": "GridSettings", "roles": [] },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

/// The save of the acceptance criteria.
fn adjusted() -> Value {
    json!({ "size": 48, "ox": 30, "oy": 25, "orient": "flat", "color": "#c8a24b", "opacity": 0.2 })
}

#[tokio::test]
async fn a_save_writes_the_six_settings_on_the_battlemap_named_in_whatever_case() {
    let (engine, store) = served().await;
    let submitted = save(&engine, Some(&CARTE.to_uppercase()), adjusted())
        .await
        .expect("queued");
    assert_eq!(
        submitted.partition,
        format!("Media/{CARTE}"),
        "{submitted:?}"
    );
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    let mut expected = carte();
    expected.extend(
        json!({
            "hexSizePx": 48, "offsetX": 30, "offsetY": 25, "orientation": "plat",
            "color": "#c8a24b", "opacity": 0.2,
        })
        .as_object()
        .expect("an object")
        .clone(),
    );
    assert_eq!(store.row_now(&format!("Media/{CARTE}")), expected);
}

#[tokio::test]
async fn a_save_it_cannot_make_is_refused_unprocessable_and_not_queued() {
    let (engine, _) = served().await;
    let mut payload = adjusted();
    payload["feetPerHex"] = json!(10);
    for (target, payload, why) in [
        (
            Some(CARTE),
            payload,
            "feetPerHex : seuls size, ox, oy, orient, color et opacity se règlent",
        ),
        (None, adjusted(), "aucune battlemap désignée"),
    ] {
        match save(&engine, target, payload).await {
            Err(Error::Refused(refusal)) => {
                assert_eq!(refusal.kind, RefusalKind::Unprocessable, "{why}");
                assert_eq!(refusal.message, why);
            }
            other => panic!("{why}: {other:?}"),
        }
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
}
