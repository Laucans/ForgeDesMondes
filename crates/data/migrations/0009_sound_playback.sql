-- The SoundPlayback aggregate's table
-- (mock/data/aggregates/SoundPlayback.aggregate.json), as the dossier designs
-- it (mock/data/model.json): each playing of a sound during a session, on the
-- DM's side alone, several at once. The aggregate's invariants are laid on
-- it with its aggregate.json.
-- Replays as a no-op, as 0001 to 0008 do: the rules' runtime applies it with
-- psql, then the server through the Data layer. The table and its indexes are
-- `IF NOT EXISTS` with their constraints inline.

CREATE TABLE IF NOT EXISTS sound_playback (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation SoundPlayback.sessionId: onDelete cascade, onUpdate propagate.
    session_id uuid NOT NULL REFERENCES session (id) ON DELETE CASCADE ON UPDATE CASCADE,
    -- Relation SoundPlayback.soundId: onDelete cascade, onUpdate propagate.
    sound_id uuid NOT NULL REFERENCES sound (id) ON DELETE CASCADE ON UPDATE CASCADE,
    origin text NOT NULL CONSTRAINT sound_playback_origin CHECK (origin IN ('soundbox', 'pastille')),
    started_at timestamptz NOT NULL,
    stopped_at timestamptz
);

-- A session's playbacks are read, stopped all at once, and cascaded by their
-- session; a sound's cascaded by it.
CREATE INDEX IF NOT EXISTS sound_playback_session_id ON sound_playback (session_id);
CREATE INDEX IF NOT EXISTS sound_playback_sound_id ON sound_playback (sound_id);
