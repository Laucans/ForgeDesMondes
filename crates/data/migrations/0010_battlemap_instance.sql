-- The BattlemapInstance aggregate's tables
-- (mock/data/aggregates/BattlemapInstance.aggregate.json), as the dossier
-- designs them (mock/data/model.json): a scene's working copy of a
-- battlemap, its tokens, its combat and the combat's initiative, and the
-- order the carousel shows. The aggregate's invariants are laid on these
-- tables with its aggregate.json.
-- Replays as a no-op, as 0001 to 0009 do: the rules' runtime applies it with
-- psql, then the server through the Data layer. Tables and indexes are
-- `IF NOT EXISTS` with their constraints inline, the view `OR REPLACE`, and
-- the foreign key closing the cycle between combat and initiative_entry is
-- dropped and added again.
--
-- Each foreign key's actions are its relation's, as the aggregate declares
-- it: cascade and propagate are CASCADE, nullify is SET NULL. Restrict, and
-- review — a deletion a human approves — refuse a row's deletion while rows
-- point to it, checked when the transaction commits: NO ACTION, deferred, so
-- that a campaign's deletion, which cascades to its media and its elements
-- as to its sessions and their instances, is judged once every row has gone.

CREATE TABLE IF NOT EXISTS battlemap_instance (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation BattlemapInstance.battlemapMediaId: onDelete restrict,
    -- onUpdate propagate.
    battlemap_media_id uuid NOT NULL
        REFERENCES media (id) ON DELETE NO ACTION ON UPDATE CASCADE
        DEFERRABLE INITIALLY DEFERRED,
    -- Relation BattlemapInstance.sessionId: onDelete cascade, onUpdate
    -- propagate.
    session_id uuid NOT NULL REFERENCES session (id) ON DELETE CASCADE ON UPDATE CASCADE,
    -- Relation BattlemapInstance.sceneId: onDelete cascade, onUpdate
    -- propagate.
    scene_id uuid NOT NULL REFERENCES scene (id) ON DELETE CASCADE ON UPDATE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    reset_at timestamptz
);

CREATE INDEX IF NOT EXISTS battlemap_instance_battlemap_media_id
    ON battlemap_instance (battlemap_media_id);
CREATE INDEX IF NOT EXISTS battlemap_instance_session_id ON battlemap_instance (session_id);
CREATE INDEX IF NOT EXISTS battlemap_instance_scene_id ON battlemap_instance (scene_id);

CREATE TABLE IF NOT EXISTS token (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation BattlemapInstance.tokenInstanceId: onDelete cascade, onUpdate
    -- propagate.
    instance_id uuid NOT NULL
        REFERENCES battlemap_instance (id) ON DELETE CASCADE ON UPDATE CASCADE,
    -- Relation BattlemapInstance.characterId: onDelete review, onUpdate
    -- propagate.
    character_id uuid NOT NULL
        REFERENCES character (entity_id) ON DELETE NO ACTION ON UPDATE CASCADE
        DEFERRABLE INITIALLY DEFERRED,
    number smallint,
    name text NOT NULL,
    q integer NOT NULL,
    r integer NOT NULL,
    visible boolean NOT NULL,
    hp_current smallint,
    hp_max smallint,
    conditions text[] NOT NULL,
    -- Generated, never written: dead at 0 HP or less; a PC's untracked HP
    -- never are.
    dead boolean NOT NULL GENERATED ALWAYS AS (coalesce(hp_current <= 0, false)) STORED,
    -- Generated, never written: the one state the players see, none for a
    -- PC's untracked HP.
    health_band text GENERATED ALWAYS AS (
        CASE
            WHEN hp_current IS NULL OR hp_max IS NULL THEN NULL
            WHEN hp_current <= 0 THEN 'mort'
            WHEN hp_current >= hp_max THEN 'indemne'
            WHEN hp_current * 2 > hp_max THEN 'blessé'
            ELSE 'en sang'
        END
    ) STORED,
    -- Generated, never written: the name's first letter, on the default disc.
    label_initial text NOT NULL GENERATED ALWAYS AS (left(name, 1)) STORED
);

CREATE INDEX IF NOT EXISTS token_instance_id ON token (instance_id);
CREATE INDEX IF NOT EXISTS token_character_id ON token (character_id);

CREATE TABLE IF NOT EXISTS combat (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation BattlemapInstance.combatInstanceId: onDelete cascade, onUpdate
    -- propagate.
    instance_id uuid NOT NULL
        REFERENCES battlemap_instance (id) ON DELETE CASCADE ON UPDATE CASCADE,
    round integer NOT NULL,
    -- Its foreign key to initiative_entry is added once that table exists.
    current_entry_id uuid,
    started_at timestamptz NOT NULL,
    ended_at timestamptz
);

CREATE INDEX IF NOT EXISTS combat_instance_id ON combat (instance_id);

CREATE TABLE IF NOT EXISTS initiative_entry (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation BattlemapInstance.initiativeEntryCombatId: onDelete cascade,
    -- onUpdate propagate.
    combat_id uuid NOT NULL REFERENCES combat (id) ON DELETE CASCADE ON UPDATE CASCADE,
    -- Relation BattlemapInstance.initiativeEntryTokenId: onDelete cascade,
    -- onUpdate propagate.
    token_id uuid NOT NULL REFERENCES token (id) ON DELETE CASCADE ON UPDATE CASCADE,
    roll smallint,
    initiative smallint NOT NULL,
    dex_score smallint,
    tiebreak double precision NOT NULL,
    rank integer NOT NULL
);

-- A combat's entries are read in rank order, and cascaded, by their combat.
CREATE INDEX IF NOT EXISTS initiative_entry_combat_id ON initiative_entry (combat_id, rank);
CREATE INDEX IF NOT EXISTS initiative_entry_token_id ON initiative_entry (token_id);

-- Relation BattlemapInstance.currentEntryId: onDelete nullify, onUpdate
-- propagate. Under the name an inline REFERENCES would have given it.
ALTER TABLE combat DROP CONSTRAINT IF EXISTS combat_current_entry_id_fkey;
ALTER TABLE combat ADD CONSTRAINT combat_current_entry_id_fkey
    FOREIGN KEY (current_entry_id) REFERENCES initiative_entry (id)
    ON DELETE SET NULL ON UPDATE CASCADE;

CREATE INDEX IF NOT EXISTS combat_current_entry_id ON combat (current_entry_id);

-- The carousel's order, never stored: each combat's entries by rank, a
-- hidden token flagged for the DM's side alone, a dead one to be skipped.
CREATE OR REPLACE VIEW initiative_order_view AS
    SELECT e.combat_id, e.id AS entry_id,
           (row_number() OVER (PARTITION BY e.combat_id ORDER BY e.rank, e.id))::integer
               AS order_index,
           t.visible AS shown_to_players,
           t.dead AS skipped
        FROM initiative_entry e
        JOIN token t ON t.id = e.token_id;
