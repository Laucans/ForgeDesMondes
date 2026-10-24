# Contest — R103 (Affiner une entité manquante)

**Rule:** R103, `tests/rules/journey/R103-refine-a-missing-entity.spec.ts`.

**What holds.** With `apps/screens/campagne.json` frozen, the journey runs on the real
runtime (`PORTS=fake`, server + shell) and every step of the rule passes: the DM opens
`campagne`, « Affiner » on *Frère Odric*, dictates the three sentences, « Proposer »; the
proposal holds words of the brief excerpt *and* of the added sentences (the two
`words(...).some(...)` assertions pass), « Background » is shown, and the entity is « Créé »
and the « à créer » count drops by one.

**What cannot hold.** Line 32:

```ts
await expect(item.getByText('CA')).toBeVisible();
```

`getByText('CA')` is a case-insensitive *substring* match, and Playwright's strict mode
rejects a locator that resolves to several elements. Inside Odric's item it resolves to
three:

1. `<p class="small muted">L’embuscade du Gué</p>` — the scene label (embus**ca**de), from
   `mock/fixtures/ToCreateList.json`;
2. `<dd>Odric a vu les cavaliers de Vesna verser l’huile …</dd>` — the proposal's
   background (**ca**valiers), the very text the rule requires to carry the brief excerpt;
3. `<strong>CA</strong>` — the armour class of the statblock, the only one meant.

So the assertion fails with a strict-mode violation, not because the statblock is missing.
Neither of the two other matches can be removed without breaking the rule itself: the scene
label is the fixture's data, and the background has to hold the excerpt's words.

**Proof it is the locator.** The same spec, with that one line written
`item.getByText('CA', { exact: true })`, passes end to end against this very branch on a
fresh runtime (`1 passed`). The copy was run once and deleted; the protected file is untouched.

**Request.** Settle the contest by changing line 32 of the rule's test to an exact match
(`getByText('CA', { exact: true })`), in a `harness:rules` task. Nothing in the screen, in
the Micro-UIs or in the fixtures needs to change.
