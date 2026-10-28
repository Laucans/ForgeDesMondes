# BattlemapInstance — migration 0014 on a live database

## Context

Task #721. Migration `0014_battlemap_instance_uniques.sql` lays three unique
indexes on the BattlemapInstance tables: `battlemap_instance (scene_id)`,
`combat (instance_id) WHERE ended_at IS NULL`, and
`token (instance_id, character_id, number)`. The projection turns the 23505 a
second write meets into a rejected command; `warm` logs and skips an engine row
the indexes refuse.

What was run: the migration over a scratch database migrated to 0013, applied
twice; over tables already holding duplicates (it refuses, names the invariant,
deletes nothing); the projection's refusals on a `ScratchDb`; the server planted
in-process over engine rows the indexes refuse, answering `GET /health`.

## Not exercised

A real restart of the server binary on a live database that holds duplicates.
0014 refuses to apply on duplicate rows in the *tables*, so the server does not
start on such a database until the rows are repaired by hand; no test shows how
that failure reads in the server's own output, nor that a DM's real data holds
no such duplicate.

## Checklist

- [ ] Before the deploy, on the live database: no scene has two instances
      (`SELECT scene_id FROM battlemap_instance GROUP BY 1 HAVING count(*) > 1`),
      no instance has two combats with no `ended_at`, no instance has a
      character with a number twice.
- [ ] Restart the server on it: the migration applies (`0014` is recorded), the
      server answers `GET /health`.
- [ ] Restart it again: nothing is applied, nothing is logged by `warm` as
      `not projected`.
- [ ] On a copy holding one duplicate, restart: the server stops, and the error
      names `0014_battlemap_instance_uniques.sql` and the invariant. Decide
      whether stopping the server is the wanted behaviour for a live table.
- [ ] A child (a token, a combat) of an instance the engine holds and the
      tables lost, whose row the indexes refuse, makes `warm` skip the whole
      instance with its other children (they are projected together). Decide
      whether a partial projection is preferred.
