# Contest — R26 and R77, an unscoped `live.setMicrophone` again

Milestone 28 caught up with `main_agent` by hand (1a33d25), which brought
milestone 27's `live.setMicrophone`. R26 was pending on
`session.closeSession` on one side and on `live.setMicrophone` on the other:
it runs for the first time on `milestone/28`, and so does R77 behind it (CI's
`cargo test` stops at the first red binary; run locally with
`--no-fail-fast`). This pull request fixes the product side (the close lost to
the microphone's projection, and no phrase heard was ever written to the
journal); R26 and R77 then stay red only on the session the tests turn the
microphone on in. Contest #557 settled the same thing for R18 and R112.

## Both tests turn the microphone on in a session other than the one they close

Both send `live.setMicrophone` with `{ "on": true }` and no target. The
manifest says the session is "the command's target, else the campaign's live
session, else its last one planned", and the payload has no campaign: the
fixtures' live session (« L'embuscade du Gué ») is picked, never the session
the test made.

### R26 « Clôture de séance avec journal et résumé »

- Test: `tests/rules/business/tests/r26_closing_with_journal_and_summary.rs`.
- It makes a session (`new_session`, in prep), sends `{ "on": true }`, then
  waits for **that** session's journal. The microphone listens in the
  fixtures' live session, whose journal fills (16 lines in 10 s, checked in
  the runtime's database); the new session's stays empty: `the journal stayed
  empty while the microphone ran`. Its `session.closeSession` would then be
  refused anyway: a session in prep is not closed (`status-lifecycle`).
- Wanted: pass the session it made as the command's target:
  `apply(s, "live.setMicrophone", json!({ "on": true }), Some(&session))`.

### R77 « Seul du texte part vers le fournisseur »

- Test: `tests/rules/business/tests/r77_only_text_goes_to_the_provider.rs`.
- In each mode it sends `{ "on": true }` before making its session, then
  closes the session it made: `commanding session.closeSession: …/commands:
  status code 422` — still in prep, the microphone went on elsewhere.
- Wanted: make the session first, then turn the microphone on and off in it:
  `let session = new_session(s, &campaign, &[])?;` before
  `apply(s, "live.setMicrophone", json!({ "on": true }), Some(&session))`, and
  `Some(&session)` on the `{ "on": false }` too.

## Checked for real

- Rules' runtime on this branch, fresh database: the whole business suite
  with `--no-fail-fast` is green but R26 and R77.
- The same two tests with only the change asked above (copies, not
  committed): R26 green (journal filled, microphone paused at the close,
  summary written with the configured model), R77 green in both modes.

## What is not mine to change

`tests/rules/`, `rules.json`, `scripts/rules-runtime.sh`. The pick of an
unscoped `live.setMicrophone` stays milestone 27's (contest #557).
