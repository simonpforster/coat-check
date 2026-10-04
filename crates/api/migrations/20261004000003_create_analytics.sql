CREATE TABLE analytics (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    prediction_id         UUID NOT NULL UNIQUE,
    recommendation        TEXT NOT NULL,
    reason                TEXT NOT NULL,
    locations_json        JSONB NOT NULL,
    actual_weather_json   JSONB,
    prediction_at         TIMESTAMPTZ NOT NULL,
    feedback_accurate     BOOLEAN NOT NULL,
    feedback_comment      TEXT,
    feedback_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);
