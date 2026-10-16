//! Rule R95: Confirmation avant de quitter une fiche modifiée.
mod common;

use common::*;

// given: la fiche d'un PNJ dont la description a été modifiée sans être enregistrée
// when: le DM clique sur un autre élément du rail et choisit tour à tour « Rester », « Abandonner » puis, après une nouvelle modification, « Enregistrer et continuer »
// then: « Rester » garde la fiche et la modification affichées, « Abandonner » ouvre l'élément choisi et la modification est perdue, et « Enregistrer et continuer » enregistre la modification avant d'ouvrir l'élément choisi
#[test]
fn r95_confirmation_before_leaving_a_modified_sheet() {
    rule(
        "R95",
        &[
            "data-capability:campaign.saveEntity",
            "capability:campaign.getEntity",
            "capability:campaign.listCharacters",
            "capability:campaign.listCampaigns",
            "data-capability:campaign.createCampaign",
        ],
        |s| {
            // The bar and its three choices are the page's: tests/rules/journey/R95-*.spec.ts walks them.
            // Here, what the product keeps: a change not saved is never stored, a saved one is.
            let campaign = new_campaign(s, "campagne")?;
            let id = new_character(
                s,
                &campaign,
                "pnj",
                json!({ "description": "Description d'origine" }),
            )?;
            let other = new_character(
                s,
                &campaign,
                "pnj",
                json!({ "description": "L'autre élément" }),
            )?;

            // The description is edited in the sheet and the DM picks the other element:
            // « Rester » then « Abandonner » send nothing, so the stored sheet is still the original.
            check(
                text(&get_entity(s, &id)?, "description") == "Description d'origine",
                "an unsaved change reached the store",
            )?;
            check(
                text(&get_entity(s, &other)?, "description") == "L'autre élément",
                "the chosen element changed without being opened by a save",
            )?;

            // A new edit, then « Enregistrer et continuer »: the sheet is saved, then the chosen element opens.
            apply(
                s,
                "campaign.saveEntity",
                json!({ "campaign": campaign, "entity": { "id": id, "description": "Description modifiée" } }),
                None,
            )?;
            check(
                text(&get_entity(s, &id)?, "description") == "Description modifiée",
                "the change was not saved before moving on",
            )?;
            check(
                text(&get_entity(s, &other)?, "description") == "L'autre élément",
                "the chosen element does not open as it was stored",
            )?;

            Ok(())
        },
    );
}
