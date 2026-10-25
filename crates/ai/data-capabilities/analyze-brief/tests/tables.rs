//! ai.analyzeBrief over PostgreSQL (`DATABASE_URL`), as the server plants it
//! with the Session aggregate, the Entity aggregate and campaign.saveEntity,
//! the Prepare reading the tables: the brief reaches `oral_brief`, text
//! alone; its scenes `scene`, ranked after the session's last
//! (`scene-order-contiguous`), with the elements and media of the campaign
//! they name (`scene-links-same-campaign`); what the campaign does not hold
//! `entity_to_create`, waiting (`brief-no-auto-creation`); Morlox, whom it
//! speaks of, a version of source `brief` with its diff — and no element is
//! created. [`BriefTables`] alone: a media of another campaign rejects its
//! link, a row written again or a tombstone changes nothing. Each test works
//! in a scratch database of its own ([`ScratchDb`]), migrated as the server
//! migrates it, dropped when it ends — even on a panic.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{FollowUp, Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, RefusalKind, Shared};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::store::RandomIds;
use harness_dataguard::tokio_postgres::Client;
use platform_scratch_db::ScratchDb;
use ports_llm::SharedLlm;
use ports_llm::fake::FakeLlm;
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};

/// A row of the Session aggregate written whole, test-only.
const ADD: &str = r#"{"dataCapability":"test.addRow","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Session"},"mode":"overwrite","callableBy":["*"]}"#;
/// A row deleted: the engine's tombstone.
const DELETE: &str = r#"{"dataCapability":"test.deleteRow","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Session"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's session 4, which holds two scenes already.
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
/// Valombre's illustration « Comptoir », and Brume's « Phare ».
const COMPTOIR: &str = "a1000000-0000-4000-8000-000000000002";
const PHARE: &str = "a1000000-0000-4000-8000-000000000005";

/// The brief: the tavern by its alias, Morlox and the counter the campaign
/// holds; the bridge, the ambush and Vesna it does not.
const BRIEF: &str = "On commence à la Taverne, où Morlox attend au Comptoir. \
Deuxième scène, le Pont des Pendus, où a lieu l'embuscade. Vesna, qu’on appelle la Balafrée, surgit.";

/// A database of its own, migrated as the server migrates it, holding — as
/// the fixtures write them — Valombre with the tavern (alias « la
/// taverne ») and the counter's illustration, Brume with its lighthouse's,
/// and Valombre's session 4 with its two scenes.
struct Scratch {
    db: ScratchDb,
    client: Client,
    notifier: Arc<MemoryNotifier>,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let db = ScratchDb::create(&format!("analyze_brief_{name}"))
            .await
            .expect("DATABASE_URL names the harness's PostgreSQL");
        let mut client = db.connect().await.expect("PostgreSQL");
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
        data::migrate(&mut client, &migrations)
            .await
            .expect("migrated");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                     ('{BRUME}', 'La Brume', 'oneshot');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                     ('{TAVERNE}', '{VALOMBRE}', 'lieu', 'La Taverne du Sanglier');
                 INSERT INTO entity_alias (entity_id, alias) VALUES ('{TAVERNE}', 'la taverne');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                     VALUES ('{TAVERNE}', 1, 'dm', '{{}}', '[]');
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                     VALUES ('{SEANCE}', '{VALOMBRE}', 4, 'La séance 4', 'prep', false);
                 INSERT INTO scene (session_id, position, title) VALUES
                     ('{SEANCE}', 1, 'La route'), ('{SEANCE}', 2, 'Le bourg');
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES
                     ('{COMPTOIR}', '{VALOMBRE}', 'illustration', 'Comptoir', 'media/comptoir.png', 800, 600),
                     ('{PHARE}', '{BRUME}', 'illustration', 'Phare', 'media/phare.png', 800, 600);"
            ))
            .await
            .expect("Valombre, Brume, session 4 and the media");
        Scratch {
            db,
            client,
            notifier: Arc::default(),
        }
    }

    /// The engine as the server plants these units over this database: the
    /// projections in the apply transaction, the Prepare reading the tables.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins {
            [
                aggregate_session::plugin(),
                aggregate_entity::plugin(),
                save_entity::plugin(),
                analyze_brief::plugin(),
            ]
            .into_iter()
            .collect()
        };
        let store = PgStore::open(self.db.connect().await.expect("PostgreSQL"))
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        let mut shared = Shared::default();
        shared.insert(Arc::new(SharedLlm::new(FakeLlm::new())));
        shared.insert(Arc::new(SharedSpeechToText::new(FakeSpeechToText::new(""))));
        let mut engine = Engine::new(
            Arc::new(store),
            self.notifier.clone(),
            Arc::new(SystemClock),
        )
        .with_rows(Arc::new(ReadOnly::new(
            self.db.connect().await.expect("PostgreSQL"),
        )))
        .with_ids(Arc::new(RandomIds))
        .with_aggregate(Aggregate::parse(aggregate_session::DOCUMENT).expect("contract I"))
        .with_aggregate(Aggregate::parse(aggregate_entity::MANIFEST).expect("contract I"))
        .with_plugins(plugins())
        .with_shared(shared);
        for manifest in [analyze_brief::MANIFEST, save_entity::MANIFEST, ADD, DELETE] {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        engine.load().await.expect("the engine loads");
        engine
    }

    /// What `sql` answers, each row as JSON, in order.
    async fn rows(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(&format!("SELECT to_jsonb(t)::text FROM ({sql}) t"), &[])
            .await
            .expect("the rows")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
    }
}

/// Submits `payload` through `data_capability` on `target`, by the DM, who
/// approves.
async fn submit(
    engine: &Engine,
    data_capability: &str,
    payload: Value,
    target: Option<&str>,
) -> Submitted {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": data_capability,
        "payload": payload,
        "targetId": target,
        // The critical fields' review is not what these tests judge.
        "caller": { "by": "dm", "from": "OralBrief", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    let submitted = engine.submit(submission).await.expect("queued");
    for follow_up in &submitted.then {
        assert!(matches!(follow_up, FollowUp::Submitted(_)), "{follow_up:?}");
    }
    submitted
}

/// What the applier made of every command queued.
async fn applied(engine: &Engine) -> Vec<CommandResult> {
    let mut results = Vec::new();
    loop {
        let ran = engine.run_once().await.expect("the applier ran");
        if ran.is_empty() {
            return results;
        }
        results.extend(ran);
    }
}

#[tokio::test]
async fn a_brief_is_kept_its_scenes_follow_the_last_its_unknowns_wait_and_morlox_is_versioned() {
    let scratch = Scratch::new("brief").await;
    let engine = scratch.serve().await;
    let morlox = submit(
        &engine,
        "campaign.saveEntity",
        json!({ "campaign": VALOMBRE, "entity": { "kind": "pnj", "name": "Morlox", "notes": "Doit 40 po." } }),
        None,
    )
    .await
    .partition
    .trim_start_matches("Entity/")
    .to_owned();
    applied(&engine).await;
    let elements = scratch.rows("SELECT id FROM entity").await.len();

    let answer = submit(
        &engine,
        "ai.analyzeBrief",
        json!({ "session": SEANCE, "text": BRIEF, "durationSeconds": 252 }),
        None,
    )
    .await;
    let results = applied(&engine).await;
    assert!(
        results.iter().all(|r| r.status == State::Applied),
        "{results:?}"
    );
    assert_eq!(results.len(), 1 + answer.then.len());
    let brief = answer.partition.trim_start_matches("Session/");

    // The brief, text alone.
    assert_eq!(
        scratch
            .rows("SELECT id::text, session_id::text, transcript, duration_seconds, analyzed_at = recorded_at AS analysed FROM oral_brief")
            .await,
        [json!({ "id": brief, "session_id": SEANCE, "transcript": BRIEF, "duration_seconds": 252, "analysed": true })]
    );
    assert_eq!(
        scratch
            .rows("SELECT column_name FROM information_schema.columns WHERE table_name = 'oral_brief' AND data_type = 'bytea'")
            .await,
        Vec::<Value>::new(),
        "no column holds audio"
    );

    // The scenes after the session's last, ranks 1 to n.
    let scenes = scratch
        .rows(&format!(
            "SELECT id::text, position, title FROM scene WHERE session_id = '{SEANCE}' ORDER BY position"
        ))
        .await;
    let ranked: Vec<(i64, &str)> = scenes
        .iter()
        .map(|s| {
            (
                s["position"].as_i64().unwrap(),
                s["title"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        ranked,
        [
            (1, "La route"),
            (2, "Le bourg"),
            (3, "On commence à la Taverne"),
            (4, "Le Pont des Pendus"),
        ]
    );
    let first = scenes[2]["id"].as_str().unwrap();
    let second = scenes[3]["id"].as_str().unwrap();
    assert_eq!(
        scratch
            .rows(&format!(
                "SELECT entity_id::text, position FROM scene_entity WHERE scene_id = '{first}' ORDER BY position"
            ))
            .await,
        [
            json!({ "entity_id": TAVERNE, "position": 1 }),
            json!({ "entity_id": morlox, "position": 2 }),
        ]
    );
    assert_eq!(
        scratch
            .rows("SELECT scene_id::text, media_id::text FROM scene_media")
            .await,
        [json!({ "scene_id": first, "media_id": COMPTOIR })]
    );

    // The unknowns wait, typed, with their excerpt; nothing is created.
    let entries = scratch
        .rows("SELECT type, name, excerpt, status, campaign_id::text, session_id::text, brief_id::text, scene_id::text, created_entity_id FROM entity_to_create ORDER BY id")
        .await;
    let entry = |kind: &str, name: &str, excerpt: &str| {
        json!({
            "type": kind, "name": name, "excerpt": excerpt, "status": "a_creer",
            "campaign_id": VALOMBRE, "session_id": SEANCE, "brief_id": brief,
            "scene_id": second, "created_entity_id": null,
        })
    };
    assert_eq!(
        entries,
        [
            entry(
                "evenement",
                "Où a lieu l'embuscade",
                "Deuxième scène, le Pont des Pendus, où a lieu l'embuscade"
            ),
            entry(
                "lieu",
                "Le Pont des Pendus",
                "Deuxième scène, le Pont des Pendus, où a lieu l'embuscade"
            ),
            entry(
                "pnj",
                "Vesna la Balafrée",
                "Vesna, qu’on appelle la Balafrée, surgit"
            ),
        ]
    );
    assert_eq!(
        scratch.rows("SELECT id FROM entity").await.len(),
        elements,
        "no element is created"
    );

    // Morlox: one version more, the brief's, with its diff.
    assert_eq!(
        scratch
            .rows(&format!(
                "SELECT version_number, source, diff FROM entity_version WHERE entity_id = '{morlox}' ORDER BY version_number"
            ))
            .await
            .last(),
        Some(&json!({
            "version_number": 2,
            "source": "brief",
            "diff": [{
                "field": "notes",
                "old": "Doit 40 po.",
                "new": "Doit 40 po.\nOn commence à la Taverne, où Morlox attend au Comptoir",
            }],
        }))
    );
    assert_eq!(
        scratch
            .rows(&format!("SELECT notes FROM entity WHERE id = '{morlox}'"))
            .await,
        [json!({ "notes": "Doit 40 po.\nOn commence à la Taverne, où Morlox attend au Comptoir" })]
    );
}

#[tokio::test]
async fn an_element_or_a_media_the_engine_tombstoned_is_not_recognised() {
    let scratch = Scratch::new("gone").await;
    let engine = scratch.serve().await;
    // The tavern and the counter deleted: their rows stay, tombstoned.
    scratch
        .client
        .batch_execute(&format!(
            "INSERT INTO dataguard_rows (partition, tombstoned) VALUES
                 ('Entity/{TAVERNE}', true), ('Media/{COMPTOIR}', true)"
        ))
        .await
        .expect("tombstoned");
    let answer = submit(
        &engine,
        "ai.analyzeBrief",
        // In uppercase, the session is the same.
        json!({ "session": SEANCE.to_uppercase(), "text": "On commence à la Taverne, au Comptoir." }),
        None,
    )
    .await;
    let results = applied(&engine).await;
    assert!(
        results.iter().all(|r| r.status == State::Applied),
        "{results:?}"
    );
    assert_eq!(answer.then.len(), 3, "a scene and two entries");
    assert!(scratch.rows("SELECT * FROM scene_entity").await.is_empty());
    assert!(scratch.rows("SELECT * FROM scene_media").await.is_empty());
    assert_eq!(
        scratch
            .rows("SELECT type, name FROM entity_to_create ORDER BY name")
            .await,
        [
            json!({ "type": "pnj", "name": "Comptoir" }),
            json!({ "type": "lieu", "name": "La Taverne" }),
        ]
    );
    assert_eq!(
        scratch
            .rows(&format!(
                "SELECT id FROM scene WHERE session_id = '{SEANCE}'"
            ))
            .await
            .len(),
        3
    );
}

#[tokio::test]
async fn a_media_of_another_campaign_is_never_linked_and_a_row_is_written_once() {
    let scratch = Scratch::new("media").await;
    let engine = scratch.serve().await;
    let scene = "5c000000-0000-4000-8000-00000000000a";
    let link = "5c000000-0000-4000-8000-00000000000b";
    let stranger = "5c000000-0000-4000-8000-00000000000c";
    let entry = "5c000000-0000-4000-8000-00000000000d";
    submit(
        &engine,
        "test.addRow",
        json!({ "sceneId": scene, "sceneSessionId": SEANCE, "sceneTitle": "Le gué", "entityId": [] }),
        Some(scene),
    )
    .await;
    submit(
        &engine,
        "test.addRow",
        json!({ "sceneMediaSceneId": scene, "sceneMediaMediaId": COMPTOIR }),
        Some(link),
    )
    .await;
    submit(
        &engine,
        "test.addRow",
        json!({ "sceneMediaSceneId": scene, "sceneMediaMediaId": PHARE }),
        Some(stranger),
    )
    .await;
    submit(
        &engine,
        "test.addRow",
        json!({
            "entityToCreateId": entry, "entityToCreateCampaignId": VALOMBRE,
            "entityToCreateSessionId": SEANCE, "entityToCreateSceneId": scene,
            "type": "lieu", "name": "Le gué", "excerpt": "au gué",
            "entityToCreateStatus": "a_creer", "createdAt": "2026-10-24T09:00:00.000Z",
        }),
        Some(entry),
    )
    .await;
    let results = applied(&engine).await;
    let states: Vec<State> = results.iter().map(|r| r.status).collect();
    assert_eq!(
        states,
        [
            State::Applied,
            State::Applied,
            State::Rejected,
            State::Applied
        ]
    );
    let reason = scratch
        .notifier
        .sent()
        .into_iter()
        .find_map(|message| message.reason)
        .unwrap_or_default();
    assert_eq!(
        reason,
        "scene-links-same-campaign : une scène ne lie que des médias de la campagne de sa séance"
    );
    assert_eq!(
        scratch.rows("SELECT media_id::text FROM scene_media").await,
        [json!({ "media_id": COMPTOIR })]
    );

    // Written again — the whole row — or deleted, the tables keep it as it
    // was first written.
    submit(
        &engine,
        "test.addRow",
        json!({ "name": "Le gué des Corbeaux", "sceneMediaSceneId": scene, "sceneMediaMediaId": COMPTOIR }),
        Some(entry),
    )
    .await;
    submit(
        &engine,
        "test.addRow",
        json!({ "excerpt": "là" }),
        Some(link),
    )
    .await;
    submit(&engine, "test.deleteRow", json!({}), Some(link)).await;
    let results = applied(&engine).await;
    assert!(
        results.iter().all(|r| r.status == State::Applied),
        "{results:?}"
    );
    assert_eq!(
        scratch
            .rows("SELECT name, excerpt, brief_id FROM entity_to_create")
            .await,
        [json!({ "name": "Le gué", "excerpt": "au gué", "brief_id": null })]
    );
    assert_eq!(
        scratch.rows("SELECT scene_id FROM scene_media").await.len(),
        1
    );
}

#[tokio::test]
async fn a_brief_of_a_session_the_tables_do_not_hold_is_rejected_whole() {
    let scratch = Scratch::new("nowhere").await;
    let engine = scratch.serve().await;
    let brief = "5c000000-0000-4000-8000-00000000000e";
    submit(
        &engine,
        "test.addRow",
        json!({
            "oralBriefId": brief, "oralBriefSessionId": "51000000-0000-4000-8000-0000000000ff",
            "transcript": "Rien.", "durationSeconds": 0,
            "recordedAt": "2026-10-24T09:00:00.000Z", "analyzedAt": null,
        }),
        Some(brief),
    )
    .await;
    let results = applied(&engine).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, State::Rejected, "{results:?}");
    assert!(scratch.rows("SELECT id FROM oral_brief").await.is_empty());
}

#[tokio::test]
async fn a_table_that_fails_fails_the_step_and_rejects_nothing() {
    let scratch = Scratch::new("fails").await;
    let engine = scratch.serve().await;
    let entry = "5c000000-0000-4000-8000-00000000000f";
    submit(
        &engine,
        "test.addRow",
        json!({
            "entityToCreateId": entry, "entityToCreateCampaignId": VALOMBRE,
            "type": "lieu", "name": "Le gué", "excerpt": "au gué",
            "entityToCreateStatus": "a_creer", "createdAt": "2026-10-24T09:00:00.000Z",
        }),
        Some(entry),
    )
    .await;
    scratch
        .client
        .batch_execute("DROP TABLE entity_to_create")
        .await
        .expect("the table gone");
    match engine.run_once().await {
        Err(harness_dataguard::Error::Store(_)) => {}
        other => panic!("the store's failure, not the command's: {other:?}"),
    }
}

#[tokio::test]
async fn a_table_the_check_cannot_read_fails_the_step_and_rejects_nothing() {
    let scratch = Scratch::new("unreadable").await;
    let engine = scratch.serve().await;
    let link = "5c000000-0000-4000-8000-000000000010";
    submit(
        &engine,
        "test.addRow",
        json!({ "sceneMediaSceneId": SEANCE, "sceneMediaMediaId": COMPTOIR }),
        Some(link),
    )
    .await;
    // The projection's own check reads `media`: an error of class 42, which
    // neither a value nor a constraint caused.
    scratch
        .client
        .batch_execute("DROP TABLE media CASCADE")
        .await
        .expect("the table gone");
    match engine.run_once().await {
        Err(harness_dataguard::Error::Store(why)) => {
            assert!(why.contains("media"), "{why}");
        }
        other => panic!("the store's failure, not the command's rejection: {other:?}"),
    }
}

#[tokio::test]
async fn a_brief_queued_twice_before_the_applier_runs_lists_each_unknown_once() {
    let scratch = Scratch::new("twice").await;
    let engine = scratch.serve().await;
    let brief = json!({ "session": SEANCE, "text": BRIEF });
    let first = submit(&engine, "ai.analyzeBrief", brief.clone(), None).await;
    let second = submit(&engine, "ai.analyzeBrief", brief, None).await;
    assert_ne!(first.partition, second.partition);
    let results = applied(&engine).await;
    assert!(
        results.iter().all(|r| r.status == State::Applied),
        "{results:?}"
    );
    assert_eq!(
        scratch
            .rows("SELECT type, name, count(*) AS times FROM entity_to_create GROUP BY type, name ORDER BY name")
            .await,
        [
            json!({ "type": "lieu", "name": "Le Pont des Pendus", "times": 1 }),
            // Not saved here: an NPC the campaign does not hold.
            json!({ "type": "pnj", "name": "Morlox", "times": 1 }),
            json!({ "type": "evenement", "name": "Où a lieu l'embuscade", "times": 1 }),
            json!({ "type": "pnj", "name": "Vesna la Balafrée", "times": 1 }),
        ]
    );
}

#[tokio::test]
async fn a_brief_of_a_session_still_queued_is_refused_until_it_is_applied() {
    let scratch = Scratch::new("queued").await;
    let engine = scratch.serve().await;
    let session = "51000000-0000-4000-8000-000000000006";
    submit(
        &engine,
        "test.addRow",
        json!({
            "sessionId": session, "sessionCampaignId": VALOMBRE,
            "sessionTitle": "La séance 5", "sessionStatus": "prep", "microphoneOn": false,
        }),
        Some(session),
    )
    .await;
    let analysis: Submission = serde_json::from_value(json!({
        "dataCapability": "ai.analyzeBrief",
        "payload": { "session": session, "text": BRIEF },
        "caller": { "by": "dm", "from": "OralBrief", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    match engine.submit(analysis.clone()).await {
        Err(harness_dataguard::Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::Unprocessable);
            assert!(
                refusal.message.contains("pas encore enregistrée"),
                "{}",
                refusal.message
            );
        }
        other => panic!("refused while the session is queued: {other:?}"),
    }
    assert!(
        scratch
            .rows("SELECT command FROM dataguard_queue WHERE data_capability = 'ai.analyzeBrief'")
            .await
            .is_empty(),
        "nothing queued"
    );

    let results = applied(&engine).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, State::Applied, "{results:?}");
    engine.submit(analysis).await.expect("queued once applied");
    let results = applied(&engine).await;
    assert!(
        results.iter().all(|r| r.status == State::Applied),
        "{results:?}"
    );
    assert_eq!(
        scratch
            .rows(&format!(
                "SELECT session_id::text FROM oral_brief WHERE session_id = '{session}'"
            ))
            .await,
        [json!({ "session_id": session })]
    );
}
