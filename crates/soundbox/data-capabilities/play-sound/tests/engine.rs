//! soundbox.playSound through the engine, over the in-memory store, as the
//! server plants its unit and its target aggregate's judge: a trigger adds
//! a playback on a partition of its own, beside the ones playing
//! (`simultaneous-playback`), its origin the DM view's surface; the player
//! side's is rejected (`dm-side-only`); a sound or a session there is not
//! is refused before anything is queued. With no tables, the session is
//! the one the payload names, as the engine holds it.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::postgres::SystemClock;
use harness_dataguard::store::RandomIds;
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// « Bruit de foule » and « Porte qui grince », Valombre's sounds.
const FOULE: &str = "50000000-0000-4000-8000-000000000001";
const PORTE: &str = "50000000-0000-4000-8000-000000000002";
/// A sound no one added.
const NULLE_PART: &str = "50000000-0000-4000-8000-0000000000ff";
/// Valombre's session 4, and Brume's.
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
const PHARE: &str = "51000000-0000-4000-8000-000000000009";
/// A session no one planned.
const AUCUNE: &str = "51000000-0000-4000-8000-0000000000ff";

/// The SoundPlayback aggregate's document, beside this unit's folder.
fn aggregate() -> Aggregate {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../aggregates/SoundPlayback/aggregate.json");
    Aggregate::parse(&std::fs::read_to_string(path).expect("SoundPlayback's aggregate.json"))
        .expect("contract I")
}

/// Valombre's two sounds and its session, Brume's session, as the engine
/// holds them, under the Sound and Session aggregates' fields.
fn valombre() -> Arc<MemoryStore> {
    let sound = || BTreeMap::from([("campaignId".to_owned(), json!(VALOMBRE))]);
    let session =
        |campaign: &str| BTreeMap::from([("sessionCampaignId".to_owned(), json!(campaign))]);
    Arc::new(
        MemoryStore::default()
            .with_row(&format!("Sound/{FOULE}"), sound())
            .with_row(&format!("Sound/{PORTE}"), sound())
            .with_row(&format!("Session/{SEANCE}"), session(VALOMBRE))
            .with_row(&format!("Session/{PHARE}"), session(BRUME)),
    )
}

/// The engine as the server plants this unit and its target aggregate over
/// `store`.
async fn planted(store: Arc<MemoryStore>) -> Engine {
    let engine = Engine::new(
        store,
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_ids(Arc::new(RandomIds))
    .with_aggregate(aggregate())
    .with_data_capability(
        DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
    )
    .with_plugins(
        [aggregate_sound_playback::plugin(), play_sound::plugin()]
            .into_iter()
            .collect(),
    );
    engine.load().await.expect("the engine loads");
    engine
}

/// A trigger of `sound` with `payload`, by `by`, as the LiveSoundbox sends
/// it.
async fn play(
    engine: &Engine,
    sound: &str,
    payload: Value,
    by: &str,
    key: Option<&str>,
) -> Result<Submitted> {
    engine
        .submit(
            serde_json::from_value::<Submission>(json!({
                "dataCapability": "soundbox.playSound@1",
                "targetId": sound,
                "payload": payload,
                "idempotencyKey": key,
                "caller": { "by": by, "from": "soundbox.LiveSoundbox", "roles": ["role:approver"] },
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

/// A DM's trigger of `sound` with `payload`, applied: its playback's row.
async fn played(
    engine: &Engine,
    store: &MemoryStore,
    sound: &str,
    payload: Value,
) -> BTreeMap<String, Value> {
    let submitted = play(engine, sound, payload, "dm", None)
        .await
        .expect("queued");
    let applied = settled(engine).await;
    assert_eq!(applied.status, State::Applied, "{applied:?}");
    assert!(applied.violations.is_empty(), "{applied:?}");
    store.row_now(&submitted.partition)
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

#[tokio::test]
async fn a_sound_from_the_pastille_plays_beside_the_one_playing_simultaneous_playback() {
    let store = valombre();
    let engine = planted(store.clone()).await;
    let foule = played(&engine, &store, FOULE, json!({ "session": SEANCE })).await;
    let porte = played(
        &engine,
        &store,
        PORTE,
        json!({ "source": "suggestion", "session": SEANCE }),
    )
    .await;

    assert_eq!(foule["origin"], json!("soundbox"), "{foule:?}");
    assert_eq!(porte["origin"], json!("pastille"), "{porte:?}");
    assert_ne!(foule["id"], porte["id"], "two playbacks, two rows");
    let foule_now = store.row_now(&format!("SoundPlayback/{}", foule["id"].as_str().unwrap()));
    for row in [&foule_now, &porte] {
        assert_eq!(row["sessionId"], json!(SEANCE), "{row:?}");
        assert!(row.get("stoppedAt").is_none_or(Value::is_null), "{row:?}");
    }
    assert_eq!(foule_now, foule, "the crowd still plays, untouched");
    assert_eq!(foule["soundId"], json!(FOULE));
    assert_eq!(porte["soundId"], json!(PORTE));
}

#[tokio::test]
async fn the_same_sound_triggered_twice_plays_twice() {
    let store = valombre();
    let engine = planted(store.clone()).await;
    let first = played(&engine, &store, FOULE, json!({ "session": SEANCE })).await;
    let second = played(&engine, &store, FOULE, json!({ "session": SEANCE })).await;
    assert_ne!(first["id"], second["id"]);
}

#[tokio::test]
async fn a_playback_is_written_whole_and_dated_now() {
    let store = valombre();
    let engine = planted(store.clone()).await;
    let row = played(
        &engine,
        &store,
        &FOULE.to_uppercase(),
        json!({ "session": SEANCE.to_uppercase(), "source": null }),
    )
    .await;
    let mut fields: Vec<&str> = row.keys().map(String::as_str).collect();
    fields.retain(|field| *field != "stoppedAt");
    assert_eq!(
        fields,
        ["id", "origin", "sessionId", "soundId", "startedAt"],
        "{row:?}"
    );
    assert_eq!(
        row["soundId"],
        json!(FOULE),
        "lowercase, as the tables write it"
    );
    assert_eq!(row["sessionId"], json!(SEANCE));
    assert_eq!(row["origin"], json!("soundbox"));
    let started = row["startedAt"].as_str().expect("an instant");
    let today = play_sound::stamp(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after 1970")
            .as_millis()
            .try_into()
            .expect("a u64"),
    );
    assert_eq!(started[..10], today[..10], "{started}");
}

#[tokio::test]
async fn a_trigger_from_the_player_side_is_rejected_by_dm_side_only() {
    for (by, payload, why) in [
        ("player", json!({ "session": SEANCE }), "a player's caller"),
        (
            "player",
            json!({ "session": SEANCE, "source": "soundbox" }),
            "a player claiming the soundbox",
        ),
        (
            "dm",
            json!({ "session": SEANCE, "source": "player" }),
            "the player view's source",
        ),
    ] {
        let store = valombre();
        let engine = planted(store.clone()).await;
        let answer = play(&engine, FOULE, payload, by, None).await;
        let result = match answer {
            // Judged at the apply…
            Ok(submitted) if submitted.result.status == State::Queued => settled(&engine).await,
            // …or at the enqueue already.
            Ok(submitted) => submitted.result,
            Err(e) => panic!("{why}: not refused, rejected: {e:?}"),
        };
        assert_eq!(result.status, State::Rejected, "{why}: {result:?}");
        assert_eq!(
            result.violations,
            ["dm-side-only".to_owned()],
            "{why}: {result:?}"
        );
        assert!(nothing_queued(&engine).await, "{why}");
    }
}

#[tokio::test]
async fn a_sound_or_a_session_there_is_not_is_refused_and_nothing_is_queued() {
    let store = valombre();
    let engine = planted(store.clone()).await;
    for (sound, payload, kind, message) in [
        (
            NULLE_PART,
            json!({ "session": SEANCE }),
            RefusalKind::NotFound,
            "ce son n'existe pas",
        ),
        (
            FOULE,
            json!({ "session": AUCUNE }),
            RefusalKind::NotFound,
            "cette séance n'existe pas",
        ),
        (
            FOULE,
            json!({ "session": PHARE }),
            RefusalKind::Unprocessable,
            "cette séance n'est pas de la campagne du son",
        ),
        // No tables to find the campaign's session in.
        (
            FOULE,
            json!({}),
            RefusalKind::Unprocessable,
            "la campagne de ce son n'a aucune séance",
        ),
        (
            "snd-foule",
            json!({ "session": SEANCE }),
            RefusalKind::Unprocessable,
            "sound n'est pas un identifiant",
        ),
        (
            FOULE,
            json!({ "session": 4 }),
            RefusalKind::Unprocessable,
            "session n'est pas un identifiant",
        ),
    ] {
        let error = refused(play(&engine, sound, payload, "dm", None).await, kind);
        assert_eq!(error, message);
    }
    assert!(nothing_queued(&engine).await);
}

#[tokio::test]
async fn a_trigger_replayed_with_its_idempotency_key_plays_once() {
    let store = valombre();
    let engine = planted(store.clone()).await;
    let payload = json!({ "session": SEANCE });
    let first = play(&engine, FOULE, payload.clone(), "dm", Some("play-1"))
        .await
        .expect("queued");
    assert_eq!(settled(&engine).await.status, State::Applied);
    let again = play(&engine, FOULE, payload, "dm", Some("play-1"))
        .await
        .expect("replayed");
    assert!(again.replayed, "{again:?}");
    assert_eq!(again.command_id, first.command_id);
    assert_eq!(again.partition, first.partition);
    assert!(nothing_queued(&engine).await, "the replay queued nothing");
}
