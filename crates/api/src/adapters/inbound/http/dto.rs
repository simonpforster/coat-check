use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};

#[derive(OpenApi)]
#[openapi(
    paths(
        super::coat_check_handler,
        super::register_contact_handler,
        super::get_prediction_handler,
        super::submit_feedback_handler,
        super::unsubscribe_handler,
    ),
    components(schemas(
        CoatCheckRequest,
        LocationDto,
        CoatCheckResponse,
        LocationResultDto,
        ErrorResponse,
        RegisterContactRequest,
        PredictionResponse,
        SubmitFeedbackRequest,
        UnsubscribeRequest,
        StatusResponse,
    ))
)]
pub struct ApiDoc;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CoatCheckRequest {
    /// One or more locations to check. The worst-case recommendation wins.
    pub locations: Vec<LocationDto>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LocationDto {
    /// Latitude (-90 to 90)
    pub lat: f64,
    /// Longitude (-180 to 180)
    pub lon: f64,
    /// Optional human-readable label (e.g. "Office")
    pub label: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CoatCheckResponse {
    /// ID of the persisted prediction (for feedback). Absent when feedback is disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prediction_id: Option<String>,
    /// Overall recommendation: "coat", "rain_jacket", "umbrella", or "no"
    pub recommendation: String,
    /// Human-readable explanation
    pub reason: String,
    /// Per-location breakdown
    pub locations: Vec<LocationResultDto>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LocationResultDto {
    pub label: Option<String>,
    pub lat: f64,
    pub lon: f64,
    /// IANA timezone identifier (e.g. "Europe/London")
    pub timezone: String,
    /// Location-specific recommendation: "coat", "rain_jacket", "umbrella", or "no"
    pub recommendation: String,
    pub reasons: Vec<String>,
    pub temp_max_celsius: f64,
    pub temp_min_celsius: f64,
    pub feels_like_min_celsius: f64,
    pub precipitation_mm: f64,
    pub wind_speed_max_kmh: f64,
    pub snowfall_cm: f64,
    /// WMO weather interpretation code
    pub weather_code: u16,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    pub detail: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterContactRequest {
    /// UUID of the prediction to attach this contact to
    pub prediction_id: String,
    /// Email address to send the feedback request to
    pub contact: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PredictionResponse {
    /// Prediction UUID
    pub id: String,
    /// The recommendation that was given
    pub recommendation: String,
    /// Human-readable explanation
    pub reason: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SubmitFeedbackRequest {
    /// UUID of the prediction being rated
    pub prediction_id: String,
    /// What the user actually brought: "no", "umbrella", "rain_jacket", or "coat"
    pub brought: String,
    /// What the user thinks they should have brought
    pub should_have_brought: String,
    /// Optional free-text comment (max 1000 chars)
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UnsubscribeRequest {
    /// Email address to unsubscribe
    pub contact: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StatusResponse {
    /// "ok"
    pub status: String,
}
