//! Rule R2: Export et import d'une campagne en archive.
mod common;

use common::*;

// given: une campagne exportée puis importée alors qu'elle existe déjà
// when: l'import se termine
// then: une copie distincte est créée, l'originale reste intacte et la date du dernier export est affichée.
#[test]
fn r2_campaign_archive_export_and_import() {
    rule(
        "R2",
        &[
            "data-capability:campaign.exportCampaign",
            "data-capability:campaign.importCampaign",
            "capability:campaign.getCampaign",
            "capability:campaign.listCampaigns",
        ],
        |s| {
            let original = CAMPAIGN;
            let before = s.read("campaign.getCampaign", json!({ "id": original }))?;
            let ids_before: Vec<String> =
                items(&s.read("campaign.listCampaigns", json!({}))?, "campaigns")
                    .iter()
                    .map(|c| text(c, "id").to_string())
                    .collect();

            apply(s, "campaign.exportCampaign", json!({}), Some(original))?;
            let exported = s.read("campaign.getCampaign", json!({ "id": original }))?;
            let archive = text(&exported["archive"], "file").to_string();
            check(!archive.is_empty(), "the export left no archive")?;

            // The campaign exists already: the import makes a copy.
            apply(
                s,
                "campaign.importCampaign",
                json!({ "archive": archive }),
                None,
            )?;

            let after = items(&s.read("campaign.listCampaigns", json!({}))?, "campaigns");
            let created: Vec<&Value> = after
                .iter()
                .filter(|c| !ids_before.iter().any(|id| id == text(c, "id")))
                .collect();
            check(
                created.len() == 1,
                "the import creates exactly one campaign",
            )?;
            check(
                text(created[0], "id") != original,
                "the copy has the original's identifier",
            )?;

            let untouched = s.read("campaign.getCampaign", json!({ "id": original }))?;
            check(
                untouched["campaign"]["name"] == before["campaign"]["name"]
                    && untouched["campaign"]["counts"] == before["campaign"]["counts"],
                "the original changed",
            )?;

            let shown = text(&untouched["campaign"], "lastExport");
            check(
                !shown.is_empty(),
                "the date of the last export is not shown",
            )?;
            check(
                shown >= text(&before["campaign"], "lastExport"),
                "the date of the last export went back",
            )?;

            Ok(())
        },
    );
}
