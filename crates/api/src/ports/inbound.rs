use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    error::DomainError, location::Location, prediction::Prediction, recommendation::CoatDecision,
};

// ── Coat check port ────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum CoatCheckError {
    #[error("domain validation error: {0}")]
    Domain(#[from] DomainError),
    #[error("weather data unavailable: {0}")]
    WeatherUnavailable(String),
}

#[async_trait::async_trait]
pub trait CoatCheckPort: Send + Sync {
    async fn check(&self, locations: Vec<Location>) -> Result<CoatDecision, CoatCheckError>;
}

// ── Feedback port ──────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum FeedbackError {
    #[error("database error: {0}")]
    Database(String),
    #[error("prediction not found")]
    PredictionNotFound,
    #[error("invalid contact address")]
    InvalidContact,
    #[error("feedback already submitted for this prediction")]
    AlreadyRated,
    #[error("feedback link has expired")]
    LinkExpired,
}

#[async_trait::async_trait]
pub trait FeedbackPort: Send + Sync {
    async fn save_prediction(&self, decision: &CoatDecision) -> Result<Uuid, FeedbackError>;

    async fn register_contact(
        &self,
        prediction_id: Uuid,
        contact: &str,
    ) -> Result<(), FeedbackError>;

    async fn get_prediction(&self, id: Uuid) -> Result<Prediction, FeedbackError>;

    async fn submit_feedback(
        &self,
        prediction_id: Uuid,
        accurate: bool,
        comment: Option<String>,
    ) -> Result<(), FeedbackError>;
}
