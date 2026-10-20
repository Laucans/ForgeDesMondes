-- The NoteImport aggregate (crates/dataguard/aggregates/NoteImport/aggregate.json):
-- the free-text notes the DM pastes to take a campaign up again, and the
-- elements the AI proposes from them, waiting for the DM's validation.
-- Replays as a no-op, as 0001 to 0004 do: the rules' runtime applies it with
-- psql, then the server through the Data layer. Tables and indexes are
-- `IF NOT EXISTS` with their constraints inline, functions `OR REPLACE`, and
-- the trigger is dropped and created again.
--
-- A refusal names its invariant: the error's constraint is the invariant's
-- id in snake case, its message starts with the id itself.

CREATE TABLE IF NOT EXISTS imported_note (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation NoteImport.campaignId: onDelete cascade, onUpdate propagate.
    campaign_id uuid NOT NULL REFERENCES campaign (id) ON DELETE CASCADE ON UPDATE CASCADE,
    raw_text text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    analyzed_at timestamptz
);

-- A campaign's notes are read and cascaded by their campaign.
CREATE INDEX IF NOT EXISTS imported_note_campaign_id ON imported_note (campaign_id);

CREATE TABLE IF NOT EXISTS note_proposal (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation NoteImport.noteId: onDelete cascade, onUpdate propagate.
    note_id uuid NOT NULL REFERENCES imported_note (id) ON DELETE CASCADE ON UPDATE CASCADE,
    type text NOT NULL CONSTRAINT note_proposal_type CHECK (type IN ('pj', 'pnj', 'lieu', 'evenement')),
    name text NOT NULL,
    excerpt text NOT NULL,
    -- Relation NoteImport.existingEntityId: onDelete nullify, onUpdate
    -- propagate.
    existing_entity_id uuid REFERENCES entity (id) ON DELETE SET NULL ON UPDATE CASCADE,
    checked boolean NOT NULL,
    -- Relation NoteImport.createdEntityId: onDelete nullify, onUpdate
    -- propagate.
    created_entity_id uuid REFERENCES entity (id) ON DELETE SET NULL ON UPDATE CASCADE,
    -- no-creation-before-validation: only a checked proposal is created.
    CONSTRAINT no_creation_before_validation CHECK (created_entity_id IS NULL OR checked)
);

-- A note's proposals are read and cascaded by their note; an element's
-- proposals are nullified by it.
CREATE INDEX IF NOT EXISTS note_proposal_note_id ON note_proposal (note_id);
CREATE INDEX IF NOT EXISTS note_proposal_existing_entity_id ON note_proposal (existing_entity_id);
CREATE INDEX IF NOT EXISTS note_proposal_created_entity_id ON note_proposal (created_entity_id);

-- no-creation-before-validation: the analysis creates no element, so a
-- proposal is written with none; the validation creates one for a proposal
-- that has none yet, and only once. What the relations do is no one's
-- validation: the created element's deletion or new id reaching the
-- proposal.
CREATE OR REPLACE FUNCTION no_creation_before_validation() RETURNS trigger
    LANGUAGE plpgsql AS $$
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

DROP TRIGGER IF EXISTS no_creation_before_validation ON note_proposal;
CREATE TRIGGER no_creation_before_validation
    BEFORE INSERT OR UPDATE ON note_proposal
    FOR EACH ROW EXECUTE FUNCTION no_creation_before_validation();
