//! Rule R6: Alias et vocabulaire de détection.
mod common;

use common::*;

// given: l'alias « la taverne » sur « La Taverne du Sanglier »
// when: la transcription démarre
// then: son vocabulaire contient les noms et les alias de toutes les entités de la campagne.
#[test]
fn r6_aliases_feed_the_transcription_vocabulary() {
    rule(
        "R6",
        &[
            "data-capability:live.setMicrophone",
            "capability:live.getTranscriptionVocabulary",
            "capability:campaign.listCharacters",
            "capability:campaign.listPlaces",
            "capability:campaign.listEvents",
        ],
        |s| {
            let places = places(s, CAMPAIGN)?;
            let tavern = by_id(&places, "taverne-sanglier").ok_or("the tavern is missing")?;
            check(
                tavern["aliases"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|x| x == "la taverne")),
                "the alias « la taverne » is missing from the tavern",
            )?;

            apply(s, "live.setMicrophone", json!({ "on": true }), None)?;
            let data = s.read(
                "live.getTranscriptionVocabulary",
                json!({ "campaign": CAMPAIGN }),
            )?;
            apply(s, "live.setMicrophone", json!({ "on": false }), None)?;
            let vocabulary: Vec<String> = items(&data, "terms")
                .iter()
                .filter_map(|t| t.as_str().map(str::to_lowercase))
                .collect();

            let mut entities = characters(s, CAMPAIGN)?;
            entities.extend(places.iter().cloned());
            entities.extend(events(s, CAMPAIGN)?);
            for e in &entities {
                let mut words = vec![text(e, "name").to_string()];
                words.extend(
                    e["aliases"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|a| a.as_str().map(String::from)),
                );
                for word in words {
                    check(
                        vocabulary.contains(&word.to_lowercase()),
                        &format!("« {word} » is not in the vocabulary"),
                    )?;
                }
            }

            Ok(())
        },
    );
}
