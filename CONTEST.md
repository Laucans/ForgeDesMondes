# Rule contested: R111 — Sons simultanés et volumes

**The rule.** R111 (`rules.json`): several effects play together from the
DM view, each at its own volume times the master volume, and « Tout
arrêter » stops them all. The rule itself holds and this change keeps it:
`soundbox.playSound` adds a playback beside whatever plays
(`simultaneous-playback`), from the soundbox or the pill.

**The test.** `tests/rules/business/tests/r111_simultaneous_sounds_and_volumes.rs`
creates its campaign with `new_campaign(s, "campagne")`, adds two sounds to
it, then plays them with `soundbox.playSound` — `{}` on « bruit de foule »,
`{ "source": "suggestion" }` on « porte qui grince » — and expects both to
be applied and listed as playing.

**Why it cannot hold.** A playback is always of a session, and the test's
campaign has none:

- `crates/data/migrations/0009_sound_playback.sql` (the Data layer of
  milestone #18, issue #247 — the owner's decision on #113 put the table
  there): `session_id uuid NOT NULL REFERENCES session (id)`. The
  SoundPlayback aggregate's relation `SoundPlayback.sessionId` says so too,
  and `data/model.json` documents `session_id` as not nullable;
- `campaign.createCampaign` (`data-capability.json`, milestone #20): only
  « a oneshot comes with its single session ». A `campagne` is created with
  no session at all, and the test creates none.

So `soundbox.playSound` finds no session to play the sound in, and refuses
it before it is queued, 422 « la campagne de ce son n'a aucune séance »: the
test fails at « soundbox.playSound was not applied ». Writing the playback
anyway would need a `session_id` the table cannot hold empty, a session
made up by a command that does not own the Session aggregate, or a schema
change in another milestone's Data layer.

**What would make it hold.** The test should play its sounds in a session:
`new_campaign(s, "oneshot")` — whose single session `campaign.createCampaign`
creates — or a session created for the campaign before the sounds play.
The other way, making `sound_playback.session_id` nullable as #113 first
planned, is a schema decision of milestone #18's.

**Note, not contested here.** The test sends no `caller`.
`SoundPlayback.sessionId` and `SoundPlayback.soundId` are `critical`, so a
playback from a caller without `role:approver` waits in `awaiting_review`,
as `campaign.createCampaign` (#273) and `session.closeSession` (#213)
already noted for theirs: it is the engine's rule for every critical field,
for the shell and the rules' helper to settle together.

Contested by the PR for issue #134 (`soundbox.playSound`).
