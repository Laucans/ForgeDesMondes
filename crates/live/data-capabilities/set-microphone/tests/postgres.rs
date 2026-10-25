//! `live.setMicrophone` over PostgreSQL (`DATABASE_URL`), as the server plants it beside
//! Session, whose projection writes its tables: each test works in a scratch database of
//! its own, migrated as the server migrates it, dropped when it ends
//! (`platform_test_kit::Scratch`). What the tables hold after the command — the status,
//! the start, the microphone — the session the query finds, the vocabulary it reads
//! (the campaign's alone), and the stream that opens once the command is applied: each
//! phrase heard becomes a line of the session's journal and its suggestions only while
//! the microphone is on, and off ends it.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use harness_dataguard::api::Submission;
use harness_dataguard::command::State;
use harness_dataguard::plugins::{RefusalKind, Shared};
use platform_test_kit::{Bench, Scratch, Served, file, refused, uuid};
use ports_speech_to_text::fake::FakeSpeechToText;
use ports_speech_to_text::{SharedSpeechToText, SpeechToText, Vocabulary as Heard};
use serde_json::{Value, json};

use set_microphone::Listening;
use set_microphone::listening::Submit;

/// The campaign, and another one.
fn campaign() -> String {
    uuid(0xca, 1)
}
fn elsewhere() -> String {
    uuid(0xca, 2)
}

/// The sessions of the campaign, by how far they are: the first closed, the second in
/// prep, the third in prep too (the last one planned).
fn closed() -> String {
    uuid(0x5e, 1)
}
fn prep() -> String {
    uuid(0x5e, 2)
}
fn planned() -> String {
    uuid(0x5e, 3)
}
fn unknown() -> String {
    uuid(0x5e, 9)
}

/// Vesna by her alias, the tavern by its alias, a sound; Gaspard le Rouge and a bell
/// are another campaign's.
fn vesna() -> String {
    uuid(0xe1, 1)
}
fn tavern() -> String {
    uuid(0xe1, 2)
}
fn gaspard() -> String {
    uuid(0xe1, 3)
}
fn door() -> String {
    uuid(0x50, 1)
}

/// Where the submissions go: the journal lines (`session.addJournalLine`)
/// kept apart from the rest.
#[derive(Default)]
struct Recorder {
    kept: Mutex<Vec<Submission>>,
    lines: Mutex<Vec<Submission>>,
}

impl Recorder {
    /// What was submitted but the journal lines.
    fn kept(&self) -> Vec<Submission> {
        self.kept
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The journal lines submitted, in order.
    fn lines(&self) -> Vec<Submission> {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl Submit for Recorder {
    async fn submit(&self, submission: Submission) -> Result<(), String> {
        let kept = if submission.data_capability == "session.addJournalLine" {
            &self.lines
        } else {
            &self.kept
        };
        kept.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(submission);
        Ok(())
    }
}

/// A planted unit over a scratch database, and what listens through it.
struct Rig {
    scratch: Scratch,
    served: Served,
    listening: Arc<Listening>,
    recorder: Arc<Recorder>,
    speech: FakeSpeechToText,
}

/// The tables the foreign keys ask for, the engine planted over them, a microphone that
/// hears `phrases`, one every `every`.
async fn rig(label: &str, phrases: &[&str], every: Duration) -> Rig {
    let scratch = Scratch::new(label).await;
    scratch
        .seed(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES
                 ('{campaign}', 'Les Cendres de Valombre', 'campagne'),
                 ('{elsewhere}', 'Ailleurs', 'campagne');
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on, started_at, closed_at) VALUES
                 ('{closed}', '{campaign}', 1, 'Arrivée', 'closed', false,
                  '2026-09-05T20:00:00Z', '2026-09-05T23:00:00Z'),
                 ('{prep}', '{campaign}', 2, 'Le moulin', 'prep', false, NULL, NULL),
                 ('{planned}', '{campaign}', 3, 'Le gué', 'prep', false, NULL, NULL);
             INSERT INTO sound (id, campaign_id, name, file_path) VALUES
                 ('{door}', '{campaign}', 'Porte qui grince', '/sons/porte.ogg'),
                 ('{bell}', '{elsewhere}', 'Cloche du port', '/sons/cloche.ogg');",
            campaign = campaign(),
            elsewhere = elsewhere(),
            closed = closed(),
            prep = prep(),
            planned = planned(),
            door = door(),
            bell = uuid(0x50, 2),
        ))
        .await;
    // An element's versions are written in the transaction that saves it
    // (every-change-versioned).
    scratch
        .seed(&format!(
            "BEGIN;
             INSERT INTO entity (id, campaign_id, type, name) VALUES
                 ('{vesna}', '{campaign}', 'pnj', 'Vesna la Balafrée'),
                 ('{tavern}', '{campaign}', 'lieu', 'La Taverne du Sanglier'),
                 ('{gaspard}', '{elsewhere}', 'pnj', 'Gaspard le Rouge');
             INSERT INTO entity_alias (entity_id, alias) VALUES
                 ('{vesna}', 'la Balafrée'),
                 ('{tavern}', 'la taverne'),
                 ('{gaspard}', 'le rouge');
             INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) VALUES
                 ('{vesna}', 1, 'dm', '{{}}', '[]'),
                 ('{tavern}', 1, 'dm', '{{}}', '[]'),
                 ('{gaspard}', 1, 'dm', '{{}}', '[]');
             COMMIT;",
            vesna = vesna(),
            tavern = tavern(),
            gaspard = gaspard(),
            campaign = campaign(),
            elsewhere = elsewhere(),
        ))
        .await;
    let speech = FakeSpeechToText::new("").listening(phrases.iter().copied(), every);
    let listening = Arc::new(Listening::new());
    let recorder = Arc::new(Recorder::default());
    listening.submitting(recorder.clone());
    let mut shared = Shared::default();
    shared.insert(Arc::new(SharedSpeechToText::new(speech.clone())));
    shared.insert(Arc::clone(&listening));
    let served = Bench::new(
        include_str!("../data-capability.json"),
        &file("crates/session/aggregates/Session/aggregate.json"),
        [aggregate_session::plugin(), set_microphone::plugin()],
    )
    .sharing(shared)
    .postgres(&scratch)
    .await;
    Rig {
        scratch,
        served,
        listening,
        recorder,
        speech,
    }
}

impl Rig {
    /// The session's columns: status, microphone, and its start in seconds since the
    /// epoch (`null` when it has none).
    async fn session(&self, id: &str) -> Value {
        self.scratch
            .rows(
                "SELECT status, microphone_on,
                        floor(extract(epoch FROM started_at))::int8 AS started_at
                 FROM session WHERE id::text = $1",
                &[json!(id)],
            )
            .await
            .remove(0)
    }

    /// The microphone turned `on` in `session`, applied.
    async fn turn(&self, on: bool, session: Option<&str>) -> Value {
        let applied = self
            .served
            .applied("live.setMicrophone", json!({ "on": on }), session)
            .await;
        assert_eq!(applied.status, State::Applied, "{applied:?}");
        assert!(applied.violations.is_empty(), "{:?}", applied.violations);
        json!(applied.status)
    }

    /// Waits until `done`, for two seconds at most.
    async fn until(&self, mut done: impl FnMut() -> bool) -> bool {
        for _ in 0..200 {
            if done() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        done()
    }
}

#[tokio::test]
async fn a_session_in_prep_goes_live_dated_with_its_microphone_on() {
    let rig = rig("set_microphone_prep", &[], Duration::from_secs(60)).await;
    rig.turn(true, Some(&prep())).await;
    assert_eq!(
        rig.session(&prep()).await,
        json!({ "status": "live", "microphone_on": true, "started_at": 1_792_000_000 })
    );
    // The other sessions are as they were.
    assert_eq!(
        rig.session(&planned()).await,
        json!({ "status": "prep", "microphone_on": false, "started_at": null })
    );
    assert!(rig.listening.listens(&prep()));
    rig.listening.shutdown();
}

#[tokio::test]
async fn text_only_no_audio_the_row_written_holds_scalars_alone() {
    let rig = rig("set_microphone_text_only", &[], Duration::from_secs(60)).await;
    rig.turn(true, Some(&prep())).await;
    let rows = rig
        .scratch
        .rows(
            "SELECT fields FROM dataguard_rows WHERE partition = $1",
            &[json!(format!("Session/{}", prep()))],
        )
        .await;
    let fields = rows[0]["fields"].as_object().expect("the row");
    assert_eq!(fields["microphoneOn"], json!(true));
    assert_eq!(fields["sessionStatus"], json!("live"));
    assert!(fields["startedAt"].is_number());
    assert!(
        fields.values().all(|v| !v.is_object() && !v.is_array()),
        "no field holds anything but a text, a number or a boolean: {fields:?}"
    );
    rig.listening.shutdown();
}

#[tokio::test]
async fn the_start_is_stamped_once() {
    let rig = rig("set_microphone_dated_once", &[], Duration::from_secs(60)).await;
    rig.turn(true, Some(&prep())).await;
    rig.turn(false, Some(&prep())).await;
    assert_eq!(
        rig.session(&prep()).await,
        json!({ "status": "live", "microphone_on": false, "started_at": 1_792_000_000 }),
        "off leaves the session live and dated"
    );
    rig.served.clock.set(1_792_003_600);
    rig.turn(true, Some(&prep())).await;
    assert_eq!(
        rig.session(&prep()).await,
        json!({ "status": "live", "microphone_on": true, "started_at": 1_792_000_000 }),
        "turned on again an hour later, it keeps its start"
    );
    rig.listening.shutdown();
}

#[tokio::test]
async fn a_closed_session_refuses_the_microphone_and_nothing_changes() {
    let rig = rig(
        "set_microphone_closed",
        &["La Balafrée"],
        Duration::from_millis(5),
    )
    .await;
    let before = rig.session(&closed()).await;
    let message = refused(
        rig.served
            .submit("live.setMicrophone", json!({ "on": true }), Some(&closed()))
            .await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("status-lifecycle"), "{message}");
    assert!(rig.served.settled().await.is_empty(), "nothing queued");
    assert_eq!(rig.session(&closed()).await, before);
    assert_eq!(before["status"], json!("closed"));
    assert!(!rig.listening.listens(&closed()));
    assert!(
        rig.speech.journal().list().is_empty(),
        "the microphone was not opened"
    );
}

#[tokio::test]
async fn a_closed_session_turned_off_stays_closed() {
    let rig = rig("set_microphone_closed_off", &[], Duration::from_secs(60)).await;
    rig.turn(false, Some(&closed())).await;
    assert_eq!(
        rig.session(&closed()).await,
        json!({ "status": "closed", "microphone_on": false, "started_at": 1_788_638_400 })
    );
}

#[tokio::test]
async fn without_a_target_the_command_finds_the_campaigns_session() {
    let rig = rig("set_microphone_resolves", &[], Duration::from_secs(60)).await;
    let partition = |submitted: Result<harness_dataguard::api::Submitted, _>| {
        submitted.expect("queued").partition
    };
    let on = || json!({ "on": true });

    // Of the sessions not closed, the last one planned.
    let found = partition(rig.served.submit("live.setMicrophone", on(), None).await);
    assert_eq!(found, format!("Session/{}", planned()));

    // The live one first, whatever was planned after it.
    rig.scratch
        .seed(&format!(
            "UPDATE session SET status = 'live' WHERE id = '{}'",
            prep()
        ))
        .await;
    let found = partition(rig.served.submit("live.setMicrophone", on(), None).await);
    assert_eq!(found, format!("Session/{}", prep()));

    // Of two live ones, the one whose microphone is on: it is the one to turn off.
    rig.scratch
        .seed(&format!(
            "UPDATE session SET status = 'live', microphone_on = true WHERE id = '{}'",
            planned()
        ))
        .await;
    rig.scratch
        .seed(&format!(
            "UPDATE session SET microphone_on = false WHERE id = '{}'",
            prep()
        ))
        .await;
    let found = partition(
        rig.served
            .submit("live.setMicrophone", json!({ "on": false }), None)
            .await,
    );
    assert_eq!(found, format!("Session/{}", planned()));

    // A session the engine deleted is none.
    rig.scratch
        .seed(&format!(
            "INSERT INTO dataguard_rows (partition, tombstoned) VALUES ('Session/{}', true)",
            planned()
        ))
        .await;
    let found = partition(rig.served.submit("live.setMicrophone", on(), None).await);
    assert_eq!(found, format!("Session/{}", prep()));
}

#[tokio::test]
async fn of_two_live_sessions_with_the_microphone_off_the_last_started_is_the_one() {
    let rig = rig("set_microphone_last_started", &[], Duration::from_secs(60)).await;
    rig.scratch
        .seed(&format!(
            "UPDATE session SET status = 'live', started_at = '2026-10-17T20:00:00Z' WHERE id = '{}';
             UPDATE session SET status = 'live', started_at = '2026-10-18T20:00:00Z' WHERE id = '{}'",
            planned(),
            prep()
        ))
        .await;
    let found = rig
        .served
        .submit("live.setMicrophone", json!({ "on": true }), None)
        .await
        .expect("queued")
        .partition;
    assert_eq!(
        found,
        format!("Session/{}", prep()),
        "the one started last, though the other was planned after"
    );
}

#[tokio::test]
async fn with_no_session_not_closed_there_is_none_to_listen_in() {
    let rig = rig("set_microphone_none", &[], Duration::from_secs(60)).await;
    rig.scratch
        .seed("UPDATE session SET status = 'closed'")
        .await;
    let message = refused(
        rig.served
            .submit("live.setMicrophone", json!({ "on": true }), None)
            .await,
        RefusalKind::NotFound,
    );
    assert!(message.contains("no session"), "{message}");
    assert!(rig.served.settled().await.is_empty(), "nothing queued");
}

#[tokio::test]
async fn a_session_the_tables_do_not_hold_is_not_found() {
    let rig = rig("set_microphone_unknown", &[], Duration::from_secs(60)).await;
    let message = refused(
        rig.served
            .submit(
                "live.setMicrophone",
                json!({ "on": true }),
                Some(&unknown()),
            )
            .await,
        RefusalKind::NotFound,
    );
    assert!(message.contains("no session"), "{message}");
    assert!(rig.served.settled().await.is_empty(), "nothing queued");
}

#[tokio::test]
async fn the_transcription_listens_for_the_names_and_aliases_of_the_campaign_alone() {
    let rig = rig("set_microphone_vocabulary", &[], Duration::from_secs(60)).await;
    rig.turn(true, Some(&prep())).await;
    let journal = rig.speech.journal().list();
    assert_eq!(
        journal.len(),
        1,
        "the stream is opened once, when it starts"
    );
    assert_eq!(journal[0].audio_bytes, 0, "no audio is handed to the port");
    assert_eq!(
        journal[0].vocabulary,
        Heard {
            campaign_names: vec!["La Taverne du Sanglier".into(), "Vesna la Balafrée".into(),],
            aliases: vec!["la taverne".into(), "la Balafrée".into()],
            sound_names: vec!["Porte qui grince".into()],
        },
        "« la taverne » is heard for, Gaspard le Rouge and the bell of another campaign are not"
    );
    rig.listening.shutdown();
}

#[tokio::test]
async fn an_element_or_a_sound_the_engine_deleted_is_not_listened_for() {
    let rig = rig("set_microphone_deleted", &[], Duration::from_secs(60)).await;
    rig.scratch
        .seed(&format!(
            "INSERT INTO dataguard_rows (partition, tombstoned) VALUES
                 ('Entity/{}', true), ('Sound/{}', true)",
            vesna(),
            door()
        ))
        .await;
    rig.turn(true, Some(&prep())).await;
    assert_eq!(
        rig.speech.journal().list()[0].vocabulary,
        Heard {
            campaign_names: vec!["La Taverne du Sanglier".into()],
            aliases: vec!["la taverne".into()],
            sound_names: vec![],
        }
    );
    rig.listening.shutdown();
}

#[tokio::test]
async fn a_phrase_names_a_target_of_the_campaign_only_while_the_microphone_is_on() {
    let rig = rig(
        "set_microphone_flow",
        &[
            "Gaspard le Rouge salue la salle.",
            "La Balafrée ? Jamais entendu parler.",
            "Derrière le comptoir, la porte qui grince.",
        ],
        Duration::from_millis(200),
    )
    .await;
    rig.turn(true, None).await;
    assert!(
        rig.until(|| rig.recorder.kept().len() == 2).await,
        "{:?}",
        rig.recorder.kept()
    );
    let sent: Vec<Value> = rig
        .recorder
        .kept()
        .iter()
        .map(|s| json!({ "to": s.data_capability, "payload": s.payload }))
        .collect();
    assert_eq!(
        sent,
        [
            json!({ "to": "live.addSuggestion", "payload": {
                "sessionId": planned(), "entityId": vesna(), "matchedText": "la Balafrée" } }),
            json!({ "to": "live.addSuggestion", "payload": {
                "sessionId": planned(), "soundId": door(), "matchedText": "Porte qui grince" } }),
        ],
        "Gaspard le Rouge, of another campaign, names nothing"
    );
    // Every phrase heard, a target named or not, is a line of the session's journal.
    assert_eq!(
        rig.recorder
            .lines()
            .iter()
            .map(|l| (l.target_id.clone(), json!(l.payload)))
            .collect::<Vec<_>>(),
        [
            "Gaspard le Rouge salue la salle.",
            "La Balafrée ? Jamais entendu parler.",
            "Derrière le comptoir, la porte qui grince.",
        ]
        .map(|text| (Some(planned()), json!({ "text": text })))
    );

    rig.turn(false, None).await;
    assert!(
        !rig.listening.listens(&planned()),
        "the stream's task has ended"
    );
    assert_eq!(rig.session(&planned()).await["microphone_on"], json!(false));
}

#[tokio::test]
async fn nothing_is_heard_once_the_microphone_is_off() {
    let rig = rig(
        "set_microphone_off",
        &["La Balafrée", "La Balafrée", "La Balafrée"],
        Duration::from_millis(150),
    )
    .await;
    rig.turn(true, Some(&prep())).await;
    assert!(rig.until(|| !rig.recorder.kept().is_empty()).await);
    rig.turn(false, Some(&prep())).await;
    assert!(!rig.listening.listens(&prep()));
    let heard = rig.recorder.kept().len();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        rig.recorder.kept().len(),
        heard,
        "no suggestion after the microphone off"
    );
}

#[tokio::test]
async fn a_command_only_queued_opens_no_stream_until_it_is_applied() {
    let rig = rig(
        "set_microphone_unapplied",
        &["La Balafrée"],
        Duration::from_millis(5),
    )
    .await;
    rig.served
        .submit("live.setMicrophone", json!({ "on": true }), Some(&prep()))
        .await
        .expect("queued");
    // Queued, not applied: the stream opens when the command is.
    assert!(!rig.listening.listens(&prep()));
    assert!(rig.speech.journal().list().is_empty());
    let settled = rig.served.settled().await;
    assert_eq!(settled.len(), 1);
    assert!(rig.listening.listens(&prep()), "applied, it listens");
    rig.listening.shutdown();
}
