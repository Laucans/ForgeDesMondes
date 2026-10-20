-- The Diffusion aggregate's table
-- (mock/data/aggregates/Diffusion.aggregate.json), as the dossier designs it
-- (mock/data/model.json): what a session's player view shows — its
-- background, its floating window and its framing — one row per session,
-- which the projected player view and the DM's tab both read. The
-- aggregate's invariants are laid on it with its aggregate.json.
-- Replays as a no-op, as 0001 to 0011 do: the rules' runtime applies it with
-- psql, then the server through the Data layer. The table and its indexes are
-- `IF NOT EXISTS` with their constraints inline.

CREATE TABLE IF NOT EXISTS diffusion (
    -- Relation Diffusion.sessionId: onDelete cascade, onUpdate propagate. One
    -- row per session.
    session_id uuid PRIMARY KEY REFERENCES session (id) ON DELETE CASCADE ON UPDATE CASCADE,
    base_kind text NOT NULL
        CONSTRAINT diffusion_base_kind CHECK (base_kind IN ('cover', 'illustration', 'battlemap')),
    -- Relation Diffusion.baseMediaId: onDelete nullify, onUpdate propagate.
    base_media_id uuid REFERENCES media (id) ON DELETE SET NULL ON UPDATE CASCADE,
    -- Relation Diffusion.baseInstanceId: onDelete nullify, onUpdate propagate.
    base_instance_id uuid
        REFERENCES battlemap_instance (id) ON DELETE SET NULL ON UPDATE CASCADE,
    -- Relation Diffusion.windowMediaId: onDelete nullify, onUpdate propagate.
    window_media_id uuid REFERENCES media (id) ON DELETE SET NULL ON UPDATE CASCADE,
    window_x numeric(6,4),
    window_y numeric(6,4),
    window_width numeric(6,4),
    view_zoom numeric(6,3) NOT NULL,
    view_pan_x numeric(9,2) NOT NULL,
    view_pan_y numeric(9,2) NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- A media's or an instance's deletion, or its new id, finds what shows it.
CREATE INDEX IF NOT EXISTS diffusion_base_media_id ON diffusion (base_media_id);
CREATE INDEX IF NOT EXISTS diffusion_base_instance_id ON diffusion (base_instance_id);
CREATE INDEX IF NOT EXISTS diffusion_window_media_id ON diffusion (window_media_id);
