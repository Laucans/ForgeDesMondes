# Contest — R99, « Tester la connexion au fournisseur »

Task #44 (screen `parametres-ia`), milestone #17.

## The rule and its test

- Rule: **R99** — given the DM has entered their home opencode endpoint URL,
  a generation model and a summary model, when they click « Tester la
  connexion », then the screen says whether the connection succeeds or, if
  not, the reason for the failure, without saving an invalid setting behind
  their back.
- Test: `tests/rules/journey/R99-test-the-provider-connection.spec.ts`, line 25:
  `ai.getByText(/reconnu|échou|refus|invalide|injoignable/i)` must be visible
  after testing an URL that answers no known API.

## Why it cannot hold in the rules gate

The screen is right and R99 holds **on its own**; it fails when the gate runs
it **beside R3**.

- `ai.getSettings` answers one `lastTest`: the newest `last_test_*` over all
  the provider rows (`crates/ai/capabilities/get-settings/src/lib.rs`, `newest`).
  The product has one last test, not one per browser.
- `.github/workflows/gates.yml` (`rules-hold`) runs a bare `npx playwright
  test`: the journey files run on parallel workers (`fullyParallel: false`
  only serialises the tests of one file) against **one** scratch database.
  Every journey but R3 and R99 is pending, so these two start together and
  walk the same steps in lock-step.
- R99 clicks « Tester la connexion » on the unknown URL: the failure
  « type d'API non reconnu » is written on the active provider's row. Within
  milliseconds R3's second test writes « Connexion réussie (binaire Claude
  Code) » on another row, newer. The Micro-UI reads `lastTest` back, so it
  paints R3's result and R99 never sees a reason of failure.

Evidence, same build, same fixtures (`PORTS=fake`), a fresh runtime each time:

| run | R3 | R99 |
| --- | --- | --- |
| `npx playwright test R3-ai R99-test` (parallel workers) | pass | **fail** |
| the whole journey suite, `--workers=2` (the CI's two workers) | pass | **fail** |
| `npx playwright test --workers=1 R3-ai R99-test` | pass | pass |
| `npx playwright test R99-test` alone | — | pass |

The database after the parallel run holds the interleaving: the opencode row
tested at :53.58, the cloud row's failure at :54.46, the Claude Code row's
success at :54.70 — one R99 click, then R3's, inside the same second.

## What is not mine to change

- `tests/rules/` (the test, `playwright.config.ts`) and `scripts/rules-runtime.sh`
  are read-only here (`rule-tests-untouched`).
- The Micro-UI, the Capability and the DataCapability are out of this task's
  scope: « a gap found there is reported, not patched ».

## Ways out, for a human to settle

1. Run the journeys serially (`workers: 1` in `playwright.config.ts`, or
   `--workers=1` in the gate): both rules hold today, with this screen as it
   is.
2. Or a task of its own on the Micro-UI/Capability: show the outcome of the
   command the screen itself sent, instead of the newest `lastTest` of the
   aggregate, so that two sessions on one product no longer read each other's
   test.

The screen `apps/screens/parametres-ia.json` itself is complete and valid
(contract E, `compositions-resolve`, `pages-mocked`); R3 holds in every run.
