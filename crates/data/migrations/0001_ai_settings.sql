-- The AiSettings aggregate (crates/dataguard/aggregates/AiSettings/aggregate.json):
-- the providers the DM can choose, and the one settings row naming the active
-- one. Replays as a no-op: the rules' runtime applies it with psql, then the
-- server through the Data layer, and nothing records the first run for the
-- second. Every constraint is inline, so `IF NOT EXISTS` covers it too.

CREATE TABLE IF NOT EXISTS ai_provider (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    mode text NOT NULL,
    label text NOT NULL,
    url text,
    detected_api_type text,
    api_key text,
    "binary" text,
    generation_model text NOT NULL,
    summary_model text NOT NULL,
    last_test_ok boolean,
    last_test_at timestamptz,
    last_test_message text,
    last_test_latency_ms integer,
    -- mode-fields: an api row has its url and detected type and no binary; a
    -- binary row names opencode or claude-code and has no url.
    CONSTRAINT ai_provider_mode_fields CHECK (
        (mode = 'api'
            AND coalesce(url, '') <> ''
            AND coalesce(detected_api_type, '') <> ''
            AND coalesce("binary", '') = '')
        OR (mode = 'binaire'
            AND coalesce("binary", '') IN ('opencode', 'claude-code')
            AND coalesce(url, '') = '')
    ),
    -- api-type-recognized: the types of ProviderApiType@1, and none other.
    CONSTRAINT ai_provider_api_type_recognized CHECK (
        detected_api_type IS NULL OR detected_api_type IN ('cloud', 'opencode')
    )
);

-- single-settings-row: one row, id = 1, naming a provider that exists. A
-- provider it names cannot be deleted (onDelete restrict); a new id follows
-- (onUpdate propagate).
CREATE TABLE IF NOT EXISTS ai_settings (
    id smallint PRIMARY KEY DEFAULT 1 CONSTRAINT ai_settings_single_row CHECK (id = 1),
    active_provider_id uuid NOT NULL
        REFERENCES ai_provider (id) ON DELETE RESTRICT ON UPDATE CASCADE,
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- One row per selectable provider, each valid under mode-fields. No key: the
-- DM enters theirs, and no secret is written here.
INSERT INTO ai_provider (id, mode, label, url, detected_api_type, "binary", generation_model, summary_model)
VALUES
    ('a1000000-0000-4000-8000-000000000001', 'api', 'API cloud',
        'https://api.anthropic.com/v1', 'cloud', NULL,
        'claude-sonnet-5-5', 'claude-haiku-4-5-20251001'),
    ('a1000000-0000-4000-8000-000000000002', 'api', 'Endpoint opencode du serveur maison',
        'http://192.168.1.40:4096', 'opencode', NULL,
        'qwen2.5-32b-instruct', 'qwen2.5-14b-instruct'),
    ('a1000000-0000-4000-8000-000000000003', 'binaire', 'Binaire opencode',
        NULL, NULL, 'opencode',
        'qwen2.5-32b-instruct', 'qwen2.5-14b-instruct'),
    ('a1000000-0000-4000-8000-000000000004', 'binaire', 'Binaire Claude Code',
        NULL, NULL, 'claude-code',
        'claude-sonnet-5-5', 'claude-haiku-4-5-20251001')
ON CONFLICT (id) DO NOTHING;

-- The cloud provider is active from the first start.
INSERT INTO ai_settings (id, active_provider_id)
VALUES (1, 'a1000000-0000-4000-8000-000000000001')
ON CONFLICT (id) DO NOTHING;
