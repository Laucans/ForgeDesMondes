# Contest — R7 against R99 (two journeys, one runtime)

Task #452, branch `feat/screen-fiche-entite`. R7, R93, R94, R95 and R96 pass
on the rules' runtime (`bash scripts/rules-runtime.sh serve`, `PORTS=fake`).
R99 (`tests/rules/journey/R99-test-the-provider-connection.spec.ts`, not
this task's) is green until R7 can run, then red: this is what turned
milestone/22 red when #487 merged, and what the settled contest of #488 did
not cover.

## What happens

- R7 ends with the binary provider configured and saved: `ai.saveSettings`
  leaves `ai_settings.active_provider_id` on « Binaire Claude Code »
  (`a1000000-…-0004`). The runtime has one database for every test.
- R99 opens `parametres-ia`, checks « API », fills the form, clicks « Tester
  la connexion » twice and never saves: its commands are `ai.testConnection`
  only (an `update` of the last test), none `ai.saveSettings`. It then
  reloads and expects `getByLabel('URL de l’API')` not to hold the bad URL.
- After the reload the page shows the saved provider, the binary form, which
  has no « URL de l’API » field: `element(s) not found` at line 28.

## Evidence

- Whole journey suite, fresh runtime each time, three times: R7, R93–R96 pass
  and R99 fails, three times out of three.
- R99 alone, fresh runtime: passes. R99 then R7: R7 passes.
- R7 then R99: passed three times and then failed on the next fresh runtime,
  so the order alone does not decide it. The saved provider is the binary in
  every case (queried after each run); a pass is likely the assertion landing
  on the form's first paint, before the stored settings replace it. Not
  verified.

## Why it cannot hold

The screen is right: a reload shows what is saved. R7 requires the binary to
be saved (« configured »), R99 requires an API form after a reload without
saving anything. Both cannot hold in one database, and nothing in the unit
this task builds, or in the three Micro-UIs, can change what AiSettings shows
for a saved binary provider.

## Wanted

One of, with no assertion of either rule weakened:

- R7 puts the API provider back (« API », « Enregistrer ») as its last step;
- R99 sets its own precondition: it saves an API provider before testing;
- or the runtime isolates each spec file's data.
