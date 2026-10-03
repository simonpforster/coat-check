use thiserror::Error;

use crate::domain::{recommendation::CheckResult, suggestion::Suggestion};

#[derive(Debug, Error)]
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
}
