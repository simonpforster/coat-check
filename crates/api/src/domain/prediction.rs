use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Weather data for a single location, stored as JSONB in Postgres.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PredictionLocation {
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

/// A persisted coat-check prediction.
#[derive(Debug, Clone)]
pub struct Prediction {
    pub id: Uuid,
    pub recommendation: String,
    pub reason: String,
    pub locations: Vec<PredictionLocation>,
    pub created_at: DateTime<Utc>,
}

/// User feedback on whether a prediction was accurate.
#[derive(Debug, Clone)]
pub struct Feedback {
    pub accurate: bool,
    pub comment: Option<String>,
}
