CREATE TABLE email_queue (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    prediction_id   UUID NOT NULL REFERENCES predictions(id),
    email           TEXT NOT NULL,
    send_after      TIMESTAMPTZ NOT NULL,
    attempts        INTEGER NOT NULL DEFAULT 0,
    last_error      TEXT,
    status          TEXT NOT NULL DEFAULT 'pending',
    sent_at         TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_email_queue_prediction_contact UNIQUE (prediction_id, email)
);

CREATE INDEX idx_email_queue_pending ON email_queue (send_after) WHERE status = 'pending';
