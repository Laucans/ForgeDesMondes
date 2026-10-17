# Contest — R1, « Plusieurs campagnes et oneshots »

Task #54 (Capability `campaign.listCampaigns`), milestone #18.

## The rule and its test

- Rule: **R1** — given two campaigns and a oneshot, when the DM opens the
  home page, then all three appear with their cover, the oneshot has only one
  session, and no account is asked.
- Test: `tests/rules/business/tests/r1_several_campaigns_and_oneshots.rs`: it
  reads `campaign.listCampaigns` from the running server and requires the
  three campaigns of `mock/fixtures/CampaignList.json` to be listed, each with
  an `art`, the oneshot with `sessions == 1`, and no `account`/`owner`.

## Why it cannot hold from this unit

The Capability is built (`crates/campaign/capabilities/list-campaigns/`, its
query registered, 9 hermetic tests green), so the rule is no longer pending;
against the rules' runtime it is red for three reasons, none inside the unit:

1. **The server does not dispatch it.** `Reads::run`
   (`crates/dataguard/server/src/reads.rs`) answers only `ai.getSettings`; any
   other identifier — `campaign.listPlayers` too — answers `data: null`.
   Observed: `POST /capabilities/campaign.listCampaigns` → `{"asOf":0,"data":null}`.
   Wiring a Capability there is an edit of `dataguard-server`'s `Cargo.toml` and
   `reads.rs`, plumbing that is not this task's unit (rule 1: one unit per task).
2. **No fixture reaches the database.** `fixtures()` in
   `crates/server/src/lib.rs` skips every file ("no aggregate holds it"), so
   the `campaign` table of a scratch database is empty: `cendres-valombre is
   not listed`.
3. **`sessions` cannot be counted.** The session table is absent and
   `campaign_overview_view` holds only `entity_counts`. The answer carries
   `sessions`, `lastPlayed` and `next` as `null` — an unknown count, never a
   made-up zero or one — so the oneshot check (`sessions == 1`) cannot hold.
   The spec anticipates this case. The Capability already answers a
   `session_count`, `last_played` or `next_session` column as it comes, so the
   Session aggregate's task only has to join the view and the query.

What this unit does satisfy: every item has its `art`, none carries `account`
or `owner` (unit tests), the shape is that of
`mock/fixtures/campaign.listCampaigns.json`.

## What is not mine to change

`tests/rules/`, `rules.json`, `scripts/rules-runtime.sh`, and the server's
plumbing outside this unit. R1 can pass once the server wires
`campaign.listCampaigns`, seeds the fixtures' campaigns, and the Session table
feeds the view's session count.
