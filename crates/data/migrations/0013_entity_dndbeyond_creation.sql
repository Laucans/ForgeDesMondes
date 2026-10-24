-- The Entity aggregate's version trigger (0004_entity.sql), laid again: a
-- D&D Beyond resync — a version from 'dndbeyond' on an element that has one
-- already — changes the stats, AC, HP max, spells and actions alone
-- (dndbeyond-resync-scope); an import that creates its PC, the element's
-- first version, writes it whole. The rest is 0004's, unchanged: the next
-- number of its element (every-change-versioned); a restoration's changes
-- are the values of the version it restores, an earlier one of the same
-- element (restore-exact); an AI proposal, the sheet's background,
-- behaviour, stats, AC, HP max and actions alone
-- (proposal-fills-sheet-fields). What changed is the version's diff, each
-- change naming its field. The transaction is marked as having versioned the
-- element, for the check of its writes at commit.
-- Replays as a no-op, as 0001 to 0012 do: the function is replaced, the
-- trigger 0004 binds to it unchanged.

CREATE OR REPLACE FUNCTION entity_version_added() RETURNS trigger
    LANGUAGE plpgsql AS $$
DECLARE
    expected integer;
    restored record;
    outside text;
BEGIN
    IF EXISTS (
        SELECT 1 FROM jsonb_array_elements(NEW.diff) AS change
        WHERE jsonb_typeof(change -> 'field') IS DISTINCT FROM 'string'
    ) THEN
        RAISE EXCEPTION 'every-change-versioned: each change of a version names its field'
            USING ERRCODE = 'check_violation', CONSTRAINT = 'every_change_versioned';
    END IF;
    SELECT coalesce(max(version_number), 0) + 1 INTO expected
        FROM entity_version WHERE entity_id = NEW.entity_id;
    IF NEW.version_number <> expected THEN
        RAISE EXCEPTION 'every-change-versioned: version % of an element whose next version is %',
                NEW.version_number, expected
            USING ERRCODE = 'check_violation', CONSTRAINT = 'every_change_versioned';
    END IF;
    IF NEW.source = 'restauration' THEN
        SELECT entity_id, version_number, snapshot INTO restored
            FROM entity_version WHERE id = NEW.restored_from_version_id;
        IF NOT FOUND OR restored.entity_id <> NEW.entity_id THEN
            RAISE EXCEPTION 'restore-exact: a restoration restores an earlier version of its element'
                USING ERRCODE = 'check_violation', CONSTRAINT = 'restore_exact';
        END IF;
        IF EXISTS (
            SELECT 1 FROM jsonb_array_elements(NEW.diff) AS change
            WHERE coalesce(restored.snapshot -> (change ->> 'field'), 'null')
                <> coalesce(change -> 'new', 'null')
        ) THEN
            RAISE EXCEPTION 'restore-exact: a restoration rewrites the fields of version % as they were',
                    restored.version_number
                USING ERRCODE = 'check_violation', CONSTRAINT = 'restore_exact';
        END IF;
    END IF;
    IF NEW.source = 'dndbeyond' AND expected > 1 THEN
        SELECT change ->> 'field' INTO outside
            FROM jsonb_array_elements(NEW.diff) AS change
            WHERE NOT (change ->> 'field') = ANY (ARRAY[
                'strength', 'dexterity', 'constitution', 'intelligence', 'wisdom', 'charisma',
                'armorClass', 'hpMax', 'spells', 'actions',
                'dndbeyondId', 'dndbeyondSyncedAt', 'updatedAt'])
            LIMIT 1;
        IF FOUND THEN
            RAISE EXCEPTION 'dndbeyond-resync-scope: a resync does not change %', outside
                USING ERRCODE = 'check_violation', CONSTRAINT = 'dndbeyond_resync_scope';
        END IF;
    END IF;
    IF NEW.source = 'ia' THEN
        SELECT change ->> 'field' INTO outside
            FROM jsonb_array_elements(NEW.diff) AS change
            WHERE NOT (change ->> 'field') = ANY (ARRAY[
                'background', 'behavior',
                'strength', 'dexterity', 'constitution', 'intelligence', 'wisdom', 'charisma',
                'armorClass', 'hpMax', 'actions',
                'initialPrompt', 'updatedAt'])
            LIMIT 1;
        IF FOUND THEN
            RAISE EXCEPTION 'proposal-fills-sheet-fields: a proposal does not fill %', outside
                USING ERRCODE = 'check_violation', CONSTRAINT = 'proposal_fills_sheet_fields';
        END IF;
    END IF;
    PERFORM set_config(entity_versioned_setting(NEW.entity_id), 'on', true);
    RETURN NEW;
END
$$;
