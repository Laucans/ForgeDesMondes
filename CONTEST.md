# Contest — R11 and R15 on the frozen preparation-seance screen

Freezing `apps/screens/preparation-seance.json` (task #219) turns R13 green
and brings R11 and R15 to run for the first time. Both stay red, for a reason
that is in neither the composition nor a Micro-UI. Checked on a fresh runtime
(`bash scripts/rules-runtime.sh serve`, restarted before each run: the store
persists), with the props the product's data demands (`OralBrief.session`,
`SceneBoard.campaign`, `Battlemap.campaign`) — without them the tests fail
earlier, and those are in the composition now.

## R15 « Instance de battlemap dans une scène »

- Test: `tests/rules/journey/R15-battlemap-instance-in-a-scene.spec.ts`,
  lines 28-34.
- It measures a token's `boundingBox()` — viewport coordinates — right after
  placing it (Playwright has scrolled the roster's button into view by then),
  reloads the screen with `open(page, 'preparation-seance')` (scroll back at
  the top), and expects the same `{ x, y }`.
- The shell stacks the composition's Micro-UIs in a column, and the mock-up
  puts the battlemap last (`mock/pages/preparation-seance.html`, after the
  summary, the brief, the scenes and the aside): the map starts at y = 4659 on
  a 5337 px page. Elowen is at `y: 521` when placed, `y: 4725` after the
  reload — the same token at the same place in the map, 4204 px apart in the
  viewport. The instance does keep it (the token is there after the reload).
- Checked: with `Battlemap` laid out first in the composition — so that the
  map is above the fold and nothing scrolls — R15 passes. The mock-up says
  otherwise, and the acceptance criterion of #219 is that the layout matches
  it, so the order is kept.
- Wanted: compare the token's position in the map (relative to the
  `[data-component="Battlemap"]` box, or its `data-*` coordinates), not in the
  viewport; or a human's decision to put the battlemap first on this screen.

## R11 « Brief oral découpé en scènes »

- Test: `tests/rules/journey/R11-oral-brief-cut-into-scenes.spec.ts`,
  lines 37-45.
- It expects the board, once the brief is processed, to hold exactly the
  mock-up's three scenes (`fx('SceneBoard').scenes`: « Retour à la Taverne du
  Sanglier », « La route des Corbeaux », « L’embuscade au Gué »), reordered,
  then four after « Ajouter une scène».
- On the real store the session s4 already holds those three (the server's
  fixtures, #537), and the brief the fake `llm` port answers
  (`crates/ports/llm/src/fake.rs`, `brief_answer`) cuts into three others,
  « Séance quatre », « Sur la route », « Le gué des Corbeaux », which
  `ai.analyzeBrief` appends after the session's last scene: six headings, then
  seven. Names and links of the three seeded scenes are shown (places, NPCs,
  events, media), the reorder and the added scene work: only the count and the
  titles differ.
- It cannot hold from the screen: it needs the fake `llm` brief to cut the
  mock-up's three scenes (`OralBrief.json`'s `result`) and the session s4 to
  start without them (or the apply to settle on them instead of appending).
  Both are the fixtures' and the port's, not the composition's.
- Wanted: the fixtures loader not to seed s4's scenes before the brief, and the
  fake brief to answer the mock-up's scenes; or the test to compare the
  scenes the board shows after the brief with the ones before plus the
  brief's, instead of the mock-up's list.

## Not contested: R3 and R7

They went red in a full journey run (`Test timeout of 30000ms exceeded`,
« element was detached from the DOM » on the AI settings form) and pass when
run alone on a fresh runtime. They do not touch this screen.
