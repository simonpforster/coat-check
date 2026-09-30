use crate::domain::{error::DomainError, location::Location, recommendation::CoatDecision};
use thiserror::Error;

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
