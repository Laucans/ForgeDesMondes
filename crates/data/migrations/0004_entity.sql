-- The Entity aggregate (crates/dataguard/aggregates/Entity/aggregate.json):
-- a campaign's elements (PCs, NPCs, places, events), their character sheets,
-- their aliases and their versions, and the two views computed from them.
-- Replays as a no-op, as 0001 to 0003 do: the rules' runtime applies it with
-- psql, then the server through the Data layer. Tables and indexes are
-- `IF NOT EXISTS` with their constraints inline, functions and views
-- `OR REPLACE`, and each trigger is dropped and created again.
--
-- The invariants a row's own columns can judge are CHECKs; those that read
-- other rows are constraint triggers, deferred to the end of the transaction
-- so a cascade (a campaign's new id reaching its elements and its players)
-- is judged once every row has moved. A refusal names its invariant: the
-- error's constraint is the invariant's id in snake case, its message starts
-- with the id itself.
--
-- portrait_media_id and token_media_id are plain columns: their foreign keys
-- to media.id (onDelete nullify, onUpdate propagate) come with the Media
-- aggregate's table. dex-modifier-derived: no column stores the DEX modifier,
-- floor((dexterity - 10) / 2) per AbilityModifier@1, computed by whoever
-- reads the sheet.

-- AliasMatch@1: the term in lowercase, with no accent (a combining mark
-- U+0300 to U+036F of the decomposed letter, the rest recomposed) and no
-- Unicode white space around it. Blank, it names nothing.
CREATE OR REPLACE FUNCTION alias_match_normalized(term text) RETURNS text
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN normalize(
        regexp_replace(
            normalize(
                lower(regexp_replace(
                    term,
                    '^[\t\n\v\f\r \u0085   -     　]+|[\t\n\v\f\r \u0085   -     　]+$',
                    '',
                    'g'
                )),
                NFD
            ),
            '[̀-ͯ]',
            '',
            'g'
        ),
        NFC
    );

CREATE TABLE IF NOT EXISTS entity (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation Entity.entityCampaignId: onDelete cascade, onUpdate propagate.
    campaign_id uuid NOT NULL REFERENCES campaign (id) ON DELETE CASCADE ON UPDATE CASCADE,
    type text NOT NULL CONSTRAINT entity_type CHECK (type IN ('pj', 'pnj', 'lieu', 'evenement')),
    name text NOT NULL,
    description text,
    notes text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- A campaign's elements are read, counted and cascaded by their campaign.
CREATE INDEX IF NOT EXISTS entity_campaign_id ON entity (campaign_id);

CREATE TABLE IF NOT EXISTS character (
    -- Relation Entity.characterEntityId: onDelete cascade, onUpdate propagate.
    -- One sheet per element at most.
    entity_id uuid PRIMARY KEY REFERENCES entity (id) ON DELETE CASCADE ON UPDATE CASCADE,
    -- Relation Entity.playerId: onDelete nullify, onUpdate propagate.
    player_id uuid REFERENCES player (id) ON DELETE SET NULL ON UPDATE CASCADE,
    initial_prompt text,
    background text,
    behavior text,
    strength smallint,
    dexterity smallint,
    constitution smallint,
    intelligence smallint,
    wisdom smallint,
    charisma smallint,
    armor_class smallint,
    hp_max smallint,
    spells jsonb NOT NULL DEFAULT '[]',
    actions jsonb NOT NULL DEFAULT '[]',
    portrait_media_id uuid,
    token_media_id uuid,
    token_crop jsonb,
    -- default-token: with no image, the token is a disc of this colour.
    token_color text NOT NULL CONSTRAINT character_default_token_color
        CHECK (btrim(token_color) <> ''),
    dndbeyond_id text,
    dndbeyond_synced_at timestamptz,
    -- default-token: a crop is a crop of the portrait.
    CONSTRAINT character_default_token_crop CHECK (token_crop IS NULL OR portrait_media_id IS NOT NULL)
);

-- A player's PCs are found, and nullified, by their player.
CREATE INDEX IF NOT EXISTS character_player_id ON character (player_id);

CREATE TABLE IF NOT EXISTS entity_alias (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation Entity.entityAliasEntityId: onDelete cascade, onUpdate propagate.
    entity_id uuid NOT NULL REFERENCES entity (id) ON DELETE CASCADE ON UPDATE CASCADE,
    alias text NOT NULL
);

CREATE INDEX IF NOT EXISTS entity_alias_entity_id ON entity_alias (entity_id);

CREATE TABLE IF NOT EXISTS entity_version (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation Entity.entityVersionEntityId: onDelete cascade, onUpdate
    -- restrict: an element with a history keeps its id.
    entity_id uuid NOT NULL REFERENCES entity (id) ON DELETE CASCADE ON UPDATE RESTRICT,
    -- EntityVersion@1: 1, then the previous number plus 1.
    version_number integer NOT NULL CONSTRAINT entity_version_number CHECK (version_number >= 1),
    source text NOT NULL CONSTRAINT entity_version_source
        CHECK (source IN ('dm', 'ia', 'dndbeyond', 'brief', 'restauration')),
    snapshot jsonb NOT NULL CONSTRAINT entity_version_snapshot CHECK (jsonb_typeof(snapshot) = 'object'),
    diff jsonb NOT NULL CONSTRAINT entity_version_diff CHECK (jsonb_typeof(diff) = 'array'),
    -- Relation Entity.restoredFromVersionId: onDelete nullify, onUpdate
    -- restrict.
    restored_from_version_id uuid REFERENCES entity_version (id) ON DELETE SET NULL ON UPDATE RESTRICT,
    created_at timestamptz NOT NULL DEFAULT now(),
    -- restore-exact: only a restoration restores.
    CONSTRAINT entity_version_restored_by_restoration
        CHECK (restored_from_version_id IS NULL OR source = 'restauration'),
    CONSTRAINT entity_version_numbered UNIQUE (entity_id, version_number)
);

CREATE INDEX IF NOT EXISTS entity_version_restored_from_version_id
    ON entity_version (restored_from_version_id);

-- character-is-pj-or-pnj: the sheet of `element`, if it has one, is a PJ's
-- or a PNJ's; only a PJ has a player, and one at its own campaign's table.
CREATE OR REPLACE FUNCTION character_is_pj_or_pnj(element uuid) RETURNS void
    LANGUAGE plpgsql AS $$
DECLARE
    sheet record;
BEGIN
    SELECT e.type, e.campaign_id, c.player_id, p.campaign_id AS player_campaign_id
        INTO sheet
        FROM character c
        JOIN entity e ON e.id = c.entity_id
        LEFT JOIN player p ON p.id = c.player_id
        WHERE c.entity_id = element;
    IF NOT FOUND THEN
        RETURN;
    END IF;
    IF sheet.type NOT IN ('pj', 'pnj') THEN
        RAISE EXCEPTION 'character-is-pj-or-pnj: a sheet for an element of type %', sheet.type
            USING ERRCODE = 'check_violation', CONSTRAINT = 'character_is_pj_or_pnj';
    END IF;
    IF sheet.player_id IS NOT NULL AND sheet.type <> 'pj' THEN
        RAISE EXCEPTION 'character-is-pj-or-pnj: a player for a %', sheet.type
            USING ERRCODE = 'check_violation', CONSTRAINT = 'character_is_pj_or_pnj';
    END IF;
    IF sheet.player_id IS NOT NULL AND sheet.player_campaign_id <> sheet.campaign_id THEN
        RAISE EXCEPTION 'character-is-pj-or-pnj: the player belongs to another campaign'
            USING ERRCODE = 'check_violation', CONSTRAINT = 'character_is_pj_or_pnj';
    END IF;
END
$$;

CREATE OR REPLACE FUNCTION character_is_pj_or_pnj_trigger() RETURNS trigger
    LANGUAGE plpgsql AS $$
DECLARE
    element uuid;
BEGIN
    CASE TG_TABLE_NAME
        WHEN 'character' THEN
            PERFORM character_is_pj_or_pnj(NEW.entity_id);
        WHEN 'entity' THEN
            PERFORM character_is_pj_or_pnj(NEW.id);
        WHEN 'player' THEN
            FOR element IN SELECT entity_id FROM character WHERE player_id = NEW.id LOOP
                PERFORM character_is_pj_or_pnj(element);
            END LOOP;
    END CASE;
    RETURN NULL;
END
$$;

DROP TRIGGER IF EXISTS character_is_pj_or_pnj ON character;
CREATE CONSTRAINT TRIGGER character_is_pj_or_pnj
    AFTER INSERT OR UPDATE ON character
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION character_is_pj_or_pnj_trigger();

DROP TRIGGER IF EXISTS character_is_pj_or_pnj ON entity;
CREATE CONSTRAINT TRIGGER character_is_pj_or_pnj
    AFTER UPDATE OF type, campaign_id ON entity
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION character_is_pj_or_pnj_trigger();

DROP TRIGGER IF EXISTS character_is_pj_or_pnj ON player;
CREATE CONSTRAINT TRIGGER character_is_pj_or_pnj
    AFTER UPDATE OF campaign_id ON player
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION character_is_pj_or_pnj_trigger();

-- alias-unique-in-campaign: no alias of `element`, normalised per
-- AliasMatch@1, names another element of its campaign, by its name or one of
-- its aliases; nor does the element's name double another's alias. One
-- campaign is judged at a time, so two writers cannot both pass.
CREATE OR REPLACE FUNCTION alias_unique_in_campaign(element uuid) RETURNS void
    LANGUAGE plpgsql AS $$
DECLARE
    campaign uuid;
    taken text;
BEGIN
    SELECT campaign_id INTO campaign FROM entity WHERE id = element;
    IF NOT FOUND THEN
        RETURN;
    END IF;
    PERFORM pg_advisory_xact_lock(hashtext('alias-unique-in-campaign'), hashtext(campaign::text));
    WITH own (term, is_alias) AS (
        SELECT alias_match_normalized(alias), true FROM entity_alias WHERE entity_id = element
        UNION ALL
        SELECT alias_match_normalized(name), false FROM entity WHERE id = element
    ), others (term, is_alias) AS (
        SELECT alias_match_normalized(a.alias), true
            FROM entity_alias a JOIN entity e ON e.id = a.entity_id
            WHERE e.campaign_id = campaign AND e.id <> element
        UNION ALL
        SELECT alias_match_normalized(e.name), false
            FROM entity e
            WHERE e.campaign_id = campaign AND e.id <> element
    )
    SELECT own.term INTO taken
        FROM own JOIN others ON others.term = own.term
        WHERE own.term <> '' AND (own.is_alias OR others.is_alias)
        LIMIT 1;
    IF FOUND THEN
        RAISE EXCEPTION 'alias-unique-in-campaign: « % » already names another element', taken
            USING ERRCODE = 'unique_violation', CONSTRAINT = 'alias_unique_in_campaign';
    END IF;
END
$$;

CREATE OR REPLACE FUNCTION alias_unique_in_campaign_trigger() RETURNS trigger
    LANGUAGE plpgsql AS $$
BEGIN
    CASE TG_TABLE_NAME
        WHEN 'entity_alias' THEN
            PERFORM alias_unique_in_campaign(NEW.entity_id);
        WHEN 'entity' THEN
            PERFORM alias_unique_in_campaign(NEW.id);
    END CASE;
    RETURN NULL;
END
$$;

DROP TRIGGER IF EXISTS alias_unique_in_campaign ON entity_alias;
CREATE CONSTRAINT TRIGGER alias_unique_in_campaign
    AFTER INSERT OR UPDATE ON entity_alias
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION alias_unique_in_campaign_trigger();

DROP TRIGGER IF EXISTS alias_unique_in_campaign ON entity;
CREATE CONSTRAINT TRIGGER alias_unique_in_campaign
    AFTER INSERT OR UPDATE OF name, campaign_id ON entity
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION alias_unique_in_campaign_trigger();

-- every-change-versioned: a version is never modified, nor erased: it goes
-- with its element alone, by the relation's cascade once the element is gone.
CREATE OR REPLACE FUNCTION entity_version_never_modified() RETURNS trigger
    LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND NOT EXISTS (SELECT 1 FROM entity WHERE id = OLD.entity_id) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'every-change-versioned: version % of an element is never modified', OLD.version_number
        USING ERRCODE = 'check_violation', CONSTRAINT = 'every_change_versioned';
END
$$;

DROP TRIGGER IF EXISTS every_change_versioned ON entity_version;
CREATE TRIGGER every_change_versioned
    BEFORE UPDATE OR DELETE ON entity_version
    FOR EACH ROW EXECUTE FUNCTION entity_version_never_modified();

-- The transaction-local setting saying this transaction added a version of
-- `element`: kept by a savepoint released, undone by one rolled back, gone
-- at the end of the transaction.
CREATE OR REPLACE FUNCTION entity_versioned_setting(element uuid) RETURNS text
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN 'entity.versioned_' || replace(element::text, '-', '_');

-- What a new version may be: the next number of its element
-- (every-change-versioned); a restoration's changes are the values of the
-- version it restores, an earlier one of the same element (restore-exact); a
-- D&D Beyond resync changes the stats, AC, HP max, spells and actions alone
-- (dndbeyond-resync-scope); an AI proposal, the sheet's background,
-- behaviour, stats, AC, HP max and actions alone
-- (proposal-fills-sheet-fields). What changed is the version's diff, each
-- change naming its field. The transaction is marked as having versioned the
-- element, for the check of its writes at commit.
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
    IF NEW.source = 'dndbeyond' THEN
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

DROP TRIGGER IF EXISTS entity_version_added ON entity_version;
CREATE TRIGGER entity_version_added
    BEFORE INSERT ON entity_version
    FOR EACH ROW EXECUTE FUNCTION entity_version_added();

-- every-change-versioned: whoever writes an element, its sheet or its
-- aliases adds the element's version in the same transaction — both
-- elements' when an alias moves from one to the other. What the relations do
-- is no one's save of it: a cascade from the campaign, the element's own
-- deletion, its player's deletion or new id reaching the PC's player.
CREATE OR REPLACE FUNCTION every_change_versioned_trigger() RETURNS trigger
    LANGUAGE plpgsql AS $$
DECLARE
    elements uuid[];
    element uuid;
BEGIN
    -- Nested: PL/pgSQL resolves OLD.player_id before it tests the table.
    IF TG_TABLE_NAME = 'character' AND TG_OP = 'UPDATE' THEN
        IF OLD.player_id IS NOT NULL
            AND to_jsonb(NEW) - 'player_id' = to_jsonb(OLD) - 'player_id'
            AND NOT EXISTS (SELECT 1 FROM player WHERE id = OLD.player_id)
        THEN
            RETURN NULL;
        END IF;
    END IF;
    IF TG_TABLE_NAME = 'entity' THEN
        elements := ARRAY[NEW.id];
    ELSIF TG_OP = 'INSERT' THEN
        elements := ARRAY[NEW.entity_id];
    ELSIF TG_OP = 'DELETE' THEN
        elements := ARRAY[OLD.entity_id];
    ELSE
        elements := ARRAY[OLD.entity_id, NEW.entity_id];
    END IF;
    FOREACH element IN ARRAY elements LOOP
        IF EXISTS (SELECT 1 FROM entity WHERE id = element)
            AND current_setting(entity_versioned_setting(element), true) IS DISTINCT FROM 'on'
        THEN
            RAISE EXCEPTION 'every-change-versioned: element % was written with no version', element
                USING ERRCODE = 'check_violation', CONSTRAINT = 'every_change_versioned';
        END IF;
    END LOOP;
    RETURN NULL;
END
$$;

DROP TRIGGER IF EXISTS every_change_versioned ON entity;
CREATE CONSTRAINT TRIGGER every_change_versioned
    AFTER INSERT OR UPDATE OF type, name, description, notes ON entity
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION every_change_versioned_trigger();

DROP TRIGGER IF EXISTS every_change_versioned ON character;
CREATE CONSTRAINT TRIGGER every_change_versioned
    AFTER INSERT OR UPDATE OR DELETE ON character
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION every_change_versioned_trigger();

DROP TRIGGER IF EXISTS every_change_versioned ON entity_alias;
CREATE CONSTRAINT TRIGGER every_change_versioned
    AFTER INSERT OR UPDATE OF alias, entity_id OR DELETE ON entity_alias
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION every_change_versioned_trigger();

-- The transcription's vocabulary, the live detection's and the rail's
-- search: every name and alias of a campaign's elements, normalised per
-- AliasMatch@1; a blank term names nothing. The sounds' terms join it with
-- the Sound aggregate's table.
CREATE OR REPLACE VIEW detection_vocabulary_view AS
    SELECT e.campaign_id, e.id AS entity_id, NULL::uuid AS sound_id,
           e.name AS term, alias_match_normalized(e.name) AS normalized_term
        FROM entity e
        WHERE alias_match_normalized(e.name) <> ''
    UNION ALL
    SELECT e.campaign_id, e.id, NULL::uuid,
           a.alias, alias_match_normalized(a.alias)
        FROM entity_alias a JOIN entity e ON e.id = a.entity_id
        WHERE alias_match_normalized(a.alias) <> '';

-- The Campaign aggregate's read model, for the home page and the overview:
-- each campaign's counts, never stored. The session, media and sound counts
-- and the session dates join it with their milestones' tables.
CREATE OR REPLACE VIEW campaign_overview_view AS
    SELECT c.id AS campaign_id,
           jsonb_build_object(
               'players', (SELECT count(*) FROM player p WHERE p.campaign_id = c.id),
               'characters', (SELECT count(*) FROM entity e
                              WHERE e.campaign_id = c.id AND e.type IN ('pj', 'pnj')),
               'places', (SELECT count(*) FROM entity e
                          WHERE e.campaign_id = c.id AND e.type = 'lieu'),
               'events', (SELECT count(*) FROM entity e
                          WHERE e.campaign_id = c.id AND e.type = 'evenement')
           ) AS entity_counts
        FROM campaign c;
