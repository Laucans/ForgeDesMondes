//! Rule R77: Seul du texte part vers le fournisseur.
mod common;

use common::*;

// given: le micro actif pendant une séance ou un brief oral, et un fournisseur de génération configuré, d'abord en mode « API », puis en mode « Binaire »
// when: le DM parle, puis demande une génération (statblock, background, analyse du brief ou résumé) dans chacun des deux modes
// then: aucune donnée audio ne sort de la machine, et ce qui est transmis à l'API ou au binaire ne contient que du texte
#[test]
fn r77_only_text_goes_to_the_provider() {
    rule(
        "R77",
        &[
            "data-capability:ai.saveSettings",
            "data-capability:ai.proposeCharacter",
            "data-capability:ai.analyzeBrief",
            "data-capability:session.closeSession",
            "data-capability:live.setMicrophone",
            "capability:ai.listProviderRequests",
            "capability:campaign.listCampaigns",
            "capability:campaign.listCharacters",
            "capability:session.listSessions",
            "data-capability:campaign.createCampaign",
            "data-capability:campaign.saveEntity",
            "data-capability:session.createSession",
        ],
        |s| {
            let campaign = new_campaign(s, "campagne")?;
            let transcript = fixture("OralBrief")["transcript"].to_string();
            let prompt = text(&fixture("EntityEditor")["dictations"], "default").to_string();
            let sent = |s: &Server| -> Result<Vec<Value>, String> {
                Ok(items(
                    &s.read("ai.listProviderRequests", json!({}))?,
                    "requests",
                ))
            };

            for mode in ["api", "binary"] {
                if mode == "api" {
                    use_api(s, "opencode")?;
                } else {
                    use_binary(s, "claude")?;
                }
                let before = sent(s)?.len();

                // The DM speaks, then asks for each kind of generation.
                apply(s, "live.setMicrophone", json!({ "on": true }), None)?;
                let npc = new_character(s, &campaign, "pnj", json!({ "prompt": prompt }))?;
                apply(
                    s,
                    "ai.proposeCharacter",
                    json!({ "prompt": prompt }),
                    Some(&npc),
                )?;
                let session = new_session(s, &campaign, &[])?;
                apply(
                    s,
                    "ai.analyzeBrief",
                    json!({ "session": session, "text": transcript }),
                    None,
                )?;
                apply(s, "session.closeSession", json!({}), Some(&session))?;
                apply(s, "live.setMicrophone", json!({ "on": false }), None)?;

                let all = sent(s)?;
                let new = &all[before.min(all.len())..];
                check(
                    new.len() >= 3,
                    &format!("in {mode} mode, the generations reached no provider"),
                )?;
                check(
                    new.iter()
                        .all(|r| r["modalities"] == json!(["text"]) && r.get("audio").is_none()),
                    &format!("in {mode} mode, something other than text was sent"),
                )?;
            }

            Ok(())
        },
    );
}
