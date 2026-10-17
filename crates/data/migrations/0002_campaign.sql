-- The Campaign aggregate (crates/dataguard/aggregates/Campaign/aggregate.json):
-- the DM's campaigns and oneshots, every one visible at home, no account.
-- Replays as a no-op, as 0001 does: the rules' runtime applies it with psql,
-- then the server through the Data layer. Every constraint is inline, so
-- `IF NOT EXISTS` covers it too.
--
-- cover_media_id is a plain column: its foreign key to media.id (onDelete
-- nullify, onUpdate propagate) comes with the Media aggregate's table.
-- origin_campaign_id is never one: the original may not exist on this
-- machine (import-creates-copy).

CREATE TABLE IF NOT EXISTS campaign (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL,
    -- oneshot-single-session is judged on a oneshot: the kind says which.
    kind text NOT NULL CONSTRAINT campaign_kind CHECK (kind IN ('campagne', 'oneshot')),
    pitch text,
    art text,
    glyph text,
    cover_media_id uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_exported_at timestamptz,
    imported_at timestamptz,
    origin_campaign_id uuid,
    -- master-volume-range: between 0 and 1, full volume until the DM moves it.
    sound_master_volume numeric(3,2) NOT NULL DEFAULT 1
        CONSTRAINT campaign_master_volume_range CHECK (sound_master_volume BETWEEN 0 AND 1)
);
