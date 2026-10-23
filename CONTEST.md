# Contest — R1 and R97, made active by `session.createSession`

Task #157 (DataCapability `session.createSession`), milestone #20.

`session.createSession` is the last unit both tests wait for, so neither is
pending any more. Against the rules' runtime both are red, and neither
reason is inside this unit. The command itself works through the server:
`new_session` gets `applied`, and `session.listSessions` lists the session in
`prep` at the campaign's next position. Its PostgreSQL tests are in
`crates/session/data-capabilities/create-session/tests/postgres.rs`.

## R1 — « Plusieurs campagnes et oneshots »

- Test: `tests/rules/business/tests/r1_several_campaigns_and_oneshots.rs`.
  It needs `capability:campaign.listCampaigns`, `aggregate:Session`,
  `capability:session.listSessions` and `data-capability:session.createSession`.
  It requires `sessions == 1` for the oneshot `nuit-carnaval` in
  `campaign.listCampaigns`'s answer.
- It fails with « the oneshot nuit-carnaval does not have exactly one
  session », for the two reasons #263 gave, both still true:
  1. **No session is counted.** `campaign.listCampaigns` answers `sessions`
     from `campaign_overview_view.session_count`
     (`crates/campaign/capabilities/list-campaigns/src/lib.rs`). The view has
     only `campaign_id` and `entity_counts` (checked on the runtime's
     database), so the answer is `null` for every campaign. Fixing this means
     a migration, `data/model.json`, and the `list-campaigns` Capability
     (milestone #18's contract). None of that is this DataCapability's.
  2. **No session reaches the database.** On the runtime's database,
     `nuit-carnaval` holds 0 rows in `session`: `crates/server/src/fixtures.rs`
     does not seed the fixtures' sessions. That is the server's plumbing, not
     this unit.

## R97 — « Médias rattachés à la séance par leur lieu »

- Test: `tests/rules/business/tests/r97_media_attached_to_the_session_by_place.rs`.
  It requires each medium of `media.listMedia` to carry `sessions`, the
  sessions it is attached to through its place.
- It fails with « the media of the tavern are not attached to the
  session », after `new_session` and `new_scene` both succeed.
  `media.listMedia` (`crates/media/capabilities/list-media`,
  `crates/media/queries/listMedia.graphql`) answers no `sessions` field and
  does not read `session_media_view`. Reproduced by hand against the runtime:
  - the session created by this command is listed;
  - the scene linking the tavern is applied;
  - the tavern's battlemap is listed with `place` set and no `sessions`.

  The view holds the association: the Session aggregate's tests show
  `session_media_view` attaching the tavern's media
  (`crates/session/data-capabilities/add-scene/tests/postgres.rs`). The read
  that should expose it is `media.listMedia` (milestone #19's contract, state
  `evolving`). #168 is named as the slice that turns R97 green, but its scope
  is a screen composition only.

## What is not mine to change

`tests/rules/`, `rules.json`, `scripts/rules-runtime.sh`,
`campaign.listCampaigns` and its view, the server's fixtures, and
`media.listMedia`. Either of these lets both rules hold, or wait without
holding this unit back:

- R1 waits for the units that count and seed sessions, and R97 for the
  `media.listMedia` version that answers `sessions`;
- or those units land first.
