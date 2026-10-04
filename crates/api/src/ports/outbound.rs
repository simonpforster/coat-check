use chrono::{DateTime, Utc};
use thiserror::Error;
use uuid::Uuid;

use chrono::NaiveDate;

use crate::domain::{
    location::Location,
    prediction::{Feedback, Prediction, PredictionLocation},
    weather::DailyForecast,
};

// ── Weather port ───────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum WeatherPortError {
    #[error("network error: {0}")]
    Network(String),
    #[error("upstream API error (status {status}): {body}")]
    Upstream { status: u16, body: String },
    #[error("response parse error: {0}")]
    Parse(String),
}

#[async_trait::async_trait]
pub trait WeatherPort: Send + Sync {
    async fn fetch_daily_forecast(
        &self,
        location: &Location,
    ) -> Result<DailyForecast, WeatherPortError>;

    async fn fetch_daily_observation(
        &self,
        location: &Location,
        date: NaiveDate,
    ) -> Result<DailyForecast, WeatherPortError>;
}

// ── Feedback store port ────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum FeedbackStoreError {
    #[error("database error: {0}")]
    Database(String),
    #[error("prediction not found")]
    NotFound,
    #[error("record already exists")]
    AlreadyExists,
}

#[derive(Clone)]
pub struct QueuedNotification {
    pub id: Uuid,
    pub prediction_id: Uuid,
    pub contact: String,
}

#[async_trait::async_trait]
pub trait FeedbackStorePort: Send + Sync {
    async fn save_prediction(
        &self,
        recommendation: &str,
        reason: &str,
        locations: &[PredictionLocation],
    ) -> Result<Uuid, FeedbackStoreError>;

    async fn get_prediction(&self, id: Uuid) -> Result<Prediction, FeedbackStoreError>;

    async fn enqueue_notification(
        &self,
        prediction_id: Uuid,
        contact: &str,
        send_after: DateTime<Utc>,
    ) -> Result<(), FeedbackStoreError>;

    async fn fetch_ready_notifications(
        &self,
    ) -> Result<Vec<QueuedNotification>, FeedbackStoreError>;

    async fn delete_notification(&self, id: Uuid) -> Result<(), FeedbackStoreError>;

    async fn mark_notification_sent(&self, id: Uuid) -> Result<(), FeedbackStoreError>;

    async fn mark_notification_failed(
        &self,
        id: Uuid,
        error: &str,
    ) -> Result<(), FeedbackStoreError>;

    async fn get_notification_sent_at(
        &self,
        prediction_id: Uuid,
    ) -> Result<DateTime<Utc>, FeedbackStoreError>;

    async fn record_feedback(
        &self,
        prediction: &Prediction,
        feedback: &Feedback,
        actual_weather: Option<&[PredictionLocation]>,
    ) -> Result<(), FeedbackStoreError>;
}

// ── Notification sender port ───────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum NotificationSendError {
    #[error("failed to send notification: {0}")]
    SendFailed(String),
}

#[async_trait::async_trait]
pub trait NotificationSenderPort: Send + Sync {
    async fn send_feedback_request(
        &self,
        to: &str,
        prediction_id: Uuid,
    ) -> Result<(), NotificationSendError>;
}
