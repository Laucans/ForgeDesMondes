# Contest — R111, made active by the soundbox screen

Task #143 (the frozen `soundbox` screen), milestone #21.

The screen was the last thing R111's test waited for, so it is no longer
pending. Against the rules' runtime it is red, and the reason is in the test's
shared helper, not in the product.

## R111 — « Sons simultanés et volumes »

- Test: `tests/rules/business/tests/r111_simultaneous_sounds_and_volumes.rs`.
  After each `soundbox.addSound` it reads `soundbox.listSounds` and finds the
  new sound with `by_name_or_title(&sounds, &name)`.
- It fails with « the sound is not listed » on the first sound.
- Why: `by_name_or_title` (`tests/rules/business/tests/common/mod.rs`) compares
  the `title` field only, whatever its name suggests. It was written for
  sessions and scenes, which have a `title`. A sound has a `name`, not a
  `title`: `soundbox.listSounds` answers `{ id, name, tags, volume, file, gain,
  playing }` (its `capability.json`, `mock/fixtures/soundbox.listSounds.json`,
  and both Micro-UIs read `name`). The sound is added (`applied`) and is listed
  under its name; the helper never looks at it.
- Checked on the runtime's database: with the helper temporarily widened to
  `text(v, "title") == title || text(v, "name") == title` (change reverted,
  not part of this pull request), R111 passes from end to end — both sounds
  overlap (`playing` true), the master volume 80 → 40 lowers both gains and
  keeps their ratio, and `soundbox.stopAll` leaves nothing playing. The
  product meets the rule's criterion.
- Why it is not met here: giving `listSounds` a `title` next to `name` would
  change a Capability's contract to fit a helper (this task changes no
  Capability, and `name` is what the manifest, the fixture and the Micro-UIs
  say), and the helper is the rules' task's (`rule-tests-untouched`).
- Wanted from the rules' task: the helper matches `name` as well as `title`
  (its name says so). `r16_tagged_sound_effects.rs` uses the same lookup and
  will hit the same wall as soon as it is no longer pending.

The frozen screen itself is not in question: `/screens/soundbox` replays
`SoundLibrary` and `LiveSoundbox` on the shell.
