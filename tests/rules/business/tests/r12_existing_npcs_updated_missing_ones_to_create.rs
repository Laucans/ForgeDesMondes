//! Rule R12: PNJ existants mis à jour, PNJ manquants à créer.
mod common;

use common::*;

// given: un brief citant un PNJ existant et un PNJ inconnu
// when: il est traité
// then: le PNJ existant est mis à jour avec son diff et le PNJ inconnu est ajouté à la liste « à créer », sans création automatique.
#[test]
fn r12_existing_npcs_updated_missing_ones_to_create() {
    rule(
        "R12",
        &[
            "data-capability:ai.analyzeBrief",
            "capability:session.getLastBrief",
            "capability:campaign.listEntitiesToCreate",
            "capability:campaign.listEntityVersions",
            "capability:campaign.listCharacters",
            "capability:campaign.listCampaigns",
            "capability:session.listSessions",
            "data-capability:campaign.createCampaign",
            "data-capability:campaign.saveEntity",
            "data-capability:session.createSession",
        ],
        |s| {
            let campaign = new_campaign(s, "campagne")?;
            let known = new_character(
                s,
                &campaign,
                "pnj",
                json!({ "name": "Morlox", "notes": "Doit 40 po à Vesna." }),
            )?;
            let session = new_session(s, &campaign, &[])?;
            let brief = fixture("OralBrief");
            let transcript = brief["transcript"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ");
            let versions = |s: &Server| -> Result<Vec<Value>, String> {
                Ok(items(
                    &s.read("campaign.listEntityVersions", json!({ "entity": known }))?,
                    "versions",
                ))
            };
            let before = versions(s)?.len();
            let characters_before = characters(s, &campaign)?.len();

            apply(
                s,
                "ai.analyzeBrief",
                json!({ "session": session, "text": transcript }),
                None,
            )?;

            // The existing NPC is updated, with its diff.
            let after = versions(s)?;
            check(
                after.len() == before + 1,
                "the existing NPC was not updated",
            )?;
            let newest = after
                .iter()
                .max_by_key(|v| v["v"].as_u64())
                .ok_or("no version")?;
            check(text(newest, "origin") == "ia", "the update is not the AI's")?;
            check(
                newest["changes"].as_array().is_some_and(|c| !c.is_empty()),
                "the update has no diff",
            )?;

            // The unknown NPCs are listed to create, none created.
            let to_create = items(
                &s.read(
                    "campaign.listEntitiesToCreate",
                    json!({ "campaign": campaign }),
                )?,
                "items",
            );
            let missing_pnj: Vec<&Value> = brief["result"]["missing"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|m| text(m, "type") == "pnj")
                .collect();
            check(!missing_pnj.is_empty(), "the fixtures hold missing NPCs")?;
            for m in missing_pnj {
                let listed = by_name(&to_create, text(m, "name"))
                    .ok_or_else(|| format!("{} is not in the list to create", text(m, "name")))?;
                check(
                    text(listed, "status") == "todo",
                    "a missing NPC is not waiting to be created",
                )?;
            }
            check(
                characters(s, &campaign)?.len() == characters_before,
                "a character was created without the DM",
            )?;

            Ok(())
        },
    );
}
