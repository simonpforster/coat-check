use sqlx::PgPool;

use crate::ports::outbound::{
    FeedbackDetailRow, FeedbackRow, FeedbackStoreError, FeedbackStorePort, FeedbackTotals,
};

#[derive(sqlx::FromRow)]
struct PgFeedbackRow {
    prediction_id: uuid::Uuid,
    recommendation: String,
    prediction_at: chrono::DateTime<chrono::Utc>,
    feedback_brought: String,
    feedback_should_have_brought: String,
    feedback_comment: Option<String>,
    feedback_at: chrono::DateTime<chrono::Utc>,
}

#[derive(sqlx::FromRow)]
struct PgFeedbackDetailRow {
    prediction_id: uuid::Uuid,
    recommendation: String,
    reason: String,
    locations_json: serde_json::Value,
    actual_weather_json: Option<serde_json::Value>,
    prediction_at: chrono::DateTime<chrono::Utc>,
    feedback_brought: String,
    feedback_should_have_brought: String,
    feedback_comment: Option<String>,
    feedback_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone)]
pub struct PgFeedbackStore {
    pool: PgPool,
}

impl PgFeedbackStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl FeedbackStorePort for PgFeedbackStore {
    async fn get_pending_notification_count(&self) -> Result<i64, FeedbackStoreError> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM email_queue WHERE status IN ('pending', 'sending')",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| FeedbackStoreError::Database(e.to_string()))?;

        Ok(count)
    }

    async fn get_prediction_count(&self) -> Result<i64, FeedbackStoreError> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM predictions")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| FeedbackStoreError::Database(e.to_string()))?;

        Ok(count)
    }

    async fn get_totals(&self) -> Result<FeedbackTotals, FeedbackStoreError> {
        let (total, matched): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), \
             COUNT(*) FILTER (WHERE feedback_brought = feedback_should_have_brought) \
             FROM analytics",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| FeedbackStoreError::Database(e.to_string()))?;

        Ok(FeedbackTotals { total, matched })
    }

    async fn list_feedback(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<FeedbackRow>, FeedbackStoreError> {
        let rows = sqlx::query_as::<_, PgFeedbackRow>(
            "SELECT prediction_id, recommendation, prediction_at, \
             feedback_brought, feedback_should_have_brought, feedback_comment, feedback_at \
             FROM analytics ORDER BY feedback_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| FeedbackStoreError::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| FeedbackRow {
                prediction_id: r.prediction_id,
                recommendation: r.recommendation,
                prediction_at: r.prediction_at,
                feedback_brought: r.feedback_brought,
                feedback_should_have_brought: r.feedback_should_have_brought,
                feedback_comment: r.feedback_comment,
                feedback_at: r.feedback_at,
            })
            .collect())
    }

    async fn get_feedback_detail(
        &self,
        prediction_id: uuid::Uuid,
    ) -> Result<Option<FeedbackDetailRow>, FeedbackStoreError> {
        let row = sqlx::query_as::<_, PgFeedbackDetailRow>(
            "SELECT prediction_id, recommendation, reason, locations_json, \
             actual_weather_json, prediction_at, feedback_brought, \
             feedback_should_have_brought, feedback_comment, feedback_at \
             FROM analytics WHERE prediction_id = $1",
        )
        .bind(prediction_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FeedbackStoreError::Database(e.to_string()))?;

        Ok(row.map(|r| FeedbackDetailRow {
            prediction_id: r.prediction_id,
            recommendation: r.recommendation,
            reason: r.reason,
            locations_json: r.locations_json,
            actual_weather_json: r.actual_weather_json,
            prediction_at: r.prediction_at,
            feedback_brought: r.feedback_brought,
            feedback_should_have_brought: r.feedback_should_have_brought,
            feedback_comment: r.feedback_comment,
            feedback_at: r.feedback_at,
        }))
    }
}
