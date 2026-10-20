-- The Sound aggregate (crates/dataguard/aggregates/Sound/aggregate.json):
-- the soundbox's tagged sound effects, each with its own volume.
-- Replays as a no-op, as 0001 to 0004 do: the rules' runtime applies it with
-- psql, then the server through the Data layer. The table and its index are
-- `IF NOT EXISTS` with their constraints inline, the function `OR REPLACE`.
--
-- Both invariants are judged by the row's own columns, as CHECKs. A refusal
-- names its invariant: the constraint is the invariant's id in snake case.
-- No column stores the effective volume: volume × the campaign's
-- sound_master_volume (EffectiveVolume@1), computed by whoever plays it.

-- tags-normalized: one dimension, every tag in lowercase, none twice, none
-- empty of a value (NULL).
CREATE OR REPLACE FUNCTION sound_tags_normalized(tags text[]) RETURNS boolean
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN coalesce(array_ndims(tags), 1) = 1
        AND NOT EXISTS (SELECT FROM unnest(tags) AS tag WHERE tag IS NULL OR tag <> lower(tag))
        AND cardinality(tags) = (SELECT count(DISTINCT tag) FROM unnest(tags) AS tag);

CREATE TABLE IF NOT EXISTS sound (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation Sound.campaignId: onDelete cascade, onUpdate propagate.
    campaign_id uuid NOT NULL REFERENCES campaign (id) ON DELETE CASCADE ON UPDATE CASCADE,
    name text NOT NULL,
    file_path text NOT NULL,
    tags text[] NOT NULL DEFAULT '{}'
        CONSTRAINT tags_normalized CHECK (sound_tags_normalized(tags)),
    -- sound-volume-range: between 0 and 1, full volume until the DM moves it.
    volume numeric(3,2) NOT NULL DEFAULT 1
        CONSTRAINT sound_volume_range CHECK (volume BETWEEN 0 AND 1),
    created_at timestamptz NOT NULL DEFAULT now()
);

-- A campaign's sounds are read, and cascaded, by their campaign.
CREATE INDEX IF NOT EXISTS sound_campaign_id ON sound (campaign_id);
