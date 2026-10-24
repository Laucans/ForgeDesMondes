# Contest — R101, and R20–R23, R105, R107, R114 behind it

Pull request #423, milestone #25 (battlemap instances: tokens, hit points
and combat). Contest #424 settled R101's battlemap setup (#426). It is still
red, now for a different reason in the test. Run locally with `--no-fail-fast`
against the rules' runtime at `bced4c0`: 8 reds. CI only shows R101,
because it stops at the first red binary.

## R101 — « Image de token par défaut »

- Test: `tests/rules/business/tests/r101_default_token_image.rs`.
  To add the portrait it sends `campaign.saveEntity` with
  `"entity": { "id": …, "portrait": true, "crop": … }`.
- It fails with `campaign.saveEntity was not applied: … "status":"rejected",
  "violations":["default-token"]`.
- Why: `saveEntity`'s contract (milestone #18,
  `crates/campaign/data-capabilities/save-entity/data-capability.json`) says
  `"entity.portrait": "uuid | null — the portrait's media"`. Any other value
  is not taken as a portrait, so the save carries a crop and no portrait.
  `default-token` (Entity aggregate: a crop is only valid with a portrait)
  then refuses it, as it should.
- Checked on the runtime: I patched the test for a moment (reverted, not part
  of this pull request) to upload an illustration with `media.uploadMedia`
  (both media units are already in the test's needs) and send its id as
  `portrait`. R101 then passes from end to end: discs B 1/2/3 with no
  portrait, then every token shows `portrait: true` and the crop, in both
  views.
- Wanted from the rules' task: the test sends a media id as `portrait`, not
  `true`.

## R20, R21, R22, R23, R105, R107, R114 — the shared `fresh_instance`

- Helper: `fresh_instance` (`tests/rules/business/tests/common/mod.rs`) runs
  `new_session(s, CAMPAIGN, …)` with `CAMPAIGN = "cendres-valombre"`, then
  `new_instance(…, "bm-gue")`. The tests then place and roll for `"elowen"` and
  `"kael"`, all fixture slugs.
- Every one fails first with
  `commanding session.createSession: …/commands: status code 404`.
- Why: the server maps fixture slugs to uuids on `/capabilities/` reads only
  (`crates/server/src/names.rs`, D8). Its own test asserts that `/commands` is
  deliberately not renamed. A command naming `cendres-valombre` finds no
  campaign. #426 already fixed R101 and R108 this way: each test makes its own
  campaign and battlemap with `new_campaign` and `new_battlemap`.
- Wanted from the rules' task: the same for `fresh_instance` and its callers.
  Each makes its own campaign, battlemap and PCs instead of naming fixture
  slugs in commands. Or rule that D8 covers `/commands`, which is then the
  server's task, not this milestone's.

## What is not mine to change

`tests/rules/`, `rules.json`, `campaign.saveEntity` (milestone #18's contract)
and the server's slug mapping. This milestone's units are not in question.
