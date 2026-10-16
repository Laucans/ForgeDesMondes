//! Rule R26: Clôture de séance avec journal et résumé.
mod common;

use common::*;

// given: une séance en cours
// when: le DM clôt la séance
// then: le micro passe en pause, le journal horodaté est conservé localement, et un résumé est généré avec le modèle configuré.
#[test]
fn r26_closing_with_journal_and_summary() {
    rule(
        "R26",
        &[
            "data-capability:live.setMicrophone",
            "data-capability:session.closeSession",
            "capability:session.getLiveSession",
            "capability:session.getSessionJournal",
            "capability:session.getPreviousSessionSummary",
            "capability:ai.getSettings",
            "capability:session.listSessions",
            "data-capability:session.createSession",
        ],
        |s| {
            let session = new_session(s, CAMPAIGN, &["elowen"])?;
            apply(s, "live.setMicrophone", json!({ "on": true }), None)?;

            // The stand-in transcription speaks: wait for the journal to hold a line.
            let started = std::time::Instant::now();
            let mut lines = Vec::new();
            while started.elapsed() < std::time::Duration::from_secs(10) && lines.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(250));
                lines = items(
                    &s.read("session.getSessionJournal", json!({ "session": session }))?,
                    "journal",
                );
            }
            check(
                !lines.is_empty(),
                "the journal stayed empty while the microphone ran",
            )?;

            apply(s, "session.closeSession", json!({}), Some(&session))?;

            let live = s.read("session.getLiveSession", json!({ "id": session }))?;
            check(
                live["microphone"]["on"] == false,
                "the microphone is not paused once the session is closed",
            )?;
            let journal = items(
                &s.read("session.getSessionJournal", json!({ "session": session }))?,
                "journal",
            );
            check(
                journal.len() >= lines.len(),
                "the journal lost lines on closing",
            )?;
            check(
                journal
                    .iter()
                    .all(|l| !text(l, "t").is_empty() && !text(l, "text").is_empty()),
                "a journal line lacks its time or its text",
            )?;

            let summary = s.read(
                "session.getPreviousSessionSummary",
                json!({ "session": session }),
            )?;
            check(
                !text(&summary, "summary").is_empty(),
                "no summary was generated",
            )?;
            let settings = s.read("ai.getSettings", json!({}))?;
            let provider = text(&settings, "provider");
            let model = text(&settings["providers"][provider], "summaryModel");
            check(
                !model.is_empty() && text(&summary, "summaryMeta").contains(model),
                "the summary was not generated with the configured summary model",
            )?;

            Ok(())
        },
    );
}
