# Rules contested: R14 — Médias téléversés et grille hexagonale réglée; R5 — Reprise depuis des notes en texte libre

**The rules.** R14 (`rules.json`): a battlemap uploaded, the DM sets its
grid, the preview follows live and a hex is always 5 feet. R5: notes pasted,
the AI proposes characters, places and events, and nothing is created until
the DM has validated the list. Both rules hold, and this change keeps them:
it adds only the command that creates the campaign they start from.

**The tests.** `tests/rules/business/tests/r14_media_uploaded_and_hex_grid_set.rs`
and `tests/rules/business/tests/r5_notes_extracted_then_validated.rs` start
with `new_campaign(s, "campagne")` (`tests/rules/business/tests/common/mod.rs`),
which posts `campaign.createCampaign` with no `caller` and waits for the
command to be `applied` or `rejected`. Every unit both tests name now exists,
`campaign.createCampaign` was the last one, so they are no longer pending.

**Why they cannot hold.** A creation of a campaign writes its id and its
kind, and the Campaign aggregate declares both `critical`
(`crates/campaign/aggregates/Campaign/aggregate.json`). The issue's
`## Architecture` lists both in `touches`, and the manifest declares them
(`crates/campaign/data-capabilities/create-campaign/data-capability.json`).
The engine (`harness-dataguard`, `guard::review_needed`) opens a review for
every command whose `touches` name a critical field when its caller does not
hold `role:approver`. The helper sends no caller, so the creation waits in
`awaiting_review` and the test fails at « campaign.createCampaign: not
settled after 20s ». The run on this branch:

```
R14 does not hold: campaign.createCampaign: not settled after 20s
R5 does not hold: campaign.createCampaign: not settled after 20s
```

Leaving `Campaign.id` and `Campaign.kind` out of `touches` would get the
creation past the review, but only by having the manifest under-declare what
it writes. That would void the review the aggregate asks for, and it is not
this task's call. It would not make either rule hold anyway: the next command
each test sends stalls the same way. `media.uploadMedia` touches
`Media.id` and `Media.kind`, and `ai.extractFromNotes` touches
`NoteImport.importedNoteId` and `NoteImport.campaignId`, all `critical`.

**What would make them hold.** The rules' helper (`Server::command`, or
`new_campaign` and `apply`) should send the DM's caller as the shell's
Micro-UIs do, as the approver of their own campaign:
`{"by": "dm", "from": "<Micro-UI>", "roles": ["role:approver"]}`. The other
way is to make the criticality of a new row's fields reviewable only on an
update, which is a change to the Campaign aggregate's contract or the
engine's. #297's contest already noted this for `soundbox.playSound`, and
for `campaign.createCampaign` (#273): « for the shell and the rules' helper
to settle together ».

Contested by the PR for issue #156 (`campaign.createCampaign`).
