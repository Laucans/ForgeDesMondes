//! Rule R18: Suggestions du micro en direct.
mod common;

use common::*;

// given: le micro en marche et un PNJ nommé par son alias
// when: la phrase est prononcée
// then: il apparaît en quelques secondes dans la pile des 5 dernières suggestions, et aucun audio ne quitte la machine.
#[test]
fn r18_live_microphone_suggestions() {
    rule(
        "R18",
        &[
            "data-capability:live.setMicrophone",
            "capability:live.listSuggestions",
            "capability:session.listSessions",
            "capability:ai.listProviderRequests",
        ],
        |s| {
            // The sentence the stand-in transcription says: Vesna, a PNJ, named by her alias « la Balafrée ».
            let feed = items(&fixture("SuggestionPill"), "feed");
            let named_by_alias = feed
                .iter()
                .find(|f| text(f, "id") == "vesna" && text(f, "type") == "pnj")
                .ok_or("the fixtures hold no PNJ named by an alias")?;
            let aliases = items(&fixture("entities"), "characters")
                .into_iter()
                .find(|c| text(c, "id") == "vesna")
                .map(|c| c["aliases"].clone())
                .unwrap_or(Value::Null);
            check(
                aliases
                    .as_array()
                    .is_some_and(|a| a.iter().any(|x| x == text(named_by_alias, "via"))),
                "the phrase does not name the PNJ by one of its aliases",
            )?;
            let wanted = text(named_by_alias, "id").to_string();

            // The microphone listens in one session: the fixture campaign's live one, else its
            // planned one (the manifest's order), named explicitly (unscoped, any campaign's live
            // session would be picked first).
            let sessions = items(
                &s.read("session.listSessions", json!({ "campaign": CAMPAIGN }))?,
                "sessions",
            );
            let planned = sessions
                .iter()
                .find(|x| text(x, "status") == "live")
                .or_else(|| sessions.iter().find(|x| text(x, "status") == "prep"))
                .map(|x| text(x, "id").to_string())
                .ok_or("the fixture campaign has no live or planned session")?;

            apply(s, "live.setMicrophone", json!({ "on": true }), Some(&planned))?;
            let started = std::time::Instant::now();
            let mut shown = Vec::new();
            while started.elapsed() < std::time::Duration::from_secs(5) {
                let data = s.read("live.listSuggestions", json!({ "campaign": CAMPAIGN }))?;
                shown = items(&data, "suggestions");
                if shown.iter().any(|x| text(x, "id") == wanted) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            apply(s, "live.setMicrophone", json!({ "on": false }), Some(&planned))?;
            check(
                shown.iter().any(|x| text(x, "id") == wanted),
                "the entity named by its alias did not appear within a few seconds",
            )?;
            check(
                shown.len() <= 5,
                "the stack holds more than the last 5 suggestions",
            )?;

            // Nothing but text ever went to a provider.
            let sent = items(&s.read("ai.listProviderRequests", json!({}))?, "requests");
            check(
                sent.iter().all(|r| r["modalities"] == json!(["text"])),
                "something other than text left the machine",
            )?;

            Ok(())
        },
    );
}
