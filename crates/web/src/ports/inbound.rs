use thiserror::Error;

use crate::{
    domain::{recommendation::CheckResult, suggestion::Suggestion},
    ports::outbound::PredictionResponse,
};

#[derive(Debug, Clone, Error)]
pub enum WebPortError {
    #[error("no locations provided")]
    NoLocations,
    #[error("invalid latitude")]
    InvalidLatitude,
    #[error("invalid longitude")]
    InvalidLongitude,
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("upstream error: {error}")]
    UpstreamError {
        error: String,
        detail: Option<String>,
    },
    #[error("geocoding failed: {0}")]
    GeocodingFailed(String),
    #[error("feedback error: {0}")]
    FeedbackError(String),
}

pub struct LocationInput {
    pub lat: String,
    pub lon: String,
    pub label: Option<String>,
}

#[async_trait::async_trait]
pub trait WebPort: Send + Sync {
    async fn check_coat(&self, locations: Vec<LocationInput>) -> Result<CheckResult, WebPortError>;

    async fn search_locations(&self, query: &str) -> Result<Vec<Suggestion>, WebPortError>;

    async fn register_email(&self, prediction_id: &str, email: &str) -> Result<(), WebPortError>;

    async fn get_prediction(&self, prediction_id: &str)
        -> Result<PredictionResponse, WebPortError>;

    async fn submit_feedback(
        &self,
        prediction_id: &str,
        brought: &str,
        should_have_brought: &str,
        comment: Option<&str>,
    ) -> Result<(), WebPortError>;

    async fn unsubscribe(&self, contact: &str) -> Result<(), WebPortError>;
}
