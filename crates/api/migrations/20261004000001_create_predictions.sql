CREATE TABLE predictions (
    id              UUID PRIMARY KEY,
    recommendation  TEXT NOT NULL,
    reason          TEXT NOT NULL,
    locations_json  JSONB NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
