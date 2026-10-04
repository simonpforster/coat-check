use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    domain::prediction::{Feedback, Prediction, PredictionLocation},
    ports::outbound::{FeedbackStoreError, FeedbackStorePort, QueuedNotification},
};

const MAX_SEND_ATTEMPTS: i64 = 5;

#[derive(Clone)]
pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl FeedbackStorePort for PgStore {
    async fn save_prediction(
        &self,
        recommendation: &str,
        reason: &str,
        locations: &[PredictionLocation],
    ) -> Result<Uuid, FeedbackStoreError> {
        let id = Uuid::new_v4();
        let locations_json = serde_json::to_value(locations)
            .map_err(|e| FeedbackStoreError::Database(e.to_string()))?;

        sqlx::query(
            "INSERT INTO predictions (id, recommendation, reason, locations_json) VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(recommendation)
        .bind(reason)
        .bind(&locations_json)
        .execute(&self.pool)
        .await
        .map_err(|e| FeedbackStoreError::Database(e.to_string()))?;

        Ok(id)
    }

    async fn get_prediction(&self, id: Uuid) -> Result<Prediction, FeedbackStoreError> {
        let row = sqlx::query_as::<_, PredictionRow>(
            "SELECT id, recommendation, reason, locations_json, created_at FROM predictions WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FeedbackStoreError::Database(e.to_string()))?
        .ok_or(FeedbackStoreError::NotFound)?;

        let locations: Vec<PredictionLocation> = serde_json::from_value(row.locations_json)
            .map_err(|e| FeedbackStoreError::Database(e.to_string()))?;

        Ok(Prediction {
            id: row.id,
            recommendation: row.recommendation,
            reason: row.reason,
            locations,
            created_at: row.created_at,
        })
    }

    async fn enqueue_notification(
        &self,
        prediction_id: Uuid,
        contact: &str,
        send_after: DateTime<Utc>,
    ) -> Result<(), FeedbackStoreError> {
        sqlx::query(
            "INSERT INTO email_queue (prediction_id, email, send_after) VALUES ($1, $2, $3)",
        )
        .bind(prediction_id)
        .bind(contact)
        .bind(send_after)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(ref db_err) = e {
                if db_err.code().as_deref() == Some("23505") {
                    return FeedbackStoreError::AlreadyExists;
                }
            }
            FeedbackStoreError::Database(e.to_string())
        })?;

        Ok(())
    }

    async fn fetch_ready_notifications(
        &self,
    ) -> Result<Vec<QueuedNotification>, FeedbackStoreError> {
        let rows = sqlx::query_as::<_, NotificationQueueRow>(
            "UPDATE email_queue SET status = 'sending' \
             WHERE id IN ( \
                 SELECT id FROM email_queue \
                 WHERE status = 'pending' AND send_after <= now() \
                 ORDER BY send_after LIMIT 50 \
                 FOR UPDATE SKIP LOCKED \
             ) \
             RETURNING id, prediction_id, email AS contact",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| FeedbackStoreError::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| QueuedNotification {
                id: r.id,
                prediction_id: r.prediction_id,
                contact: r.contact,
            })
            .collect())
    }

    async fn delete_notification(&self, id: Uuid) -> Result<(), FeedbackStoreError> {
        sqlx::query("DELETE FROM email_queue WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| FeedbackStoreError::Database(e.to_string()))?;

        Ok(())
    }

    async fn mark_notification_sent(&self, id: Uuid) -> Result<(), FeedbackStoreError> {
        sqlx::query("UPDATE email_queue SET status = 'sent', sent_at = now() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| FeedbackStoreError::Database(e.to_string()))?;

        Ok(())
    }

    async fn mark_notification_failed(
        &self,
        id: Uuid,
        error: &str,
    ) -> Result<(), FeedbackStoreError> {
        sqlx::query(
            "UPDATE email_queue SET \
                attempts = attempts + 1, \
                last_error = $2, \
                status = CASE WHEN attempts + 1 >= $3 THEN 'failed' ELSE 'pending' END, \
                send_after = now() + make_interval(mins => power(2, attempts)::int) \
             WHERE id = $1",
        )
        .bind(id)
        .bind(error)
        .bind(MAX_SEND_ATTEMPTS)
        .execute(&self.pool)
        .await
        .map_err(|e| FeedbackStoreError::Database(e.to_string()))?;

        Ok(())
    }

    async fn get_notification_sent_at(
        &self,
        prediction_id: Uuid,
    ) -> Result<DateTime<Utc>, FeedbackStoreError> {
        let row: Option<(DateTime<Utc>,)> = sqlx::query_as(
            "SELECT sent_at FROM email_queue WHERE prediction_id = $1 AND status = 'sent' AND sent_at IS NOT NULL LIMIT 1",
        )
        .bind(prediction_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FeedbackStoreError::Database(e.to_string()))?;

        row.map(|r| r.0).ok_or(FeedbackStoreError::NotFound)
    }

    async fn record_feedback(
        &self,
        prediction: &Prediction,
        feedback: &Feedback,
        actual_weather: Option<&[PredictionLocation]>,
    ) -> Result<(), FeedbackStoreError> {
        let locations_json = serde_json::to_value(&prediction.locations)
            .map_err(|e| FeedbackStoreError::Database(e.to_string()))?;

        let actual_json = actual_weather
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| FeedbackStoreError::Database(e.to_string()))?;

        sqlx::query(
            "INSERT INTO analytics (prediction_id, recommendation, reason, locations_json, prediction_at, feedback_accurate, feedback_comment, actual_weather_json) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(prediction.id)
        .bind(&prediction.recommendation)
        .bind(&prediction.reason)
        .bind(&locations_json)
        .bind(prediction.created_at)
        .bind(feedback.accurate)
        .bind(feedback.comment.as_deref())
        .bind(&actual_json)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(ref db_err) = e {
                if db_err.code().as_deref() == Some("23505") {
                    return FeedbackStoreError::AlreadyExists;
                }
            }
            FeedbackStoreError::Database(e.to_string())
        })?;

        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct PredictionRow {
    id: Uuid,
    recommendation: String,
    reason: String,
    locations_json: serde_json::Value,
    created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct NotificationQueueRow {
    id: Uuid,
    prediction_id: Uuid,
    contact: String,
}
