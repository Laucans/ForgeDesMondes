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
            "capability:ai.listProviderRequests",
        ],
        |s| {
            // The sentence the stand-in transcription says, naming an entity by an alias.
            let feed = items(&fixture("SuggestionPill"), "feed");
            let named_by_alias = feed
                .iter()
                .find(|f| text(f, "kind") == "entity" && text(f, "via") != text(f, "name"))
                .ok_or("the fixtures hold no entity named by an alias")?;
            let wanted = text(named_by_alias, "id").to_string();

            apply(s, "live.setMicrophone", json!({ "on": true }), None)?;
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
            apply(s, "live.setMicrophone", json!({ "on": false }), None)?;
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
