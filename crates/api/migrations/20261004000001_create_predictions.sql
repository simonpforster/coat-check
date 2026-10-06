CREATE EXTENSION IF NOT EXISTS pg_ttl_index;

CREATE TABLE predictions (
    id              UUID PRIMARY KEY,
    recommendation  TEXT NOT NULL,
    reason          TEXT NOT NULL,
    locations_json  JSONB NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ NOT NULL DEFAULT now() + interval '9 days'
);

SELECT ttl_create_index('public.predictions', 'expires_at', 0, 10000);
