# Contest — R1, « Plusieurs campagnes et oneshots »

Task #246 (the Session aggregate), milestone #18.

## The rule and its test

- Rule: **R1**: given two campaigns and a oneshot, when the DM opens the home
  page, then all three appear with their cover, the oneshot has a single
  session, and no account is asked for.
- Test: `tests/rules/business/tests/r1_several_campaigns_and_oneshots.rs`. It
  needs `capability:campaign.listCampaigns` and `aggregate:Session`, reads
  `campaign.listCampaigns` from the running server, and requires
  `sessions == 1` for the oneshot `nuit-carnaval` (`mock/fixtures/CampaignList.json`).

## Why it cannot hold from this unit

This task installs `crates/dataguard/aggregates/Session/aggregate.json`, the
last unit the test waits for. So R1 is no longer pending, and against the
rules' runtime it is red: « the oneshot nuit-carnaval does not have exactly one
session ». None of the reasons is inside this unit:

1. **No session is counted.** `campaign.listCampaigns` answers `sessions` from
   `campaign_overview_view.session_count`
   (`crates/campaign/capabilities/list-campaigns/src/lib.rs`). That view
   (`crates/data/migrations/0004_entity.sql`) has no session column yet, so the
   answer is `null`. Adding the column means a new migration and its entry in
   `data/model.json`, which #246's SPEC rules out (« this task writes no
   migration and does not touch `data/model.json` »). It also means a change to
   the `list-campaigns` persisted query, which is another unit's.
2. **No session reaches the database.** `crates/server/src/fixtures.rs` seeds
   only the Campaign, Player and Entity aggregates' tables. The oneshot's one
   session (the fixture's `"sessions": 1`) is never a `session` row, whatever
   the view counts.

What this unit does satisfy: the `session` table, keyed to its campaign, is
there to be counted, and its relations, views and invariants are tested
(`crates/dataguard/server/tests/session_invariants.rs`,
`crates/dataguard/server/tests/session_migration.rs`).

## What is not mine to change

`tests/rules/`, `rules.json`, `scripts/rules-runtime.sh`, the Campaign
aggregate's view and the `campaign.listCampaigns` Capability. R1 can hold once:

- `campaign_overview_view` counts each campaign's sessions (`session_count`,
  and `last_played` / `next_session` as the Capability already reads them);
- the server seeds the fixtures' sessions;
- or R1 waits for the unit that does this, e.g. `capability:session.listSessions`
  or `data-capability:session.createSession` (milestone 20), named in its needs.
