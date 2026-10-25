# Rules contested by #465 (freeze dm-view)

`apps/screens/dm-view.json` is frozen as the SPEC says (LiveSessionBar, ElementsPanel, TableSheet,
`Battlemap {instance: $ctx.battlemap, view: dm}`, LiveSoundbox, SuggestionPill) and the shell lays it out
(`apps/shell/src/server/frames.ts`). The screen mounts, the three tabs `Fiche` / `Battlemap` / `Soundbox` and the fixed
pill are in place. The ten rules below are still red on the real product. None is mis-mounted (`rules-gate status`
says `red`, not `mis-mounted`), and none can be fixed inside a composition: a Micro-UI, the server's fixtures or
the context a Micro-UI sets would have to change, which this task may not touch.

Probed on the runtime's server (`RULES_SERVER_URL`), `POST /capabilities/<id>`:

| read | variables | answer |
| --- | --- | --- |
| `session.getLiveSession` | none | session `s4`, no scenes |
| `session.listScenes` | none / `{campaign}` | `[]` |
| `session.listScenes` | `{session: "s4"}` | the scenes `sc1…` |
| `live.listSuggestions` | none / `{session: "s4"}` | `{history: [], suggestions: []}` |
| `live.listSuggestions` | `{campaign: "cendres-valombre"}` | the history |
| `soundbox.listSounds` | none | `null` |
| `soundbox.listSounds` | `{campaign: "cendres-valombre"}` | `{masterVolume: 100, sounds: []}` |
| `campaign.getCampaign` | none | `null` |
| `campaign.getCampaign` | `{id: "cendres-valombre"}` | the campaign |
| `table.getBattlemapInstance` | `{id: "bmi-gue-s4"}` | `null` |

## R17 — Conduite de la séance depuis la DM view

Test: `tests/rules/journey/R17-running-the-session-from-the-dm-view.spec.ts:21`,
`expect(panel.locator('details[data-scene-index]')).toHaveCount(3)`, received 0.
Why it cannot hold: ElementsPanel reads the `session` and `campaign` names of the screen's context
(`apps/live/elements-panel/src/index.ts:94-95`); nothing in dm-view sets them — LiveSessionBar sets only `scene`
(`apps/session/live-session-bar/src/index.ts:81`). Without `session`, `session.listScenes` answers `[]`.
Missing piece: a Micro-UI (LiveSessionBar) setting the `session` and `campaign` context from `getLiveSession`, or a
`session`/`campaign` prop the screen can freeze; a Micro-UI change, not a composition's.

## R24 — Diffusion vers la player view

Test: `R24-broadcast-to-the-player-view.spec.ts:17`, PlayerStage's `h1` (the cover) is not found.
Why it cannot hold: `player-view` composes PlayerStage with no `campaign`, so `campaign.getCampaign` is asked with
no id and answers `null`: no cover. The DM side (R17's `session`/`campaign` context) is missing too, so the click on the
illustration in ElementsPanel cannot be reached. Missing piece: the campaign of the live session reaching
PlayerStage and ElementsPanel (same as R17).

## R98 and R109 — Pastille des suggestions

Tests: `R98-suggestions-pill-as-a-fixed-overlay.spec.ts:16`, `R109-suggestions-history.spec.ts:30`.
Why they cannot hold: SuggestionPill asks `live.listSuggestions` with its `campaign` prop; the screen gives none (the
SPEC's props are `{}`) and the real server answers an empty feed without it, so no entity ever reaches the pill. The
overlay itself is `position: fixed` bottom right (`apps/live/suggestion-pill/src/style.ts`), outside the centre, which
takes no room for it (`apps/shell/src/server/page.ts`). Missing piece: the campaign id reaching the pill (a context
LiveSessionBar sets, as R17).

## R16 and R110 — Soundbox

Tests: `R16-tagged-sound-effects.spec.ts:15`, `R110-soundbox-search-and-tags.spec.ts:15` (business tests `r16_…`
pass; the journey does not).
Why they cannot hold: both start from the sound library's `Modifier` button on a fixture sound; the real server
answers `soundbox.listSounds` with `sounds: []` and, without a campaign, `null` — the sounds fixtures
(`sounds.json`, `soundbox.listSounds.json`) are not loaded by `crates/server`'s fixtures loader. LiveSoundbox also
reads the `campaign` context. Missing piece: the server loading the sound fixtures (infrastructure) and the
`campaign` context.

## R106, R19, R23, R113 — Battlemap in the DM view

Tests: `R106-…:14` (`Lancer le combat` not found), `R19-…:18` (token), `R23-…:16` (token), `R113-…:14`
(`Envoyer aux joueurs`).
Why they cannot hold: the SPEC has the Battlemap on `$ctx.battlemap`, which only PlayerStage sets
(`apps/live/player-stage/src/index.ts:151`, for the player view); on the DM view nothing sets it, so the Battlemap
gets no instance and draws neither tokens nor combat controls. Probed with the mock-up's literal `bmi-gue-s4` in
its place (not committed): `table.getBattlemapInstance` answers `null` on the real server (the battlemap instance
fixture is not loaded), the carousel `Ordre d’initiative` stays hidden (R106, R23: the `## Decision` of #465 records that the PCs'
`pjInitiative` is not exposed, so a combat cannot start — read there, not re-probed here), R19's counter/token never shows within its 1 s, and R113's pilot tab finds no
`Elowen` token. The `## Decision` of #465 also records that at 1280×720 the fixed pill (R98 forbids moving it or shifting the
content) covers Elowen's token (not reproduced here, the map not drawing) — R19/R23 will need the test's viewport or token chosen away from the bottom right.
Missing piece: something setting `battlemap` from the live scene on the DM view, the battlemap instance fixture
loaded by the server, `pjInitiative` exposed.
