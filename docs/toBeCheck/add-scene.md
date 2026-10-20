# session.addScene — to check in real conditions

## Context

PR #284 (`feat/add-scene`) adds `session.addScene`. The repair merged
`milestone/20-…` (#289, `session.addPlaceToSession`) into it:
`scene-links-same-campaign` for a new scene's places is now judged by
#289's `ScenePlaces`, its verdict computed with `Rows::place_facts`
(`Rows::judge_scene`), kept in `Links` under the scene's id, and judged
again at a start (`Rows::rejudge_links`). `Scenes` keeps only each
session's campaign and each scene's session and rank.

A second merge brought #287 (`session.reorderScenes`), which had its own
`Scenes`: the two are one index now — `record`, `campaign_of`,
`next_position` for `addScene`, `take`, `orders` for the reorder; a
session's tombstone forgets its scenes too. At a start, `REORDER_SCENES`
runs after the scenes projected again.

The PostgreSQL tests (`tests/add_scene_projection.rs`) were written without
a local database (Docker down): only the CI's `rust` job ran them.

## To verify

- [ ] Over the real database, add 2 scenes to a session holding none: ranks
      1 and 2 in `scene`; a third, with a place of the campaign: rank 3, one
      `scene_entity` row at position 1, its media in `session_media_view`.
- [ ] A place of another campaign: the command is rejected with
      `scene-links-same-campaign`, nothing reaches `scene` nor `scene_entity`.
- [ ] Restart the server with an `addScene` queued: it is applied as judged
      (a foreign place still rejected).
- [ ] `session.addPlaceToSession` on a scene `addScene` created: over the
      tables it ranks after the scene's links. Over the engine's memory it
      does not read the scene row's `entityId` list, so its rank can repeat
      one of them.
- [ ] After a reorder (#287), `addScene`: the new scene is ranked n+1 in
      `scene`, the others keep the reorder's ranks; a second reorder must
      list it (`scene-order-contiguous`), in memory and over the tables.
- [ ] Restart after a reorder whose scenes the tables lost: they are
      projected again with their first rank, then `REORDER_SCENES` puts back
      the order's ranks.

## Out of scope here (follow-ups)

- [ ] A oneshot's session is written on `Campaign/<id>` (#279): `addScene`
      knows it through `Versioning`; check once #279 lands that both agree.
- [ ] Rewriting a scene (#161, #162): `scene_entity` links are insert-only,
      so a dropped link and a stale rank stay, and `session_media_view`
      keeps the media of a removed place. `UPSERT_SCENE` also writes the
      scene row's own rank on each write of it, so a later write of a scene
      after a reorder would put its first rank back (#287's risk note).
- [ ] A deleted campaign or session: `Scenes` forgets only the partition
      tombstoned, so a session of a deleted campaign still takes scenes.
