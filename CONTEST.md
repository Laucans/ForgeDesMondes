# Contested rule tests

## R106 — Fiche d'une instance précise en combat

Test: `tests/rules/journey/R106-sheet-of-a-precise-instance-in-combat.spec.ts`

The test cannot hold as written, whatever the Micro-UI does.

After the DM has started the combat it opens the sheet of « Bandit 2 », then checks the two other instances:

```ts
for (const other of ['Bandit 1', 'Bandit 3']) {
  await carousel.locator('.kit-token__item').filter({ hasText: other }).click();
```

Playwright's `hasText` with a string is a case-insensitive **substring** match, and `click()` is strict: it fails when the locator resolves to more than one element. « Bandit 1 » is a substring of « Chef bandit 1 », the hidden boss of the same scene (fixture `mock/fixtures/table.getBattlemapInstance.json`, character `vesna`, token `t-vesna-1`). That boss is in the DM carousel by rule: R107 and the business rule `hidden-keeps-rank` require a hidden token to keep its rank, marked with an icon, in the DM's carousel (« figure avec une icône dans le carrousel DM seulement »). So the DM carousel holds two items whose text contains « Bandit 1 », and the click throws a strict-mode violation:

```
locator('…').filter({ hasText: 'Bandit 1' }) resolved to 2 elements:
  1) <div … data-token="t-vesna-1" …>  Chef bandit 1
  2) <div … data-token="t-bandit-1" …> Bandit 1
```

It was reproduced against the real Micro-UI bundle on the mock-up's `dm-view` page in Chromium; the same walk with the exact item (`#carousel-t-bandit-1`) passes: the sheet opens on that instance and shows `11 / 11 PV`, and Bandit 2's HP stay at `4 / 11`.

The rule itself holds (one instance's HP change touches that instance only: `hp-change-one-token`, covered by the Micro-UI's tests and R21/R23's business tests). Only the locator is ambiguous: it needs an exact match (`{ hasText: new RegExp(`^${other}$`) }` on the name, or the item's `data-token`). Neither the fixture's names (R107 names « Chef bandit 1 ») nor the DM carousel can change to satisfy it.

Left untouched, as the rules require: `tests/rules/` and `rules.json`.
