use chrono::{DateTime, Utc};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum FeedbackStoreError {
    #[error("database error: {0}")]
    Database(String),
}

#[derive(Clone)]
pub struct FeedbackRow {
    pub prediction_id: Uuid,
    pub recommendation: String,
    pub prediction_at: DateTime<Utc>,
    pub feedback_brought: String,
    pub feedback_should_have_brought: String,
    pub feedback_comment: Option<String>,
    pub feedback_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct FeedbackTotals {
    pub total: i64,
    pub matched: i64,
}

#[derive(Clone)]
pub struct FeedbackDetailRow {
    pub prediction_id: Uuid,
    pub recommendation: String,
    pub reason: String,
    pub locations_json: serde_json::Value,
    pub actual_weather_json: Option<serde_json::Value>,
    pub prediction_at: DateTime<Utc>,
    pub feedback_brought: String,
    pub feedback_should_have_brought: String,
    pub feedback_comment: Option<String>,
    pub feedback_at: DateTime<Utc>,
}

#[async_trait::async_trait]
pub trait FeedbackStorePort: Send + Sync {
    async fn get_totals(&self) -> Result<FeedbackTotals, FeedbackStoreError>;

    async fn list_feedback(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<FeedbackRow>, FeedbackStoreError>;

    async fn get_feedback_detail(
        &self,
        prediction_id: Uuid,
    ) -> Result<Option<FeedbackDetailRow>, FeedbackStoreError>;
}
