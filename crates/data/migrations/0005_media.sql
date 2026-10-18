-- The Media aggregate (crates/dataguard/aggregates/Media/aggregate.json): a
-- campaign's battlemaps and illustrations, uploaded and ready-made, and the
-- hex grid of each battlemap; and the foreign keys to media.id the Campaign
-- and Entity aggregates left as plain columns. Replays as a no-op, as 0001
-- to 0004 do: the rules' runtime applies it with psql, then the server
-- through the Data layer. Tables and indexes are `IF NOT EXISTS` with their
-- constraints inline, functions `OR REPLACE`, and each trigger and each
-- foreign key added to an older table is dropped and created again.
--
-- The invariants a row's own columns can judge are CHECKs, named after their
-- invariant; those that read other rows are constraint triggers, deferred to
-- the end of the transaction so a battlemap and its grid are written
-- together, and a cascade is judged once every row has moved. A trigger's
-- refusal names its invariant: the error's constraint is the invariant's id
-- in snake case, its message starts with the id itself.

CREATE TABLE IF NOT EXISTS media (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation Media.campaignId: onDelete cascade, onUpdate propagate.
    campaign_id uuid NOT NULL REFERENCES campaign (id) ON DELETE CASCADE ON UPDATE CASCADE,
    -- grid-only-for-battlemap is judged on the kind.
    kind text NOT NULL CONSTRAINT media_kind CHECK (kind IN ('battlemap', 'illustration')),
    title text NOT NULL,
    file_path text NOT NULL,
    width_px integer NOT NULL,
    height_px integer NOT NULL,
    -- Relation Media.placeId: onDelete nullify, onUpdate propagate.
    place_id uuid REFERENCES entity (id) ON DELETE SET NULL ON UPDATE CASCADE,
    uploaded_at timestamptz NOT NULL DEFAULT now()
);

-- A campaign's media are listed, and cascaded, by their campaign; a place's
-- found, and nullified, by their place.
CREATE INDEX IF NOT EXISTS media_campaign_id ON media (campaign_id);
CREATE INDEX IF NOT EXISTS media_place_id ON media (place_id);

CREATE TABLE IF NOT EXISTS battlemap_grid (
    -- Relation Media.mediaId: onDelete cascade, onUpdate propagate. One grid
    -- per battlemap.
    media_id uuid PRIMARY KEY REFERENCES media (id) ON DELETE CASCADE ON UPDATE CASCADE,
    hex_size_px numeric(7,2) NOT NULL,
    offset_x numeric(7,2) NOT NULL,
    offset_y numeric(7,2) NOT NULL,
    orientation text NOT NULL CONSTRAINT battlemap_grid_orientation
        CHECK (orientation IN ('pointe', 'plat')),
    color text NOT NULL,
    opacity numeric(3,2) NOT NULL,
    -- hex-is-five-feet: generated, never written, whatever the hex's size,
    -- offset or orientation.
    feet_per_hex smallint NOT NULL GENERATED ALWAYS AS (5) STORED
        CONSTRAINT hex_is_five_feet CHECK (feet_per_hex = 5),
    CONSTRAINT grid_values_range CHECK (hex_size_px > 0 AND opacity BETWEEN 0 AND 1)
);

-- grid-only-for-battlemap: `medium`, if it is still there, is a battlemap
-- with its grid, or something else without one.
CREATE OR REPLACE FUNCTION grid_only_for_battlemap(medium uuid) RETURNS void
    LANGUAGE plpgsql AS $$
DECLARE
    found_kind text;
    gridded boolean;
BEGIN
    SELECT m.kind, EXISTS (SELECT 1 FROM battlemap_grid g WHERE g.media_id = m.id)
        INTO found_kind, gridded
        FROM media m WHERE m.id = medium;
    IF NOT FOUND THEN
        RETURN;
    END IF;
    IF found_kind = 'battlemap' AND NOT gridded THEN
        RAISE EXCEPTION 'grid-only-for-battlemap: a battlemap has its grid from its upload'
            USING ERRCODE = 'check_violation', CONSTRAINT = 'grid_only_for_battlemap';
    END IF;
    IF found_kind <> 'battlemap' AND gridded THEN
        RAISE EXCEPTION 'grid-only-for-battlemap: a grid for a media of kind %', found_kind
            USING ERRCODE = 'check_violation', CONSTRAINT = 'grid_only_for_battlemap';
    END IF;
END
$$;

CREATE OR REPLACE FUNCTION grid_only_for_battlemap_trigger() RETURNS trigger
    LANGUAGE plpgsql AS $$
BEGIN
    IF TG_TABLE_NAME = 'media' THEN
        PERFORM grid_only_for_battlemap(NEW.id);
    ELSIF TG_OP = 'INSERT' THEN
        PERFORM grid_only_for_battlemap(NEW.media_id);
    ELSIF TG_OP = 'DELETE' THEN
        PERFORM grid_only_for_battlemap(OLD.media_id);
    ELSE
        PERFORM grid_only_for_battlemap(OLD.media_id);
        PERFORM grid_only_for_battlemap(NEW.media_id);
    END IF;
    RETURN NULL;
END
$$;

DROP TRIGGER IF EXISTS grid_only_for_battlemap ON media;
CREATE CONSTRAINT TRIGGER grid_only_for_battlemap
    AFTER INSERT OR UPDATE OF kind ON media
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION grid_only_for_battlemap_trigger();

DROP TRIGGER IF EXISTS grid_only_for_battlemap ON battlemap_grid;
CREATE CONSTRAINT TRIGGER grid_only_for_battlemap
    AFTER INSERT OR UPDATE OR DELETE ON battlemap_grid
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION grid_only_for_battlemap_trigger();

-- media-place-is-lieu: the place of `medium`, if it has one, is an element
-- of type « lieu » of the media's own campaign.
CREATE OR REPLACE FUNCTION media_place_is_lieu(medium uuid) RETURNS void
    LANGUAGE plpgsql AS $$
DECLARE
    place record;
BEGIN
    SELECT e.type, e.campaign_id, m.campaign_id AS media_campaign_id
        INTO place
        FROM media m
        JOIN entity e ON e.id = m.place_id
        WHERE m.id = medium;
    IF NOT FOUND THEN
        RETURN;
    END IF;
    IF place.type <> 'lieu' THEN
        RAISE EXCEPTION 'media-place-is-lieu: the place of a media is a lieu, not a %', place.type
            USING ERRCODE = 'check_violation', CONSTRAINT = 'media_place_is_lieu';
    END IF;
    IF place.campaign_id <> place.media_campaign_id THEN
        RAISE EXCEPTION 'media-place-is-lieu: the place belongs to another campaign'
            USING ERRCODE = 'check_violation', CONSTRAINT = 'media_place_is_lieu';
    END IF;
END
$$;

CREATE OR REPLACE FUNCTION media_place_is_lieu_trigger() RETURNS trigger
    LANGUAGE plpgsql AS $$
DECLARE
    medium uuid;
BEGIN
    IF TG_TABLE_NAME = 'media' THEN
        PERFORM media_place_is_lieu(NEW.id);
    ELSE
        FOR medium IN SELECT id FROM media WHERE place_id = NEW.id LOOP
            PERFORM media_place_is_lieu(medium);
        END LOOP;
    END IF;
    RETURN NULL;
END
$$;

DROP TRIGGER IF EXISTS media_place_is_lieu ON media;
CREATE CONSTRAINT TRIGGER media_place_is_lieu
    AFTER INSERT OR UPDATE OF place_id, campaign_id ON media
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION media_place_is_lieu_trigger();

DROP TRIGGER IF EXISTS media_place_is_lieu ON entity;
CREATE CONSTRAINT TRIGGER media_place_is_lieu
    AFTER UPDATE OF type, campaign_id ON entity
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION media_place_is_lieu_trigger();

-- The foreign keys 0002 and 0004 left to this table, under the names an
-- inline REFERENCES would have given them. Relation Campaign.coverMediaId,
-- Entity.portraitMediaId and Entity.tokenMediaId: onDelete nullify, onUpdate
-- propagate.
ALTER TABLE campaign DROP CONSTRAINT IF EXISTS campaign_cover_media_id_fkey;
ALTER TABLE campaign ADD CONSTRAINT campaign_cover_media_id_fkey
    FOREIGN KEY (cover_media_id) REFERENCES media (id) ON DELETE SET NULL ON UPDATE CASCADE;

ALTER TABLE character DROP CONSTRAINT IF EXISTS character_portrait_media_id_fkey;
ALTER TABLE character ADD CONSTRAINT character_portrait_media_id_fkey
    FOREIGN KEY (portrait_media_id) REFERENCES media (id) ON DELETE SET NULL ON UPDATE CASCADE;

ALTER TABLE character DROP CONSTRAINT IF EXISTS character_token_media_id_fkey;
ALTER TABLE character ADD CONSTRAINT character_token_media_id_fkey
    FOREIGN KEY (token_media_id) REFERENCES media (id) ON DELETE SET NULL ON UPDATE CASCADE;

-- A media's deletion, or its new id, finds what points to it.
CREATE INDEX IF NOT EXISTS campaign_cover_media_id ON campaign (cover_media_id);
CREATE INDEX IF NOT EXISTS character_portrait_media_id ON character (portrait_media_id);
CREATE INDEX IF NOT EXISTS character_token_media_id ON character (token_media_id);

-- default-token: a crop is a crop of the portrait. A portrait nullified by
-- its media's deletion takes its crop with it — the token is the disc of its
-- colour then; a portrait cleared by hand under a crop is still refused by
-- character_default_token_crop (0004).
CREATE OR REPLACE FUNCTION token_crop_follows_portrait() RETURNS trigger
    LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.portrait_media_id IS NULL AND OLD.portrait_media_id IS NOT NULL
        AND NOT EXISTS (SELECT 1 FROM media WHERE id = OLD.portrait_media_id)
    THEN
        NEW.token_crop := NULL;
    END IF;
    RETURN NEW;
END
$$;

DROP TRIGGER IF EXISTS token_crop_follows_portrait ON character;
CREATE TRIGGER token_crop_follows_portrait
    BEFORE UPDATE OF portrait_media_id ON character
    FOR EACH ROW EXECUTE FUNCTION token_crop_follows_portrait();

-- every-change-versioned (0004), with what the media relations do: what the
-- relations do is no one's save of an element, and a sheet's portrait or
-- token nullified by its media's deletion, or given its media's new id — the
-- crop gone with the portrait — is the relation's doing too. The rest is
-- 0004's, as it was.
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
        IF to_jsonb(NEW) - 'portrait_media_id' - 'token_media_id' - 'token_crop'
                = to_jsonb(OLD) - 'portrait_media_id' - 'token_media_id' - 'token_crop'
            AND (NEW.portrait_media_id IS DISTINCT FROM OLD.portrait_media_id
                OR NEW.token_media_id IS DISTINCT FROM OLD.token_media_id)
            AND (NEW.portrait_media_id IS NOT DISTINCT FROM OLD.portrait_media_id
                OR (OLD.portrait_media_id IS NOT NULL
                    AND NOT EXISTS (SELECT 1 FROM media WHERE id = OLD.portrait_media_id)))
            AND (NEW.token_media_id IS NOT DISTINCT FROM OLD.token_media_id
                OR (OLD.token_media_id IS NOT NULL
                    AND NOT EXISTS (SELECT 1 FROM media WHERE id = OLD.token_media_id)))
            AND (NEW.token_crop IS NOT DISTINCT FROM OLD.token_crop
                OR (NEW.token_crop IS NULL AND NEW.portrait_media_id IS NULL
                    AND OLD.portrait_media_id IS NOT NULL))
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
