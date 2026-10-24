# Contested rule: R9 — Historique, diff et retour arrière

**The test:** `tests/rules/business/tests/r9_history_diff_and_rollback.rs`,
contested by issue #447 (`ai.proposeCharacter`), which makes the rule active.

**Why it cannot hold:** the test reads the history with
`s.read("campaign.listEntityVersions", json!({ "id": id }))`. The Capability
`campaign.listEntityVersions` takes its element as `entity`, not `id`
(`crates/campaign/capabilities/list-entity-versions/capability.json`:
`"input": { "entity": "string" }`; its query is
`query ListEntityVersions($entity: ID!)`). With `id`, the read answers
`data: null` for every element. So `versions` is always empty, `top` is 0 before
and after the AI change, and the test stops at "the AI change made no new
version", whatever the DataCapabilities write.

Neither side is this task's to change:

- the test is under `tests/rules/` (rule 11, gate `rule-tests-untouched`);
- `campaign.listEntityVersions` is built by milestone 18 (`plan/graph.json`,
  rule 17, gate `contracts-untouched`), and its other consumers (the
  VersionHistory Micro-UI, the mock-up) read it with `entity`.

**What does hold:** the same flow, run by hand against `rules-runtime.sh serve`
with the read asked as `{ "entity": id }`, passes every check of the test:

```
new version: True | origin: ia | diff ok: True | changed: True
undo restores exactly: True
back to earlier restores exactly: True
```

The in-memory test
`crates/ai/data-capabilities/propose-character/tests/submit.rs`
(`undoing_the_proposal_or_going_back_to_the_version_before_restores_the_sheet_exactly`)
also proves the proposal's version, its undo and the return to the earlier
version.

**What would make it hold:** the test reads with `json!({ "entity": id })` (a
`harness:rules` change), or the Capability accepts `id` as well, through the
contract manager.
