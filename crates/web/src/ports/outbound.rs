use thiserror::Error;

// ── Coat Check API port ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Error)]
pub enum CoatCheckApiError {
    #[error("network error: {0}")]
    Network(String),
    #[error("upstream API error: {error}")]
    Upstream {
        error: String,
        detail: Option<String>,
    },
}

#[derive(Clone)]
pub struct CoatCheckLocation {
    pub lat: f64,
    pub lon: f64,
    pub label: Option<String>,
}

#[derive(Clone)]
pub struct CoatCheckResult {
    pub prediction_id: Option<String>,
    pub recommendation: String,
    pub reason: String,
    pub locations: Vec<CoatCheckLocationResult>,
}

#[derive(Clone)]
pub struct CoatCheckLocationResult {
    pub label: Option<String>,
    pub lat: f64,
    pub lon: f64,
    pub recommendation: String,
    pub reasons: Vec<String>,
    pub temp_max_celsius: f64,
    pub temp_min_celsius: f64,
    pub feels_like_min_celsius: f64,
    pub precipitation_mm: f64,
    pub wind_speed_max_kmh: f64,
}

#[async_trait::async_trait]
pub trait CoatCheckApiPort: Send + Sync {
    async fn check(
        &self,
        locations: Vec<CoatCheckLocation>,
    ) -> Result<CoatCheckResult, CoatCheckApiError>;
}

// ── Feedback API port ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Error)]
pub enum FeedbackApiError {
    #[error("network error: {0}")]
    Network(String),
    #[error("API error: {0}")]
    Api(String),
}

pub struct PredictionResponse {
    pub recommendation: String,
    pub reason: String,
}

#[async_trait::async_trait]
pub trait FeedbackApiPort: Send + Sync {
    async fn register_email(
        &self,
        prediction_id: &str,
        email: &str,
    ) -> Result<(), FeedbackApiError>;

    async fn get_prediction(
        &self,
        prediction_id: &str,
    ) -> Result<PredictionResponse, FeedbackApiError>;

    async fn submit_feedback(
        &self,
        prediction_id: &str,
        brought: &str,
        should_have_brought: &str,
        comment: Option<&str>,
    ) -> Result<(), FeedbackApiError>;

    async fn unsubscribe(&self, contact: &str) -> Result<(), FeedbackApiError>;
}

// ── Geocoding port ──────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum GeocodingError {
    #[error("geocoding request failed: {0}")]
    RequestFailed(String),
}

#[derive(Clone)]
pub struct GeocodingResult {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub country: Option<String>,
    pub admin1: Option<String>,
}

impl GeocodingResult {
    pub fn display_name(&self) -> String {
        let mut parts = vec![self.name.clone()];
        if let Some(ref admin1) = self.admin1 {
            if admin1 != &self.name {
                parts.push(admin1.clone());
            }
        }
        if let Some(ref country) = self.country {
            parts.push(country.clone());
        }
        parts.join(", ")
    }
}

#[async_trait::async_trait]
pub trait GeocodingPort: Send + Sync {
    async fn geocode(&self, query: &str) -> Result<Vec<GeocodingResult>, GeocodingError>;
}
