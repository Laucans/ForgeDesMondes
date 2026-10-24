# ai.createFromBrief — to check in a real run

## Context

PR #496 (`feat/ai-create-from-brief`) adds the DataCapability
`ai.createFromBrief`. Its tests run the engine over memory. Nothing was run
against the real server, PostgreSQL or a real provider.

## What shipped

- The creation accepts what the ToCreateList sends: the entry in the
  payload, the entry's session as the target.
- A double click makes one element: the entry stays held for up to 60 s
  after its creation is handed to the engine, until its stamp is queued.
- The scene link is only the creation's own follow-up, once.

## Known gap: the list keeps showing « à créer »

No projection writes `entity_to_create`. The Session aggregate projects
absences, sessions, scenes, scene order and presences only
(`crates/session/aggregates/Session/src/lib.rs`, `plugin()`). The stamp
(`entityToCreateStatus = creee`, `createdEntityId`, `refinementText`) lives
only in `dataguard_rows`. `campaign.listEntitiesToCreate` reads the table,
so the list keeps showing the entry « à créer » until the Session projection
covers `entity_to_create`. That is the Session aggregate's task, not this
DataCapability's.

## Review flow

`entityToCreateStatus` (Session) and `type` (Entity) are critical fields.
The ToCreateList sends `roles: []`, so the stamp, the save and the link all
wait on a review (`awaiting_review`) until an approver decides it.

## Checklist

- [ ] From the real ToCreateList, « Créer depuis le brief » on an entry gets
      no 422: the command is queued on `Session/<entry id>` with two
      follow-ups, `Entity/<element id>` and `Session/<scene id>`.
- [ ] Once the reviews are approved: one `entity` row, one `entity_version`
      row (version 1), and for an NPC one `character` row.
- [ ] A `scene_entity` row for the element at the scene's next position.
- [ ] A fast double click: one element, one provider call in the AI
      request journal. The second click answers « l'entrée est déjà en
      cours de création ».
- [ ] After the Session projection covers `entity_to_create`: the row
      reads `creee` and the entry leaves the list.
