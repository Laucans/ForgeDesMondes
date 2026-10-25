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

            // The archive is the export's own answer: the command's result
            // carries it as `attachment: {file, content}` once applied.
            let exported = apply(s, "campaign.exportCampaign", json!({}), Some(original))?;
            let attachment = &exported["attachment"];
            check(
                !text(attachment, "file").is_empty() && attachment["content"].is_object(),
                "the export left no archive",
            )?;
            let archive = attachment["content"].clone();

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
                untouched["name"] == before["name"] && untouched["counts"] == before["counts"],
                "the original changed",
            )?;

            let shown = text(&untouched, "lastExport");
            check(
                !shown.is_empty(),
                "the date of the last export is not shown",
            )?;
            check(
                shown > text(&before, "lastExport"),
                "the date of the last export is not the one of the export just made",
            )?;

            Ok(())
        },
    );
}
