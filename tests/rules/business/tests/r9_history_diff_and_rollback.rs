//! Rule R9: Historique, diff et retour arrière.
mod common;

use common::*;

// given: une modification appliquée par l'IA ou par D&D Beyond
// when: le DM ouvre l'historique
// then: une nouvelle version existe avec son diff, et annuler ce changement ou revenir à une version antérieure restaure exactement l'état d'avant.
#[test]
fn r9_history_diff_and_rollback() {
    rule(
        "R9",
        &[
            "data-capability:ai.proposeCharacter",
            "data-capability:campaign.restoreEntityVersion",
            "capability:campaign.listEntityVersions",
            "capability:campaign.getEntity",
            "capability:campaign.listCampaigns",
            "capability:campaign.listCharacters",
            "data-capability:campaign.createCampaign",
            "data-capability:campaign.saveEntity",
        ],
        |s| {
            let campaign = new_campaign(s, "campagne")?;
            let prompt = text(&fixture("EntityEditor")["dictations"], "default").to_string();
            let id = new_character(s, &campaign, "pnj", json!({ "prompt": prompt }))?;
            let versions = |s: &Server| -> Result<Vec<Value>, String> {
                Ok(items(
                    &s.read("campaign.listEntityVersions", json!({ "id": id }))?,
                    "versions",
                ))
            };
            let top = |list: &[Value]| {
                list.iter()
                    .filter_map(|v| v["v"].as_u64())
                    .max()
                    .unwrap_or(0)
            };
            // The state restored must be exactly the one before: compare without the bookkeeping.
            let state = |s: &Server| -> Result<Value, String> {
                let mut e = get_entity(s, &id)?;
                if let Some(o) = e.as_object_mut() {
                    o.remove("version");
                    o.remove("updatedAt");
                }
                Ok(e)
            };

            let first = versions(s)?;
            let (v_before, state_before) = (top(&first), state(s)?);

            // A change applied by the AI is versioned with its diff.
            apply(
                s,
                "ai.proposeCharacter",
                json!({ "prompt": prompt }),
                Some(&id),
            )?;
            let list = versions(s)?;
            check(
                top(&list) == v_before + 1,
                "the AI change made no new version",
            )?;
            let newest = list
                .iter()
                .find(|v| v["v"].as_u64() == Some(top(&list)))
                .ok_or("no newest version")?;
            check(
                text(newest, "origin") == "ia",
                "the version is not marked as the AI's",
            )?;
            let diff = newest["changes"].as_array().ok_or("no diff")?;
            check(
                !diff.is_empty()
                    && diff.iter().all(|c| {
                        c.get("field").is_some() && c.get("old").is_some() && c.get("new").is_some()
                    }),
                "the diff lacks field, old or new",
            )?;
            check(state(s)? != state_before, "the AI changed nothing")?;

            // Undoing that change restores the exact state before.
            let every_change: Vec<usize> = (0..diff.len()).collect();
            apply(
                s,
                "campaign.restoreEntityVersion",
                json!({ "version": top(&list) - 1, "changes": every_change }),
                Some(&id),
            )?;
            check(
                state(s)? == state_before,
                "undoing the change did not restore the state",
            )?;

            // So does going back to the earlier version.
            apply(
                s,
                "ai.proposeCharacter",
                json!({ "prompt": prompt }),
                Some(&id),
            )?;
            apply(
                s,
                "campaign.restoreEntityVersion",
                json!({ "version": v_before }),
                Some(&id),
            )?;
            check(
                state(s)? == state_before,
                "going back to the earlier version did not restore the state",
            )?;

            Ok(())
        },
    );
}
