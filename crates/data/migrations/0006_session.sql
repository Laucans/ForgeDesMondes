-- The Session aggregate's tables (mock/data/aggregates/Session.aggregate.json),
-- as the dossier designs them (mock/data/model.json): a campaign's sessions,
-- the PCs present at each, its ordered scenes and what each scene links,
-- the oral briefs, the « à créer » list, the session's journal, and the two
-- views computed from them. The aggregate's invariants are laid on these
-- tables with its aggregate.json.
-- Replays as a no-op, as 0001 to 0005 do: the rules' runtime applies it with
-- psql, then the server through the Data layer. Tables and indexes are
-- `IF NOT EXISTS` with their constraints inline, views `OR REPLACE`.
--
-- Each foreign key's actions are its relation's, as the aggregate declares
-- it: cascade and propagate are CASCADE, nullify is SET NULL, restrict is
-- RESTRICT. A text column taking a set of values is checked against it,
-- under the name <table>_<column>.

CREATE TABLE IF NOT EXISTS session (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation Session.sessionCampaignId: onDelete cascade, onUpdate propagate.
    campaign_id uuid NOT NULL REFERENCES campaign (id) ON DELETE CASCADE ON UPDATE CASCADE,
    position integer NOT NULL,
    title text NOT NULL,
    planned_on date,
    status text NOT NULL CONSTRAINT session_status CHECK (status IN ('prep', 'live', 'closed')),
    started_at timestamptz,
    closed_at timestamptz,
    microphone_on boolean NOT NULL,
    summary text,
    summary_model text,
    summary_edited_at timestamptz
);

-- A campaign's sessions are listed, and cascaded, by their campaign.
CREATE INDEX IF NOT EXISTS session_campaign_id ON session (campaign_id);

-- A PC is checked present once per session.
CREATE TABLE IF NOT EXISTS session_present_character (
    -- Relation Session.sessionPresentCharacterSessionId: onDelete cascade,
    -- onUpdate propagate.
    session_id uuid NOT NULL REFERENCES session (id) ON DELETE CASCADE ON UPDATE CASCADE,
    -- Relation Session.sessionPresentCharacterCharacterId: onDelete cascade,
    -- onUpdate propagate.
    character_id uuid NOT NULL
        REFERENCES character (entity_id) ON DELETE CASCADE ON UPDATE CASCADE,
    added_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (session_id, character_id)
);

CREATE INDEX IF NOT EXISTS session_present_character_character_id
    ON session_present_character (character_id);

CREATE TABLE IF NOT EXISTS scene (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation Session.sceneSessionId: onDelete cascade, onUpdate propagate.
    session_id uuid NOT NULL REFERENCES session (id) ON DELETE CASCADE ON UPDATE CASCADE,
    position integer NOT NULL,
    title text NOT NULL,
    notes text
);

-- A session's scenes are read in order, and cascaded, by their session.
CREATE INDEX IF NOT EXISTS scene_session_id ON scene (session_id, position);

-- An element is linked once per scene.
CREATE TABLE IF NOT EXISTS scene_entity (
    -- Relation Session.sceneEntitySceneId: onDelete cascade, onUpdate
    -- propagate.
    scene_id uuid NOT NULL REFERENCES scene (id) ON DELETE CASCADE ON UPDATE CASCADE,
    -- Relation Session.entityId: onDelete cascade, onUpdate propagate.
    entity_id uuid NOT NULL REFERENCES entity (id) ON DELETE CASCADE ON UPDATE CASCADE,
    position integer NOT NULL,
    PRIMARY KEY (scene_id, entity_id)
);

CREATE INDEX IF NOT EXISTS scene_entity_entity_id ON scene_entity (entity_id);

-- A media is linked once per scene.
CREATE TABLE IF NOT EXISTS scene_media (
    -- Relation Session.sceneMediaSceneId: onDelete cascade, onUpdate
    -- propagate.
    scene_id uuid NOT NULL REFERENCES scene (id) ON DELETE CASCADE ON UPDATE CASCADE,
    -- Relation Session.sceneMediaMediaId: onDelete cascade, onUpdate
    -- propagate.
    media_id uuid NOT NULL REFERENCES media (id) ON DELETE CASCADE ON UPDATE CASCADE,
    PRIMARY KEY (scene_id, media_id)
);

CREATE INDEX IF NOT EXISTS scene_media_media_id ON scene_media (media_id);

-- Text only: no column holds audio.
CREATE TABLE IF NOT EXISTS oral_brief (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation Session.oralBriefSessionId: onDelete cascade, onUpdate
    -- propagate.
    session_id uuid NOT NULL REFERENCES session (id) ON DELETE CASCADE ON UPDATE CASCADE,
    transcript text NOT NULL,
    duration_seconds integer NOT NULL,
    recorded_at timestamptz NOT NULL,
    analyzed_at timestamptz
);

CREATE INDEX IF NOT EXISTS oral_brief_session_id ON oral_brief (session_id);

CREATE TABLE IF NOT EXISTS entity_to_create (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation Session.entityToCreateCampaignId: onDelete cascade, onUpdate
    -- propagate.
    campaign_id uuid NOT NULL REFERENCES campaign (id) ON DELETE CASCADE ON UPDATE CASCADE,
    -- Relation Session.entityToCreateSessionId: onDelete cascade, onUpdate
    -- propagate.
    session_id uuid REFERENCES session (id) ON DELETE CASCADE ON UPDATE CASCADE,
    -- Relation Session.briefId: onDelete nullify, onUpdate propagate.
    brief_id uuid REFERENCES oral_brief (id) ON DELETE SET NULL ON UPDATE CASCADE,
    -- Relation Session.entityToCreateSceneId: onDelete nullify, onUpdate
    -- propagate.
    scene_id uuid REFERENCES scene (id) ON DELETE SET NULL ON UPDATE CASCADE,
    type text NOT NULL CONSTRAINT entity_to_create_type CHECK (type IN ('pnj', 'lieu', 'evenement')),
    name text NOT NULL,
    excerpt text NOT NULL,
    refinement_text text,
    status text NOT NULL CONSTRAINT entity_to_create_status CHECK (status IN ('a_creer', 'creee')),
    -- Relation Session.createdEntityId: onDelete nullify, onUpdate propagate.
    created_entity_id uuid REFERENCES entity (id) ON DELETE SET NULL ON UPDATE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now()
);

-- A campaign's list is read, and cascaded, by its campaign; the others are
-- found, and cascaded or nullified, by what they point to.
CREATE INDEX IF NOT EXISTS entity_to_create_campaign_id ON entity_to_create (campaign_id);
CREATE INDEX IF NOT EXISTS entity_to_create_session_id ON entity_to_create (session_id);
CREATE INDEX IF NOT EXISTS entity_to_create_brief_id ON entity_to_create (brief_id);
CREATE INDEX IF NOT EXISTS entity_to_create_scene_id ON entity_to_create (scene_id);
CREATE INDEX IF NOT EXISTS entity_to_create_created_entity_id ON entity_to_create (created_entity_id);

-- Text only: no column holds audio.
CREATE TABLE IF NOT EXISTS session_journal_line (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation Session.sessionJournalLineSessionId: onDelete cascade,
    -- onUpdate restrict.
    session_id uuid NOT NULL REFERENCES session (id) ON DELETE CASCADE ON UPDATE RESTRICT,
    -- Relation Session.sessionJournalLineSceneId: onDelete nullify, onUpdate
    -- propagate.
    scene_id uuid REFERENCES scene (id) ON DELETE SET NULL ON UPDATE CASCADE,
    spoken_at timestamptz NOT NULL,
    text text NOT NULL
);

-- A session's journal is read in time order, and cascaded, by its session.
CREATE INDEX IF NOT EXISTS session_journal_line_session_id
    ON session_journal_line (session_id, spoken_at);
CREATE INDEX IF NOT EXISTS session_journal_line_scene_id ON session_journal_line (scene_id);

-- The characters a session offers in the DM view and on its battlemaps,
-- never stored: the PCs checked present, and the NPCs linked to its scenes.
CREATE OR REPLACE VIEW session_roster_view AS
    SELECT p.session_id, p.character_id, 'present'::text AS via
        FROM session_present_character p
    UNION
    SELECT s.session_id, l.entity_id, 'scene'::text
        FROM scene_entity l
        JOIN scene s ON s.id = l.scene_id
        JOIN entity e ON e.id = l.entity_id
        WHERE e.type = 'pnj';

-- The media of a session, never stored, so that it follows every change of
-- place: those linked to one of its scenes, with no place, and those of a
-- place one of its scenes links, with that place. Once each: a media linked
-- to a scene and at a place of another is a direct link.
CREATE OR REPLACE VIEW session_media_view AS
    SELECT DISTINCT ON (session_id, media_id) session_id, media_id, via_place_id
        FROM (
            SELECT s.session_id, m.media_id, NULL::uuid AS via_place_id
                FROM scene_media m
                JOIN scene s ON s.id = m.scene_id
            UNION ALL
            SELECT s.session_id, m.id, m.place_id
                FROM scene_entity l
                JOIN scene s ON s.id = l.scene_id
                JOIN media m ON m.place_id = l.entity_id
        ) associated
        ORDER BY session_id, media_id, via_place_id NULLS FIRST;
