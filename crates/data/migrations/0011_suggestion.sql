-- The Suggestion aggregate's table
-- (mock/data/aggregates/Suggestion.aggregate.json), as the dossier designs it
-- (mock/data/model.json): each element or sound the microphone recognised
-- during a live session, added and never rewritten — the pill's pile and its
-- history are read from them. The aggregate's invariants are laid on it with
-- its aggregate.json.
-- Replays as a no-op, as 0001 to 0010 do: the rules' runtime applies it with
-- psql, then the server through the Data layer. The table and its indexes are
-- `IF NOT EXISTS` with their constraints inline.

CREATE TABLE IF NOT EXISTS live_suggestion (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation Suggestion.sessionId: onDelete cascade, onUpdate propagate.
    session_id uuid NOT NULL REFERENCES session (id) ON DELETE CASCADE ON UPDATE CASCADE,
    -- Relation Suggestion.entityId: onDelete cascade, onUpdate propagate.
    entity_id uuid REFERENCES entity (id) ON DELETE CASCADE ON UPDATE CASCADE,
    -- Relation Suggestion.soundId: onDelete cascade, onUpdate propagate.
    sound_id uuid REFERENCES sound (id) ON DELETE CASCADE ON UPDATE CASCADE,
    matched_text text NOT NULL,
    detected_at timestamptz NOT NULL
);

-- A session's suggestions are read newest first, and cascaded, by their
-- session; an element's or a sound's cascaded by it.
CREATE INDEX IF NOT EXISTS live_suggestion_session_id
    ON live_suggestion (session_id, detected_at DESC);
CREATE INDEX IF NOT EXISTS live_suggestion_entity_id ON live_suggestion (entity_id);
CREATE INDEX IF NOT EXISTS live_suggestion_sound_id ON live_suggestion (sound_id);
