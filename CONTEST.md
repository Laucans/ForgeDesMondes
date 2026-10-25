# Contest — R2, `tests/rules/business/tests/r2_campaign_archive_export_and_import.rs`

**Rule R2** — export and import of a campaign as an archive: a campaign
exported then imported while it still exists gives a separate copy, the
original stays intact and the date of its last export is shown.

**The test** reads `campaign.getCampaign` as `{campaign: {name, counts,
lastExport}, archive: {file}}` and passes `archive.file` as the payload of
`campaign.importCampaign`. It fails on its fourth step, before it reaches the
import:

```
R2 does not hold: the export left no archive
```

**Why it cannot hold as written**

1. `campaign.getCampaign` (`crates/campaign/capabilities/get-campaign/`, a
   contract another milestone produces; gate `contracts-untouched`) answers
   the campaign's overview flat — `{id, name, type, …, lastExport, counts}` —
   and has no `archive` member and no `campaign` wrapper. A Capability only
   reads (rule 2), and the archive of an export is the answer of the export
   command itself (`attachment: {file, content}`), never a row of the tables a
   read could return. The test's `before["campaign"]["name"]`,
   `["campaign"]["counts"]` and `["campaign"]["lastExport"]` fail on the same
   shape.
2. Even with an `archive` on the read, `archive.file` is the archive's file
   *name* (`les-cendres-de-valombre-2026-10-18T09-30-00-123Z.json`); the
   import's payload is the archive's document (`{format: "campaign-archive",
   version: 1, …}`), or the text of that JSON — what the export answers as
   `content`.

This unit (`campaign.importCampaign`) is built and proven on its own
(`crates/campaign/data-capabilities/import-campaign/tests/`, from the Concept's
fixtures through the engine to the tables): a separate copy, the original's
rows byte for byte unchanged and its last export kept, two copies for the same
archive twice, 422 before anything is queued for an archive of another format
or version. What R2 still needs is a change of the read contract
(`campaign.getCampaign` answering `{campaign, archive: {file}}` with the
archive's content), which the contract manager owns, or a rewrite of the test
to read the archive from the export's own result.
