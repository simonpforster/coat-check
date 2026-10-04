ALTER TABLE email_queue ADD CONSTRAINT uq_email_queue_prediction_contact UNIQUE (prediction_id, email);
