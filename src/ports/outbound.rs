use crate::domain::{location::Location, weather::DailyForecast};
use thiserror::Error;

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
}
