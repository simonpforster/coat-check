use serde::{Deserialize, Serialize};
use tracing::info;

use crate::ports::outbound::{
    CoatCheckApiError, CoatCheckApiPort, CoatCheckLocation, CoatCheckLocationResult,
    CoatCheckResult,
};

#[derive(Clone)]
pub struct CoatCheckApiClient {
    http: reqwest::Client,
    api_url: String,
}

impl CoatCheckApiClient {
    pub fn new(api_url: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("failed to build HTTP client"),
            api_url,
        }
    }
}

#[async_trait::async_trait]
impl CoatCheckApiPort for CoatCheckApiClient {
    async fn check(
        &self,
        locations: Vec<CoatCheckLocation>,
    ) -> Result<CoatCheckResult, CoatCheckApiError> {
        let url = format!("{}/coat-check", self.api_url);

        let request = ApiRequest {
            locations: locations
                .into_iter()
                .map(|l| ApiLocation {
                    lat: l.lat,
                    lon: l.lon,
                    label: l.label,
                })
                .collect(),
        };

        info!(
            location_count = request.locations.len(),
            "coat-check API request"
        );

        let resp = self
            .http
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| CoatCheckApiError::Network(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body: ApiErrorResponse =
                resp.json().await.unwrap_or_else(|_| ApiErrorResponse {
                    error: format!("HTTP {status}"),
                    detail: None,
                });
            return Err(CoatCheckApiError::Upstream {
                error: body.error,
                detail: body.detail,
            });
        }

        let api_resp: ApiResponse = resp
            .json()
            .await
            .map_err(|e| CoatCheckApiError::Network(e.to_string()))?;

        Ok(CoatCheckResult {
            recommendation: api_resp.recommendation,
            reason: api_resp.reason,
            locations: api_resp
                .locations
                .into_iter()
                .map(|r| CoatCheckLocationResult {
                    label: r.label,
                    lat: r.lat,
                    lon: r.lon,
                    recommendation: r.recommendation,
                    reasons: r.reasons,
                    temp_max_celsius: r.temp_max_celsius,
                    temp_min_celsius: r.temp_min_celsius,
                    feels_like_min_celsius: r.feels_like_min_celsius,
                    precipitation_mm: r.precipitation_mm,
                    wind_speed_max_kmh: r.wind_speed_max_kmh,
                })
                .collect(),
        })
    }
}

// ── Private serde DTOs ──────────────────────────────────────────────────────

#[derive(Serialize)]
struct ApiRequest {
    locations: Vec<ApiLocation>,
}

#[derive(Serialize)]
struct ApiLocation {
    lat: f64,
    lon: f64,
    label: Option<String>,
}

#[derive(Deserialize)]
struct ApiResponse {
    recommendation: String,
    reason: String,
    locations: Vec<ApiLocationResult>,
}

#[derive(Deserialize)]
struct ApiLocationResult {
    label: Option<String>,
    lat: f64,
    lon: f64,
    recommendation: String,
    reasons: Vec<String>,
    temp_max_celsius: f64,
    temp_min_celsius: f64,
    feels_like_min_celsius: f64,
    precipitation_mm: f64,
    wind_speed_max_kmh: f64,
}

#[derive(Deserialize)]
struct ApiErrorResponse {
    error: String,
    detail: Option<String>,
}
