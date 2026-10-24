# Contest — R7, R94, R95 (journeys of the fiche-entite screen)

Task #452, branch `feat/screen-fiche-entite`. R93 and R96 pass on the rules'
runtime (`PORTS=fake`). The three below cannot, for reasons that are the
tests' and not the screen's. Each was run for real with the test's flow
corrected, and holds (see "Checked for real").

## R7 — `tests/rules/journey/R7-dictated-prompt-assisted-creation.spec.ts`

- The test opens `?new=pnj`, fills « Prompt initial » and clicks « Proposer
  background, comportement et statblock » at once.
- The button is disabled there (`locator.click: element is not enabled`).
  The sheet is not saved, and `ai.proposeCharacter` is an `update` of an
  `Entity` that exists (`target.id: $entityId`, `data-capability.json`):
  there is nobody to propose for. EntityEditor (#451, done) states it in its
  own test: « a sheet not saved has no one to ask for ».
- Making it work as written needs a policy the SPEC does not give: saving a
  nameless sheet, or inventing a provisional name (`Entity.name` is saved by
  `campaign.saveEntity`, and the editor refuses « Donnez un nom à la
  fiche »). That is a product decision, not this screen's.
- Wanted: the test names and saves the sheet first (`Nom`, then « Enregistrer
  la fiche »), then asks for the proposal.

## R95 — `tests/rules/journey/R95-confirm-before-leaving-a-modified-sheet.spec.ts`

- Line 28: `rail.getByRole('link', { name: /Morlox/ })` resolves to two links
  (strict-mode violation): « Morlox le gobelin » (PNJ) and the event « Le
  marché noir de Morlox » (`mock/fixtures`, `campaign.listEvents`). The rail
  must list events (R93), so no rendering of the rail can make `/Morlox/`
  unique.
- The first two thirds of the flow ran before that line (« Rester » keeps
  the sheet and its change, « Abandonner » opens Gaspard).
- Wanted: `{ name: /^Morlox/ }`.

## R94 — `tests/rules/journey/R94-rail-folds-and-becomes-a-drawer.spec.ts`

- Fails `the sheet did not move when the drawer opened` 5 times out of 10
  (`--repeat-each=10`, alone, same build).
- After `page.reload()` the test reads the sheet's box at once
  (`editor.boundingBox()`, nothing awaited on the sheet itself: the
  `toBeHidden()` checks before it pass on an unrendered rail). The sheet is
  painted by three asynchronous reads (~100 ms); when it is read before it
  paints, its box is the loading skeleton's, and differs from the one
  sampled after the drawer opened. The drawer, the fold memory and the
  overlap all hold; only the two samples race.
- Wanted: wait for the sheet before the first sample, e.g.
  `await expect(editor.getByLabel('Nom')).toHaveValue('Morlox')` before
  `sheetBefore`.

## Checked for real

Rules' runtime (`bash scripts/rules-runtime.sh serve`, `PORTS=fake`), the
tests copied outside `tests/rules/` and corrected as "Wanted" says:

- R7, API then binary (Claude Code) configured on `parametres-ia`: new PNJ
  named and saved, prompt dictated, proposal asked. Both modes: a background
  sharing words with the prompt, six abilities, CA > 0, PV > 0, a « Retirer »
  ability.
- R95 whole: « Rester » keeps the change, « Abandonner » opens Gaspard and
  drops it, « Enregistrer et continuer » saves it (reopening Morlox shows
  `Seconde modification`).
- R94 passes when the sheet is awaited.

Two gaps this task found in the products of other tasks and fixed in
EntityEditor's folder, within its manifest (see the PR): a command was
parked `awaiting_review` for lack of `role:approver`, and a save sent a crop
without a portrait (`default-token` rejected it).
