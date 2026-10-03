use serde::Deserialize;

use crate::ports::outbound::{GeocodingError, GeocodingPort, GeocodingResult};

#[derive(Clone)]
pub struct OpenMeteoGeocodingClient {
    http: reqwest::Client,
    base_url: String,
}

impl OpenMeteoGeocodingClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build HTTP client"),
            base_url: "https://geocoding-api.open-meteo.com/v1/search".into(),
        }
    }
}

impl Default for OpenMeteoGeocodingClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl GeocodingPort for OpenMeteoGeocodingClient {
    async fn geocode(&self, query: &str) -> Result<Vec<GeocodingResult>, GeocodingError> {
        let resp = self
            .http
            .get(&self.base_url)
            .query(&[("name", query), ("count", "5"), ("language", "en")])
            .send()
            .await
            .map_err(|e| GeocodingError::RequestFailed(e.to_string()))?;

        let body: GeocodingResponseDto = resp
            .json()
            .await
            .map_err(|e| GeocodingError::RequestFailed(e.to_string()))?;

        Ok(body
            .results
            .into_iter()
            .map(|r| GeocodingResult {
                name: r.name,
                latitude: r.latitude,
                longitude: r.longitude,
                country: r.country,
                admin1: r.admin1,
            })
            .collect())
    }
}

// ── Private serde DTOs ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GeocodingResponseDto {
    #[serde(default)]
    results: Vec<GeocodingResultDto>,
}

#[derive(Deserialize)]
struct GeocodingResultDto {
    name: String,
    latitude: f64,
    longitude: f64,
    country: Option<String>,
    admin1: Option<String>,
}
