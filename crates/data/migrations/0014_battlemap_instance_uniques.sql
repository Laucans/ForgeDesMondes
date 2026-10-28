-- The BattlemapInstance aggregate's invariants that read several rows at
-- once (aggregates/BattlemapInstance/aggregate.json), laid on the tables of
-- 0010 as unique indexes. The DataGuard's judge sees one row at a time, so
-- two commands that interleave could each pass it and write a scene's second
-- instance, an instance's second running combat, or a second token numbered
-- alike; PostgreSQL refuses the second whatever the order. The refusal is
-- SQLSTATE 23505: the projection (class 23) rejects the command and hands it
-- back to its author, the store never fails (rule 8).
-- Replays as a no-op, as 0001 to 0013 do: the rules' runtime applies it with
-- psql, then the server through the Data layer. Each index is `IF NOT EXISTS`.
--
-- It refuses to apply on rows that already break an index, and says which:
-- an index that is not there is a guarantee nobody has, and a row is never
-- deleted for it — the DM's data is theirs to repair. Rows only the engine
-- holds are another matter: the projection's `warm` logs the partition the
-- indexes refuse and goes on, so the server still starts.

DO $$
DECLARE
    broken text;
BEGIN
    SELECT string_agg(scene_id::text, ', ') INTO broken FROM (
        SELECT scene_id FROM battlemap_instance GROUP BY scene_id HAVING count(*) > 1
    ) duplicated;
    IF broken IS NOT NULL THEN
        RAISE EXCEPTION 'one-instance-per-scene: scenes with several instances: %', broken
            USING ERRCODE = 'unique_violation', CONSTRAINT = 'battlemap_instance_scene_id_unique', HINT = 'Repair the rows named, as their owner decides, then migrate again: nothing was deleted.';
    END IF;

    SELECT string_agg(instance_id::text, ', ') INTO broken FROM (
        SELECT instance_id FROM combat WHERE ended_at IS NULL
            GROUP BY instance_id HAVING count(*) > 1
    ) duplicated;
    IF broken IS NOT NULL THEN
        RAISE EXCEPTION 'one-active-combat: instances with several combats running: %', broken
            USING ERRCODE = 'unique_violation', CONSTRAINT = 'combat_one_active', HINT = 'Repair the rows named, as their owner decides, then migrate again: nothing was deleted.';
    END IF;

    SELECT string_agg(instance_id::text || '/' || character_id::text || '/' || number, ', ')
        INTO broken FROM (
        SELECT instance_id, character_id, number FROM token WHERE number IS NOT NULL
            GROUP BY instance_id, character_id, number HAVING count(*) > 1
    ) duplicated;
    IF broken IS NOT NULL THEN
        RAISE EXCEPTION 'multi-drop-numbering: tokens sharing a number (instance/character/number): %', broken
            USING ERRCODE = 'unique_violation', CONSTRAINT = 'token_instance_character_number', HINT = 'Repair the rows named, as their owner decides, then migrate again: nothing was deleted.';
    END IF;
END
$$;

-- Invariant one-instance-per-scene: a scene has at most one battlemap
-- instance.
CREATE UNIQUE INDEX IF NOT EXISTS battlemap_instance_scene_id_unique
    ON battlemap_instance (scene_id);

-- Invariant one-active-combat: an instance has at most one combat with no
-- ended_at. An ended combat is history: any number of them.
CREATE UNIQUE INDEX IF NOT EXISTS combat_one_active
    ON combat (instance_id) WHERE ended_at IS NULL;

-- Invariants multi-drop-numbering and token-roster (its NPC side): a number
-- is taken once per character on an instance. A PC's token carries no number
-- (NULL, never equal to another), so its one-token-per-instance stays the
-- DataGuard's to judge.
CREATE UNIQUE INDEX IF NOT EXISTS token_instance_character_number
    ON token (instance_id, character_id, number);
