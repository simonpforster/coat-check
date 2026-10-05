use serde::{Deserialize, Serialize};
use tracing::info;

use crate::ports::outbound::{
    CoatCheckApiError, CoatCheckApiPort, CoatCheckLocation, CoatCheckLocationResult,
    CoatCheckResult, FeedbackApiError, FeedbackApiPort, PredictionResponse,
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
            let body: ApiErrorResponse = resp.json().await.unwrap_or_else(|_| ApiErrorResponse {
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
            prediction_id: api_resp.prediction_id,
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

#[async_trait::async_trait]
impl FeedbackApiPort for CoatCheckApiClient {
    async fn register_email(
        &self,
        prediction_id: &str,
        email: &str,
    ) -> Result<(), FeedbackApiError> {
        let url = format!("{}/feedback/register", self.api_url);
        let resp = self
            .http
            .post(&url)
            .json(&serde_json::json!({
                "prediction_id": prediction_id,
                "contact": email,
            }))
            .send()
            .await
            .map_err(|e| FeedbackApiError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(FeedbackApiError::Api(body));
        }
        Ok(())
    }

    async fn get_prediction(
        &self,
        prediction_id: &str,
    ) -> Result<PredictionResponse, FeedbackApiError> {
        let url = format!("{}/predictions/{}", self.api_url, prediction_id);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| FeedbackApiError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(FeedbackApiError::Api(body));
        }

        let data: ApiPredictionResponse = resp
            .json()
            .await
            .map_err(|e| FeedbackApiError::Network(e.to_string()))?;

        Ok(PredictionResponse {
            recommendation: data.recommendation,
            reason: data.reason,
        })
    }

    async fn submit_feedback(
        &self,
        prediction_id: &str,
        brought: &str,
        should_have_brought: &str,
        comment: Option<&str>,
    ) -> Result<(), FeedbackApiError> {
        let url = format!("{}/feedback/submit", self.api_url);
        let resp = self
            .http
            .post(&url)
            .json(&serde_json::json!({
                "prediction_id": prediction_id,
                "brought": brought,
                "should_have_brought": should_have_brought,
                "comment": comment,
            }))
            .send()
            .await
            .map_err(|e| FeedbackApiError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(FeedbackApiError::Api(body));
        }
        Ok(())
    }

    async fn unsubscribe(&self, contact: &str) -> Result<(), FeedbackApiError> {
        let url = format!(
            "{}/feedback/unsubscribe?contact={}",
            self.api_url,
            urlencoding::encode(contact)
        );
        let resp = self
            .http
            .post(&url)
            .send()
            .await
            .map_err(|e| FeedbackApiError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(FeedbackApiError::Api(body));
        }
        Ok(())
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
    prediction_id: Option<String>,
    recommendation: String,
    reason: String,
    locations: Vec<ApiLocationResult>,
}

#[derive(Deserialize)]
struct ApiPredictionResponse {
    recommendation: String,
    reason: String,
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
