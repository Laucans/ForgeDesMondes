--
-- PostgreSQL database dump
--


-- Dumped from database version 16.15 (Debian 16.15-1.pgdg13+2)
-- Dumped by pg_dump version 16.15 (Homebrew)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: alias_match_normalized(text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.alias_match_normalized(term text) RETURNS text
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN NORMALIZE(regexp_replace(NORMALIZE(lower(regexp_replace(term, '^[\t\n\v\f\r \u0085   -     　]+|[\t\n\v\f\r \u0085   -     　]+$'::text, ''::text, 'g'::text)), NFD), '[̀-ͯ]'::text, ''::text, 'g'::text), NFC);


--
-- Name: alias_unique_in_campaign(uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.alias_unique_in_campaign(element uuid) RETURNS void
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: alias_unique_in_campaign_trigger(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.alias_unique_in_campaign_trigger() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: character_is_pj_or_pnj(uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.character_is_pj_or_pnj(element uuid) RETURNS void
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: character_is_pj_or_pnj_trigger(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.character_is_pj_or_pnj_trigger() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: entity_version_added(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.entity_version_added() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: entity_version_never_modified(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.entity_version_never_modified() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP = 'DELETE' AND NOT EXISTS (SELECT 1 FROM entity WHERE id = OLD.entity_id) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'every-change-versioned: version % of an element is never modified', OLD.version_number
        USING ERRCODE = 'check_violation', CONSTRAINT = 'every_change_versioned';
END
$$;


--
-- Name: entity_versioned_setting(uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.entity_versioned_setting(element uuid) RETURNS text
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN ('entity.versioned_'::text || replace((element)::text, '-'::text, '_'::text));


--
-- Name: every_change_versioned_trigger(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.every_change_versioned_trigger() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: no_creation_before_validation(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.no_creation_before_validation() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.created_entity_id IS NOT NULL THEN
            RAISE EXCEPTION 'no-creation-before-validation: the analysis of notes creates no element'
                USING ERRCODE = 'check_violation', CONSTRAINT = 'no_creation_before_validation';
        END IF;
        RETURN NEW;
    END IF;
    IF OLD.created_entity_id IS NOT NULL
        AND NEW.created_entity_id IS DISTINCT FROM OLD.created_entity_id
        AND EXISTS (SELECT 1 FROM entity WHERE id = OLD.created_entity_id)
    THEN
        RAISE EXCEPTION 'no-creation-before-validation: proposal « % » already created its element', OLD.name
            USING ERRCODE = 'check_violation', CONSTRAINT = 'no_creation_before_validation';
    END IF;
    RETURN NEW;
END
$$;


--
-- Name: sound_tags_normalized(text[]); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.sound_tags_normalized(tags text[]) RETURNS boolean
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN ((COALESCE(array_ndims(tags), 1) = 1) AND (NOT (EXISTS (SELECT FROM unnest(sound_tags_normalized.tags) tag(tag) WHERE ((tag.tag IS NULL) OR (tag.tag <> lower(tag.tag)))))) AND (cardinality(tags) = (SELECT count(DISTINCT tag.tag) AS count FROM unnest(sound_tags_normalized.tags) tag(tag))));


SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: ai_provider; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.ai_provider (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    mode text NOT NULL,
    label text NOT NULL,
    url text,
    detected_api_type text,
    api_key text,
    "binary" text,
    generation_model text NOT NULL,
    summary_model text NOT NULL,
    last_test_ok boolean,
    last_test_at timestamp with time zone,
    last_test_message text,
    last_test_latency_ms integer,
    CONSTRAINT ai_provider_api_type_recognized CHECK (((detected_api_type IS NULL) OR (detected_api_type = ANY (ARRAY['cloud'::text, 'opencode'::text])))),
    CONSTRAINT ai_provider_mode_fields CHECK ((((mode = 'api'::text) AND (COALESCE(url, ''::text) <> ''::text) AND (COALESCE(detected_api_type, ''::text) <> ''::text) AND (COALESCE("binary", ''::text) = ''::text)) OR ((mode = 'binaire'::text) AND (COALESCE("binary", ''::text) = ANY (ARRAY['opencode'::text, 'claude-code'::text])) AND (COALESCE(url, ''::text) = ''::text))))
);


--
-- Name: ai_settings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.ai_settings (
    id smallint DEFAULT 1 NOT NULL,
    active_provider_id uuid NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT ai_settings_single_row CHECK ((id = 1))
);


--
-- Name: campaign; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.campaign (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name text NOT NULL,
    kind text NOT NULL,
    pitch text,
    art text,
    glyph text,
    cover_media_id uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    last_exported_at timestamp with time zone,
    imported_at timestamp with time zone,
    origin_campaign_id uuid,
    sound_master_volume numeric(3,2) DEFAULT 1 NOT NULL,
    CONSTRAINT campaign_kind CHECK ((kind = ANY (ARRAY['campagne'::text, 'oneshot'::text]))),
    CONSTRAINT campaign_master_volume_range CHECK (((sound_master_volume >= (0)::numeric) AND (sound_master_volume <= (1)::numeric)))
);


--
-- Name: entity; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.entity (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    campaign_id uuid NOT NULL,
    type text NOT NULL,
    name text NOT NULL,
    description text,
    notes text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT entity_type CHECK ((type = ANY (ARRAY['pj'::text, 'pnj'::text, 'lieu'::text, 'evenement'::text])))
);


--
-- Name: player; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.player (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    campaign_id uuid NOT NULL,
    name text NOT NULL,
    notes text,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: campaign_overview_view; Type: VIEW; Schema: public; Owner: -
--

CREATE VIEW public.campaign_overview_view AS
 SELECT id AS campaign_id,
    jsonb_build_object('players', ( SELECT count(*) AS count
           FROM public.player p
          WHERE (p.campaign_id = c.id)), 'characters', ( SELECT count(*) AS count
           FROM public.entity e
          WHERE ((e.campaign_id = c.id) AND (e.type = ANY (ARRAY['pj'::text, 'pnj'::text])))), 'places', ( SELECT count(*) AS count
           FROM public.entity e
          WHERE ((e.campaign_id = c.id) AND (e.type = 'lieu'::text))), 'events', ( SELECT count(*) AS count
           FROM public.entity e
          WHERE ((e.campaign_id = c.id) AND (e.type = 'evenement'::text)))) AS entity_counts
   FROM public.campaign c;


--
-- Name: character; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."character" (
    entity_id uuid NOT NULL,
    player_id uuid,
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
    spells jsonb DEFAULT '[]'::jsonb NOT NULL,
    actions jsonb DEFAULT '[]'::jsonb NOT NULL,
    portrait_media_id uuid,
    token_media_id uuid,
    token_crop jsonb,
    token_color text NOT NULL,
    dndbeyond_id text,
    dndbeyond_synced_at timestamp with time zone,
    CONSTRAINT character_default_token_color CHECK ((btrim(token_color) <> ''::text)),
    CONSTRAINT character_default_token_crop CHECK (((token_crop IS NULL) OR (portrait_media_id IS NOT NULL)))
);


--
-- Name: entity_alias; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.entity_alias (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    entity_id uuid NOT NULL,
    alias text NOT NULL
);


--
-- Name: detection_vocabulary_view; Type: VIEW; Schema: public; Owner: -
--

CREATE VIEW public.detection_vocabulary_view AS
 SELECT e.campaign_id,
    e.id AS entity_id,
    NULL::uuid AS sound_id,
    e.name AS term,
    public.alias_match_normalized(e.name) AS normalized_term
   FROM public.entity e
  WHERE (public.alias_match_normalized(e.name) <> ''::text)
UNION ALL
 SELECT e.campaign_id,
    e.id AS entity_id,
    NULL::uuid AS sound_id,
    a.alias AS term,
    public.alias_match_normalized(a.alias) AS normalized_term
   FROM (public.entity_alias a
     JOIN public.entity e ON ((e.id = a.entity_id)))
  WHERE (public.alias_match_normalized(a.alias) <> ''::text);


--
-- Name: entity_version; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.entity_version (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    entity_id uuid NOT NULL,
    version_number integer NOT NULL,
    source text NOT NULL,
    snapshot jsonb NOT NULL,
    diff jsonb NOT NULL,
    restored_from_version_id uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT entity_version_diff CHECK ((jsonb_typeof(diff) = 'array'::text)),
    CONSTRAINT entity_version_number CHECK ((version_number >= 1)),
    CONSTRAINT entity_version_restored_by_restoration CHECK (((restored_from_version_id IS NULL) OR (source = 'restauration'::text))),
    CONSTRAINT entity_version_snapshot CHECK ((jsonb_typeof(snapshot) = 'object'::text)),
    CONSTRAINT entity_version_source CHECK ((source = ANY (ARRAY['dm'::text, 'ia'::text, 'dndbeyond'::text, 'brief'::text, 'restauration'::text])))
);


--
-- Name: imported_note; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.imported_note (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    campaign_id uuid NOT NULL,
    raw_text text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    analyzed_at timestamp with time zone
);


--
-- Name: note_proposal; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.note_proposal (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    note_id uuid NOT NULL,
    type text NOT NULL,
    name text NOT NULL,
    excerpt text NOT NULL,
    existing_entity_id uuid,
    checked boolean NOT NULL,
    created_entity_id uuid,
    CONSTRAINT no_creation_before_validation CHECK (((created_entity_id IS NULL) OR checked)),
    CONSTRAINT note_proposal_type CHECK ((type = ANY (ARRAY['pj'::text, 'pnj'::text, 'lieu'::text, 'evenement'::text])))
);


--
-- Name: sound; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.sound (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    campaign_id uuid NOT NULL,
    name text NOT NULL,
    file_path text NOT NULL,
    tags text[] DEFAULT '{}'::text[] NOT NULL,
    volume numeric(3,2) DEFAULT 1 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT sound_volume_range CHECK (((volume >= (0)::numeric) AND (volume <= (1)::numeric))),
    CONSTRAINT tags_normalized CHECK (public.sound_tags_normalized(tags))
);


--
-- Name: ai_provider ai_provider_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_provider
    ADD CONSTRAINT ai_provider_pkey PRIMARY KEY (id);


--
-- Name: ai_settings ai_settings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_settings
    ADD CONSTRAINT ai_settings_pkey PRIMARY KEY (id);


--
-- Name: campaign campaign_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.campaign
    ADD CONSTRAINT campaign_pkey PRIMARY KEY (id);


--
-- Name: character character_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."character"
    ADD CONSTRAINT character_pkey PRIMARY KEY (entity_id);


--
-- Name: entity_alias entity_alias_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.entity_alias
    ADD CONSTRAINT entity_alias_pkey PRIMARY KEY (id);


--
-- Name: entity entity_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.entity
    ADD CONSTRAINT entity_pkey PRIMARY KEY (id);


--
-- Name: entity_version entity_version_numbered; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.entity_version
    ADD CONSTRAINT entity_version_numbered UNIQUE (entity_id, version_number);


--
-- Name: entity_version entity_version_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.entity_version
    ADD CONSTRAINT entity_version_pkey PRIMARY KEY (id);


--
-- Name: imported_note imported_note_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.imported_note
    ADD CONSTRAINT imported_note_pkey PRIMARY KEY (id);


--
-- Name: note_proposal note_proposal_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.note_proposal
    ADD CONSTRAINT note_proposal_pkey PRIMARY KEY (id);


--
-- Name: player player_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.player
    ADD CONSTRAINT player_pkey PRIMARY KEY (id);


--
-- Name: sound sound_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sound
    ADD CONSTRAINT sound_pkey PRIMARY KEY (id);


--
-- Name: character_player_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX character_player_id ON public."character" USING btree (player_id);


--
-- Name: entity_alias_entity_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX entity_alias_entity_id ON public.entity_alias USING btree (entity_id);


--
-- Name: entity_campaign_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX entity_campaign_id ON public.entity USING btree (campaign_id);


--
-- Name: entity_version_restored_from_version_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX entity_version_restored_from_version_id ON public.entity_version USING btree (restored_from_version_id);


--
-- Name: imported_note_campaign_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX imported_note_campaign_id ON public.imported_note USING btree (campaign_id);


--
-- Name: note_proposal_created_entity_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX note_proposal_created_entity_id ON public.note_proposal USING btree (created_entity_id);


--
-- Name: note_proposal_existing_entity_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX note_proposal_existing_entity_id ON public.note_proposal USING btree (existing_entity_id);


--
-- Name: note_proposal_note_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX note_proposal_note_id ON public.note_proposal USING btree (note_id);


--
-- Name: player_campaign_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX player_campaign_id ON public.player USING btree (campaign_id);


--
-- Name: sound_campaign_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX sound_campaign_id ON public.sound USING btree (campaign_id);


--
-- Name: entity alias_unique_in_campaign; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER alias_unique_in_campaign AFTER INSERT OR UPDATE OF name, campaign_id ON public.entity DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.alias_unique_in_campaign_trigger();


--
-- Name: entity_alias alias_unique_in_campaign; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER alias_unique_in_campaign AFTER INSERT OR UPDATE ON public.entity_alias DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.alias_unique_in_campaign_trigger();


--
-- Name: character character_is_pj_or_pnj; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER character_is_pj_or_pnj AFTER INSERT OR UPDATE ON public."character" DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.character_is_pj_or_pnj_trigger();


--
-- Name: entity character_is_pj_or_pnj; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER character_is_pj_or_pnj AFTER UPDATE OF type, campaign_id ON public.entity DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.character_is_pj_or_pnj_trigger();


--
-- Name: player character_is_pj_or_pnj; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER character_is_pj_or_pnj AFTER UPDATE OF campaign_id ON public.player DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.character_is_pj_or_pnj_trigger();


--
-- Name: entity_version entity_version_added; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER entity_version_added BEFORE INSERT ON public.entity_version FOR EACH ROW EXECUTE FUNCTION public.entity_version_added();


--
-- Name: character every_change_versioned; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER every_change_versioned AFTER INSERT OR DELETE OR UPDATE ON public."character" DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.every_change_versioned_trigger();


--
-- Name: entity every_change_versioned; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER every_change_versioned AFTER INSERT OR UPDATE OF type, name, description, notes ON public.entity DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.every_change_versioned_trigger();


--
-- Name: entity_alias every_change_versioned; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER every_change_versioned AFTER INSERT OR DELETE OR UPDATE OF alias, entity_id ON public.entity_alias DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.every_change_versioned_trigger();


--
-- Name: entity_version every_change_versioned; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER every_change_versioned BEFORE DELETE OR UPDATE ON public.entity_version FOR EACH ROW EXECUTE FUNCTION public.entity_version_never_modified();


--
-- Name: note_proposal no_creation_before_validation; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER no_creation_before_validation BEFORE INSERT OR UPDATE ON public.note_proposal FOR EACH ROW EXECUTE FUNCTION public.no_creation_before_validation();


--
-- Name: ai_settings ai_settings_active_provider_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_settings
    ADD CONSTRAINT ai_settings_active_provider_id_fkey FOREIGN KEY (active_provider_id) REFERENCES public.ai_provider(id) ON UPDATE CASCADE ON DELETE RESTRICT;


--
-- Name: character character_entity_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."character"
    ADD CONSTRAINT character_entity_id_fkey FOREIGN KEY (entity_id) REFERENCES public.entity(id) ON UPDATE CASCADE ON DELETE CASCADE;


--
-- Name: character character_player_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."character"
    ADD CONSTRAINT character_player_id_fkey FOREIGN KEY (player_id) REFERENCES public.player(id) ON UPDATE CASCADE ON DELETE SET NULL;


--
-- Name: entity_alias entity_alias_entity_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.entity_alias
    ADD CONSTRAINT entity_alias_entity_id_fkey FOREIGN KEY (entity_id) REFERENCES public.entity(id) ON UPDATE CASCADE ON DELETE CASCADE;


--
-- Name: entity entity_campaign_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.entity
    ADD CONSTRAINT entity_campaign_id_fkey FOREIGN KEY (campaign_id) REFERENCES public.campaign(id) ON UPDATE CASCADE ON DELETE CASCADE;


--
-- Name: entity_version entity_version_entity_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.entity_version
    ADD CONSTRAINT entity_version_entity_id_fkey FOREIGN KEY (entity_id) REFERENCES public.entity(id) ON UPDATE RESTRICT ON DELETE CASCADE;


--
-- Name: entity_version entity_version_restored_from_version_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.entity_version
    ADD CONSTRAINT entity_version_restored_from_version_id_fkey FOREIGN KEY (restored_from_version_id) REFERENCES public.entity_version(id) ON UPDATE RESTRICT ON DELETE SET NULL;


--
-- Name: imported_note imported_note_campaign_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.imported_note
    ADD CONSTRAINT imported_note_campaign_id_fkey FOREIGN KEY (campaign_id) REFERENCES public.campaign(id) ON UPDATE CASCADE ON DELETE CASCADE;


--
-- Name: note_proposal note_proposal_created_entity_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.note_proposal
    ADD CONSTRAINT note_proposal_created_entity_id_fkey FOREIGN KEY (created_entity_id) REFERENCES public.entity(id) ON UPDATE CASCADE ON DELETE SET NULL;


--
-- Name: note_proposal note_proposal_existing_entity_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.note_proposal
    ADD CONSTRAINT note_proposal_existing_entity_id_fkey FOREIGN KEY (existing_entity_id) REFERENCES public.entity(id) ON UPDATE CASCADE ON DELETE SET NULL;


--
-- Name: note_proposal note_proposal_note_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.note_proposal
    ADD CONSTRAINT note_proposal_note_id_fkey FOREIGN KEY (note_id) REFERENCES public.imported_note(id) ON UPDATE CASCADE ON DELETE CASCADE;


--
-- Name: player player_campaign_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.player
    ADD CONSTRAINT player_campaign_id_fkey FOREIGN KEY (campaign_id) REFERENCES public.campaign(id) ON UPDATE CASCADE ON DELETE CASCADE;


--
-- Name: sound sound_campaign_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sound
    ADD CONSTRAINT sound_campaign_id_fkey FOREIGN KEY (campaign_id) REFERENCES public.campaign(id) ON UPDATE CASCADE ON DELETE CASCADE;


--
-- PostgreSQL database dump complete
--


