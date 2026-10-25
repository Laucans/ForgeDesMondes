# Contest — R18, the one planned session the fixtures had is now the live one

Task #600, « Server fixtures: the live session s4 … on the real server ».

## The rule and its test

R18 « Suggestions du micro en direct »,
`tests/rules/business/tests/r18_live_microphone_suggestions.rs`. After
#557 (« Rules contest settled »), it lists the fixture campaign's sessions
(`session.listSessions { campaign: "cendres-valombre" }`), takes the one whose
`status` is `prep`, and sends `live.setMicrophone { on: true }` to it:

```rust
.find(|x| text(x, "status") == "prep")
.ok_or("the fixture campaign has no planned session")?;
```

## Why it cannot hold

The task's AC1 loads the session `s4` as the running one (status `live`, a
start time): `session.getLiveSession` with no id must answer it. `s4` is the
only session of `mock/fixtures/SessionList.json` that is `prep`; `s1` to `s3`
are `played`. Once `s4` is live the fixture campaign has no planned session,
and the test stops on `R18 does not hold: the fixture campaign has no planned
session` before it sends a command. Nothing of the rule itself is in doubt:
the criterion (the microphone on, the PNJ named by its alias in the stack of
suggestions, no audio out) does not say which session the microphone is in.

Run against a fresh runtime with this task's loader (`--no-fail-fast`): R18 is
the only red test the change causes. (R4 is red when the suite runs twice on
the same store, an earlier rule having edited Morlox's notes; on a fresh
store it holds, with or without this change.)

## What would make it hold

Select the session the microphone listens in as the campaign's live one, else
its planned one — the order `live.setMicrophone`'s manifest gives: "the
command's target, else the campaign's live session, else its last one
planned". For example, in the test:

```rust
.find(|x| text(x, "status") == "live")
.or_else(|| sessions.iter().find(|x| text(x, "status") == "prep"))
```

The assertions stay as they are. Adding a fifth, planned session to
`mock/fixtures/SessionList.json` would also satisfy the test, but it changes
the mock-up's session list, which this task (infrastructure, the loader) does
not cover.
