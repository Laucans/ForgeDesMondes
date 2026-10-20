-- The Player aggregate (crates/dataguard/aggregates/Player/aggregate.json):
-- the real people at the DM's table, kept apart from the campaign's elements.
-- Replays as a no-op, as 0001 and 0002 do: the rules' runtime applies it with
-- psql, then the server through the Data layer. Every constraint is inline,
-- so `IF NOT EXISTS` covers it too.
--
-- player-not-character: a player has a name and notes, never a sheet or a
-- token. A PC points to its player (character.player_id, with the Entity
-- aggregate's table); no column here points to a character.

CREATE TABLE IF NOT EXISTS player (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Relation Player.campaignId: onDelete cascade, onUpdate propagate.
    campaign_id uuid NOT NULL REFERENCES campaign (id) ON DELETE CASCADE ON UPDATE CASCADE,
    name text NOT NULL,
    notes text,
    created_at timestamptz NOT NULL DEFAULT now()
);

-- A campaign's players are read, and cascaded, by their campaign.
CREATE INDEX IF NOT EXISTS player_campaign_id ON player (campaign_id);
