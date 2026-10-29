-- The BattlemapInstance aggregate's token tells a PC from an NPC
-- (aggregates/BattlemapInstance/aggregate.json, field characterType): the
-- type of its character's element (entity.type), copied onto the token so
-- that the tables themselves judge what the DataGuard's judge, which sees one
-- row at a time, cannot — however two writes interleave.
--   - pj-hp-not-tracked: a PC's token tracks no HP (hp_current and hp_max
--     empty); an NPC's tracks both, 0 <= hp_current <= hp_max;
--   - token-roster (its PC side): a PC has one token per instance. An NPC may
--     have several, told apart by their number (0014).
-- A refusal is SQLSTATE 23514 (check) or 23505 (unique): the projection
-- (class 23) rejects the command and hands it back to its author, the store
-- never fails (rule 8).
-- Replays as a no-op, as 0001 to 0014 do: the rules' runtime applies it with
-- psql, then the server through the Data layer. The column is `IF NOT
-- EXISTS`, the functions `OR REPLACE`, the trigger and the constraints are
-- dropped and added again, the index is `IF NOT EXISTS`.
--
-- A token that does not name its marker gets its character's type, whoever
-- writes it (the projection, a fixture's SQL), so that the writers which
-- know nothing of the marker yet stay valid.
--
-- It refuses to apply on rows that already break a rule, and says how many
-- and which: a constraint that is not there is a guarantee nobody has, and a
-- row is never deleted for it — the DM's data is theirs to repair. Rows only
-- the engine holds are another matter: the projection's `warm` logs the
-- partition the tables refuse and goes on, so the server still starts.

ALTER TABLE token ADD COLUMN IF NOT EXISTS character_type text;

-- The marker a token without one gets: its character's type. One that names
-- a marker names its character's: a token cannot call a PC an NPC to track
-- its HP, or the other way round.
CREATE OR REPLACE FUNCTION token_character_type_default() RETURNS trigger
    LANGUAGE plpgsql AS $$
DECLARE
    kind text;
BEGIN
    SELECT e.type INTO kind FROM entity e WHERE e.id = NEW.character_id;
    IF NEW.character_type IS NULL THEN
        NEW.character_type := kind;
    ELSIF kind IS NOT NULL AND NEW.character_type <> kind THEN
        RAISE EXCEPTION 'pj-hp-not-tracked: the token of a % is marked %', kind, NEW.character_type
            USING ERRCODE = 'check_violation', CONSTRAINT = 'token_character_type_matches';
    END IF;
    RETURN NEW;
END
$$;

DROP TRIGGER IF EXISTS token_character_type_default ON token;
CREATE TRIGGER token_character_type_default
    BEFORE INSERT OR UPDATE OF character_id, character_type ON token
    FOR EACH ROW EXECUTE FUNCTION token_character_type_default();

-- Existing tokens take their character's type.
UPDATE token t SET character_type = e.type
    FROM entity e
    WHERE e.id = t.character_id AND t.character_type IS NULL;

-- Invariant pj-hp-not-tracked, on one row: whether a token of `character_type`
-- tracks its HP as it should. Always true or false, never null, so that a
-- missing type or missing HP is refused, not let through.
CREATE OR REPLACE FUNCTION token_hp_tracked(
    character_type text, hp_current smallint, hp_max smallint
) RETURNS boolean
    LANGUAGE sql IMMUTABLE AS $$
    SELECT CASE character_type
        WHEN 'pj' THEN hp_current IS NULL AND hp_max IS NULL
        WHEN 'pnj' THEN hp_current IS NOT NULL AND hp_max IS NOT NULL
            AND hp_current >= 0 AND hp_current <= hp_max
        ELSE false
    END
$$;

DO $$
DECLARE
    broken text;
    total integer;
BEGIN
    SELECT count(*), string_agg(id::text, ', ') INTO total, broken
        FROM token WHERE NOT token_hp_tracked(character_type, hp_current, hp_max);
    IF total > 0 THEN
        RAISE EXCEPTION 'pj-hp-not-tracked: % token(s) break it (a PC with HP, an NPC without or beyond its maximum): %', total, broken
            USING ERRCODE = 'check_violation', CONSTRAINT = 'token_pj_hp_not_tracked', HINT = 'Repair the rows named, as their owner decides, then migrate again: nothing was deleted.';
    END IF;

    SELECT count(*), string_agg(instance_id::text || '/' || character_id::text, ', ') INTO total, broken FROM (
        SELECT instance_id, character_id FROM token WHERE character_type = 'pj'
            GROUP BY instance_id, character_id HAVING count(*) > 1
    ) duplicated;
    IF total > 0 THEN
        RAISE EXCEPTION 'token-roster: % PC(s) with several tokens on an instance (instance/character): %', total, broken
            USING ERRCODE = 'unique_violation', CONSTRAINT = 'token_instance_pj', HINT = 'Repair the rows named, as their owner decides, then migrate again: nothing was deleted.';
    END IF;
END
$$;

ALTER TABLE token ALTER COLUMN character_type SET NOT NULL;

ALTER TABLE token DROP CONSTRAINT IF EXISTS token_character_type;
ALTER TABLE token ADD CONSTRAINT token_character_type
    CHECK (character_type IN ('pj', 'pnj'));

-- Invariant pj-hp-not-tracked.
ALTER TABLE token DROP CONSTRAINT IF EXISTS token_pj_hp_not_tracked;
ALTER TABLE token ADD CONSTRAINT token_pj_hp_not_tracked
    CHECK (token_hp_tracked(character_type, hp_current, hp_max));

-- Invariant token-roster: a PC has one token per instance. An NPC's tokens
-- are told apart by their number (token_instance_character_number, 0014).
CREATE UNIQUE INDEX IF NOT EXISTS token_instance_pj
    ON token (instance_id, character_id) WHERE character_type = 'pj';
