# Rule contested: R14 — Médias téléversés et grille hexagonale réglée

**The rule.** R14 (`rules.json`): a battlemap is uploaded, the DM sets the
grid's size, offset, orientation, colour and opacity, the preview follows,
and one hex is still 5 feet. The rule itself holds and this change keeps
it.

**The test.** `tests/rules/business/tests/r14_media_uploaded_and_hex_grid_set.rs`
saves
`{ "size": 40, "ox": 5, "oy": 7, "orient": "flat", "color": "accent", "opacity": 70 }`
through `media.saveGridSettings`, then expects `media.getBattlemap` to answer
`grid.opacity == 70`.

**Why it cannot hold.** The test treats the opacity as a percentage (70).
Everything the product has already built treats it as a fraction of 1:

- `concepts/HexGrid/v1/concept.json`: `opacity` is "a number from 0 to 1
  inclusive, a fraction of 1: a screen that shows a percentage divides it
  by 100 before it reaches the grid";
- the Media aggregate's invariant `grid-values-range`
  (`crates/dataguard/aggregates/Media/aggregate.json`, judged in
  `crates/dataguard/server/src/media.rs`) and the table's CHECK
  `grid_values_range` (`crates/data/migrations/0005_media.sql`,
  `opacity numeric(3,2) … opacity BETWEEN 0 AND 1`) both reject 70;
- `media.getBattlemap` (`crates/media/capabilities/get-battlemap`) answers
  the opacity "as a fraction of 1, not a percentage";
- the GridSettings Micro-UI (`apps/media/grid-settings/src/model.ts`)
  shows a percentage and sends `opacity / 100`.

So once `media.uploadMedia` and `campaign.createCampaign` land, the save in
R14's test is rejected under `grid-values-range`, and the test fails at
"the grid's opacity was not kept". The only ways to make it pass would
break HexGrid@1, a done aggregate invariant, a table CHECK and a done
Capability. Accepting 70 and reading it as 0.7 would not help either:
`getBattlemap` would answer 0.7, not 70.

**What would make it hold.** The test should save `"opacity": 0.7` (the
fraction HexGrid@1 states), and expect 0.7 back, as the GridSettings panel
does. The mock-up's fixtures (`mock/fixtures/media.getBattlemap.json`) write
a percentage, which is probably where the test's 70 comes from.

Contested by the PR for issue #128 (`media.saveGridSettings`).
