# Contest — R112 and R18, one unscoped `live.setMicrophone`

Pull request #556, `live.setMicrophone` (milestone 27). R112, R18, R26, R6
and R77 were pending on `live.setMicrophone`; they first run with this PR.
R112 fails `rules-hold`. CI stops at the first red binary; run locally with
`--no-fail-fast` against the rules' runtime on a fresh database, R112 and R18
are red, R26, R6 and R77 hold.

## The two tests want opposite sessions from the same command

Both send `live.setMicrophone` with `{ "on": true }`: no target, no campaign.
The manifest says the session is "the command's target, else the campaign's
live session, else its last one planned", and the payload has no campaign. So
an unscoped command can only mean one session across every campaign of the
store, and the two tests need two different ones.

### R112 « Fenêtre d'illustration proportionnelle »

- Test: `tests/rules/business/tests/r112_proportional_illustration_window.rs`.
- It makes a new campaign and a session in it (`new_session`), sends
  `{ "on": true }`, then `live.showCover { campaign }` for that new campaign.
- It fails with `commanding live.showCover: …/commands: status code 422`
  (`NoLiveSession`): the microphone went on in the fixture campaign's
  session (`Les Cendres de Valombre`, position 4), not in the session the
  test just made, so the new campaign has no live session.
- Reproduced alone on a fresh database too.
- Wanted: pass the session it made as the command's target:
  `apply(s, "live.setMicrophone", json!({ "on": true }), Some(&session))`
  (`new_session` already returns its id).

### R18 « Suggestions du micro en direct »

- Test: `tests/rules/business/tests/r18_live_microphone_suggestions.rs`.
- It sends `{ "on": true }` and expects `live.listSuggestions { campaign:
  "cendres-valombre" }` to show Vesna within 5 s.
- Alone on a fresh database it holds (the pick lands on the fixture
  campaign's prep session). In the whole suite it fails with `the entity
  named by its alias did not appear within a few seconds`: an earlier rule
  left a live session in a test campaign, and the unscoped command picks it
  (a live session first), so the stand-in transcription listens with another
  campaign's vocabulary.
- Wanted: the same as R112, a target. List the fixture campaign's sessions
  (`session.listSessions { campaign }`) and send the planned one as target.

## Why the product does not settle it

A pick that makes R112 hold (the newest campaign, the newest session) sends
R18 and R26 elsewhere than the fixture campaign, and the reverse (the oldest
campaign, the fixture one) makes R112 fail. By reading, not by a run: they
cannot hold together on the same unscoped command, whatever the pick; the
test must say which session it means. The data model has no creation time on
`session` to order them by either.

## Checked for real

- Rules' runtime on this branch (`c35fbc0`), fresh database each time:
  R112 alone red; the suite with `--skip r112` red on R18 only.
- R26, R6 and R77 hold with the pick as it is.

## What is not mine to change

`tests/rules/`, `rules.json`, `scripts/rules-runtime.sh`. No code of the unit
changed for this contest.
