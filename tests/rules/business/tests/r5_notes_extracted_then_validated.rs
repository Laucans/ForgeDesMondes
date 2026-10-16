//! Rule R5: Reprise depuis des notes en texte libre.
mod common;

use common::*;

// given: des notes collées
// when: l'IA propose personnages, lieux et événements
// then: aucune entité n'est créée tant que le DM n'a pas validé la liste, et seules les entités validées le sont.
#[test]
fn r5_notes_extracted_then_validated() {
    rule(
        "R5",
        &[
            "data-capability:ai.extractFromNotes",
            "data-capability:campaign.createEntitiesFromNote",
            "capability:campaign.listImportedNotes",
            "capability:campaign.listCharacters",
            "capability:campaign.listPlaces",
            "capability:campaign.listEvents",
        ],
        |s| {
            let campaign = new_campaign(s, "campagne")?;
            let count = |s: &Server| -> Result<usize, String> {
                Ok(characters(s, &campaign)?.len()
                    + places(s, &campaign)?.len()
                    + events(s, &campaign)?.len())
            };
            let before = count(s)?;
            let draft = text(&fixture("NoteImport"), "draft").to_string();

            apply(
                s,
                "ai.extractFromNotes",
                json!({ "campaign": campaign, "text": draft }),
                None,
            )?;
            let notes = s.read(
                "campaign.listImportedNotes",
                json!({ "campaign": campaign }),
            )?;
            let proposals = items(&notes, "proposals");
            check(
                proposals.len() >= 2,
                "the AI proposed fewer than two entities",
            )?;
            check(
                count(s)? == before,
                "an entity was created before the DM validated",
            )?;

            let accepted = &proposals[0];
            let refused = &proposals[1];
            apply(
                s,
                "campaign.createEntitiesFromNote",
                json!({ "campaign": campaign, "accepted": [text(accepted, "id")] }),
                None,
            )?;
            check(
                count(s)? == before + 1,
                "exactly the validated entity must be created",
            )?;
            let named: Vec<String> = characters(s, &campaign)?
                .iter()
                .chain(places(s, &campaign)?.iter())
                .chain(events(s, &campaign)?.iter())
                .map(|e| text(e, "name").to_string())
                .collect();
            check(
                named.iter().any(|n| n == text(accepted, "name")),
                "the validated entity is missing",
            )?;
            check(
                !named.iter().any(|n| n == text(refused, "name")),
                "a refused entity was created",
            )?;

            Ok(())
        },
    );
}
