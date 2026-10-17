//! Rule R100: Création assistée avec les deux fournisseurs.
mod common;

use common::*;

// given: un PNJ avec un prompt initial dicté, et deux réglages valides : l'API cloud puis l'endpoint opencode
// when: le DM demande la proposition IA avec chacun des deux fournisseurs
// then: chaque fournisseur remplit le background, le comportement et un statblock complet (caractéristiques, CA, PV, capacités) dans les mêmes champs de la fiche
#[test]
fn r100_assisted_creation_with_both_providers() {
    rule(
        "R100",
        &[
            "data-capability:ai.saveSettings",
            "data-capability:ai.proposeCharacter",
            "capability:campaign.getEntity",
            "capability:campaign.listCampaigns",
            "capability:campaign.listCharacters",
            "data-capability:campaign.createCampaign",
            "data-capability:campaign.saveEntity",
        ],
        |s| {
            let campaign = new_campaign(s, "campagne")?;
            let prompt = text(&fixture("EntityEditor")["dictations"], "default").to_string();
            let mut shapes = Vec::new();
            for provider in ["cloud", "opencode"] {
                use_api(s, provider)?;
                let id = new_character(s, &campaign, "pnj", json!({ "prompt": prompt }))?;
                apply(
                    s,
                    "ai.proposeCharacter",
                    json!({ "prompt": prompt }),
                    Some(&id),
                )?;
                let sheet = get_entity(s, &id)?;
                check(
                    !text(&sheet, "background").is_empty(),
                    &format!("{provider}: no background"),
                )?;
                check(
                    !text(&sheet, "behavior").is_empty(),
                    &format!("{provider}: no behavior"),
                )?;
                for stat in ["FOR", "DEX", "CON", "INT", "SAG", "CHA"] {
                    check(
                        sheet["stats"][stat].as_i64().is_some(),
                        &format!("{provider}: no {stat}"),
                    )?;
                }
                check(
                    sheet["ca"].as_i64().is_some_and(|v| v > 0),
                    &format!("{provider}: no CA"),
                )?;
                check(
                    sheet["pvMax"].as_i64().is_some_and(|v| v > 0),
                    &format!("{provider}: no PV"),
                )?;
                check(
                    sheet["features"].as_array().is_some_and(|f| !f.is_empty()),
                    &format!("{provider}: no abilities"),
                )?;
                let mut fields: Vec<String> = sheet
                    .as_object()
                    .map(|o| o.keys().cloned().collect())
                    .unwrap_or_default();
                fields.sort();
                shapes.push(fields);
            }
            check(
                shapes[0] == shapes[1],
                "the two providers do not fill the same fields",
            )?;

            Ok(())
        },
    );
}
