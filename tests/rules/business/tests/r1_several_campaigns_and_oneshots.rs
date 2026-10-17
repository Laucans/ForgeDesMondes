//! Rule R1: Plusieurs campagnes et oneshots.
mod common;

use common::*;

// given: deux campagnes et un oneshot
// when: le DM ouvre l'accueil
// then: les trois apparaissent avec leur couverture, le oneshot n'a qu'une séance, et aucun compte n'est demandé.
#[test]
fn r1_several_campaigns_and_oneshots() {
    rule("R1", &["capability:campaign.listCampaigns"], |s| {
        // No credential is sent: a read that is answered asks for no account.
        let data = s.read("campaign.listCampaigns", json!({}))?;
        let listed = items(&data, "campaigns");
        let expected = items(&fixture("CampaignList"), "campaigns");
        let campaigns = expected
            .iter()
            .filter(|c| text(c, "type") == "campagne")
            .count();
        check(campaigns >= 2, "the fixtures hold two campaigns")?;
        for c in &expected {
            let id = text(c, "id");
            let shown = by_id(&listed, id).ok_or_else(|| format!("{id} is not listed"))?;
            let cover = !text(shown, "art").is_empty() || !text(shown, "cover").is_empty();
            check(cover, &format!("{id} has no cover"))?;
            if text(c, "type") == "oneshot" {
                check(
                    shown.get("sessions").and_then(Value::as_u64) == Some(1),
                    &format!("the oneshot {id} does not have exactly one session"),
                )?;
            }
        }
        check(
            expected.iter().any(|c| text(c, "type") == "oneshot"),
            "the fixtures hold a oneshot",
        )?;
        check(
            listed
                .iter()
                .all(|c| c.get("account").is_none() && c.get("owner").is_none()),
            "a campaign is tied to an account",
        )?;

        Ok(())
    });
}
