use std::{sync::Arc, time::Duration};

use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tower_http::{cors::CorsLayer, timeout::TimeoutLayer, trace::TraceLayer};
use tracing::{info, warn};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;
use uuid::Uuid;

use crate::{
    domain::location::Location,
    ports::inbound::{CoatCheckError, CoatCheckPort, FeedbackError, FeedbackPort},
};

// ── OpenAPI ───────────────────────────────────────────────────────────────────

#[derive(OpenApi)]
#[openapi(
    paths(coat_check_handler),
    components(schemas(
        CoatCheckRequest,
        LocationDto,
        CoatCheckResponse,
        LocationResultDto,
        ErrorResponse,
    ))
)]
pub struct ApiDoc;

// ── App state ─────────────────────────────────────────────────────────────────

struct AppState<P: CoatCheckPort, F: FeedbackPort> {
    coat_check: P,
    feedback: F,
}

// ── Request / Response DTOs ───────────────────────────────────────────────────

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
    /// Location-specific recommendation: "coat", "rain_jacket", "umbrella", or "no"
    pub recommendation: String,
    pub reasons: Vec<String>,
    pub temp_max_celsius: f64,
    pub temp_min_celsius: f64,
    pub feels_like_min_celsius: f64,
    pub precipitation_mm: f64,
    pub wind_speed_max_kmh: f64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    pub detail: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterContactRequest {
    pub prediction_id: String,
    pub contact: String,
}

#[derive(Debug, Serialize)]
pub struct PredictionResponse {
    pub id: String,
    pub recommendation: String,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct SubmitFeedbackRequest {
    pub prediction_id: String,
    pub accurate: bool,
    pub comment: Option<String>,
}

// ── Router ────────────────────────────────────────────────────────────────────

pub fn router<P, F>(coat_check: P, feedback: F) -> Router
where
    P: CoatCheckPort + Clone + 'static,
    F: FeedbackPort + Clone + 'static,
{
    Router::new()
        .route("/coat-check", post(coat_check_handler::<P, F>))
        .route("/feedback/register", post(register_contact_handler::<P, F>))
        .route("/predictions/{id}", get(get_prediction_handler::<P, F>))
        .route("/feedback/submit", post(submit_feedback_handler::<P, F>))
        .route("/health", get(|| async { "ok" }))
        .route("/ready", get(ready_handler::<P, F>))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .with_state(Arc::new(AppState {
            coat_check,
            feedback,
        }))
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::GATEWAY_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(CorsLayer::permissive())
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// Check whether you need a coat today.
///
/// Accepts one or more locations (lat/lon). Returns a recommendation
/// for each location and an overall worst-case recommendation.
#[utoipa::path(
    post,
    path = "/coat-check",
    request_body = CoatCheckRequest,
    responses(
        (status = 200, description = "Coat recommendation", body = CoatCheckResponse),
        (status = 422, description = "Invalid location or empty request", body = ErrorResponse),
        (status = 502, description = "Weather service unavailable", body = ErrorResponse),
    ),
    tag = "coat-check"
)]
async fn coat_check_handler<P, F>(
    State(state): State<Arc<AppState<P, F>>>,
    Json(body): Json<CoatCheckRequest>,
) -> impl IntoResponse
where
    P: CoatCheckPort,
    F: FeedbackPort,
{
    info!(
        location_count = body.locations.len(),
        "coat-check request received"
    );

    let locations: Result<Vec<Location>, _> = body
        .locations
        .into_iter()
        .map(|dto| Location::new(dto.lat, dto.lon, dto.label))
        .collect();

    let locations = match locations {
        Ok(locs) => locs,
        Err(e) => {
            warn!(error = %e, "invalid location in request");
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ErrorResponse {
                    error: "invalid_location".into(),
                    detail: Some(e.to_string()),
                }),
            )
                .into_response();
        }
    };

    match state.coat_check.check(locations).await {
        Ok(decision) => {
            info!(
                recommendation = decision.overall.as_str(),
                "coat-check response"
            );

            let prediction_id = match state.feedback.save_prediction(&decision).await {
                Ok(id) => Some(id.to_string()),
                Err(e) => {
                    warn!(error = %e, "failed to save prediction, continuing without ID");
                    None
                }
            };

            let response = CoatCheckResponse {
                prediction_id,
                recommendation: decision.overall.as_str().to_string(),
                reason: decision.overall_reason,
                locations: decision
                    .by_location
                    .into_iter()
                    .map(|r| LocationResultDto {
                        label: r.location.label,
                        lat: r.location.latitude,
                        lon: r.location.longitude,
                        recommendation: r.recommendation.as_str().to_string(),
                        reasons: r.reasons,
                        temp_max_celsius: r.temp_max_celsius,
                        temp_min_celsius: r.temp_min_celsius,
                        feels_like_min_celsius: r.feels_like_min_celsius,
                        precipitation_mm: r.precipitation_mm,
                        wind_speed_max_kmh: r.wind_speed_max_kmh,
                    })
                    .collect(),
            };
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(CoatCheckError::Domain(e)) => {
            warn!(error = %e, "domain error");
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ErrorResponse {
                    error: "domain_error".into(),
                    detail: Some(e.to_string()),
                }),
            )
                .into_response()
        }
        Err(CoatCheckError::WeatherUnavailable(msg)) => {
            warn!(error = %msg, "upstream weather unavailable");
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: "weather_unavailable".into(),
                    detail: Some(msg),
                }),
            )
                .into_response()
        }
    }
}

async fn register_contact_handler<P, F>(
    State(state): State<Arc<AppState<P, F>>>,
    Json(body): Json<RegisterContactRequest>,
) -> impl IntoResponse
where
    P: CoatCheckPort,
    F: FeedbackPort,
{
    let prediction_id = match body.prediction_id.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_prediction_id".into(),
                    detail: Some("prediction_id must be a valid UUID".into()),
                }),
            )
                .into_response();
        }
    };

    match state
        .feedback
        .register_contact(prediction_id, &body.contact)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))).into_response(),
        Err(FeedbackError::InvalidContact) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid_contact".into(),
                detail: Some("please provide a valid contact address".into()),
            }),
        )
            .into_response(),
        Err(FeedbackError::PredictionNotFound) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "not_found".into(),
                detail: Some("prediction not found".into()),
            }),
        )
            .into_response(),
        Err(e) => {
            warn!(error = %e, "failed to register contact");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "internal_error".into(),
                    detail: Some(e.to_string()),
                }),
            )
                .into_response()
        }
    }
}

async fn get_prediction_handler<P, F>(
    State(state): State<Arc<AppState<P, F>>>,
    Path(id): Path<String>,
) -> impl IntoResponse
where
    P: CoatCheckPort,
    F: FeedbackPort,
{
    let prediction_id = match id.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_prediction_id".into(),
                    detail: Some("prediction_id must be a valid UUID".into()),
                }),
            )
                .into_response();
        }
    };

    match state.feedback.get_prediction(prediction_id).await {
        Ok(prediction) => (
            StatusCode::OK,
            Json(PredictionResponse {
                id: prediction.id.to_string(),
                recommendation: prediction.recommendation,
                reason: prediction.reason,
            }),
        )
            .into_response(),
        Err(FeedbackError::PredictionNotFound) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "not_found".into(),
                detail: Some("prediction not found".into()),
            }),
        )
            .into_response(),
        Err(FeedbackError::LinkExpired) => (
            StatusCode::GONE,
            Json(ErrorResponse {
                error: "link_expired".into(),
                detail: Some("feedback link has expired".into()),
            }),
        )
            .into_response(),
        Err(e) => {
            warn!(error = %e, "failed to get prediction");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "internal_error".into(),
                    detail: Some(e.to_string()),
                }),
            )
                .into_response()
        }
    }
}

async fn submit_feedback_handler<P, F>(
    State(state): State<Arc<AppState<P, F>>>,
    Json(body): Json<SubmitFeedbackRequest>,
) -> impl IntoResponse
where
    P: CoatCheckPort,
    F: FeedbackPort,
{
    let prediction_id = match body.prediction_id.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_prediction_id".into(),
                    detail: Some("prediction_id must be a valid UUID".into()),
                }),
            )
                .into_response();
        }
    };

    if let Some(ref comment) = body.comment {
        if comment.len() > 1000 {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ErrorResponse {
                    error: "comment_too_long".into(),
                    detail: Some("comment must be 1000 characters or fewer".into()),
                }),
            )
                .into_response();
        }
    }

    match state
        .feedback
        .submit_feedback(prediction_id, body.accurate, body.comment)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))).into_response(),
        Err(FeedbackError::PredictionNotFound) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "not_found".into(),
                detail: Some("prediction not found".into()),
            }),
        )
            .into_response(),
        Err(FeedbackError::AlreadyRated) => (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "already_rated".into(),
                detail: Some("feedback already submitted for this prediction".into()),
            }),
        )
            .into_response(),
        Err(FeedbackError::LinkExpired) => (
            StatusCode::GONE,
            Json(ErrorResponse {
                error: "link_expired".into(),
                detail: Some("feedback link has expired".into()),
            }),
        )
            .into_response(),
        Err(e) => {
            warn!(error = %e, "failed to submit feedback");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "internal_error".into(),
                    detail: Some(e.to_string()),
                }),
            )
                .into_response()
        }
    }
}

async fn ready_handler<P, F>(State(state): State<Arc<AppState<P, F>>>) -> impl IntoResponse
where
    P: CoatCheckPort,
    F: FeedbackPort,
{
    let probe = Location::new(0.0, 0.0, None).unwrap();
    match state.coat_check.check(vec![probe]).await {
        Ok(_) => (StatusCode::OK, "ready").into_response(),
        Err(e) => {
            warn!(error = %e, "readiness probe failed");
            (StatusCode::SERVICE_UNAVAILABLE, "not ready").into_response()
        }
    }
}

// ── Integration tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            location::Location,
            prediction::Prediction,
            recommendation::{CoatDecision, CoatRecommendation, LocationRecommendation},
        },
        ports::inbound::{CoatCheckError, CoatCheckPort, FeedbackError, FeedbackPort},
    };
    use axum_test::TestServer;

    // ── Fake CoatCheckPort impls ───────────────────────────────────────────

    #[derive(Clone)]
    struct AlwaysNo;

    #[async_trait::async_trait]
    impl CoatCheckPort for AlwaysNo {
        async fn check(&self, locations: Vec<Location>) -> Result<CoatDecision, CoatCheckError> {
            let by_location = locations
                .into_iter()
                .map(|loc| LocationRecommendation {
                    location: loc,
                    recommendation: CoatRecommendation::No,
                    reasons: vec![],
                    temp_max_celsius: 20.0,
                    temp_min_celsius: 15.0,
                    feels_like_min_celsius: 14.5,
                    precipitation_mm: 0.0,
                    wind_speed_max_kmh: 5.0,
                })
                .collect();
            Ok(CoatDecision {
                overall: CoatRecommendation::No,
                by_location,
                overall_reason: "No coat or jacket needed at any of your locations today.".into(),
            })
        }
    }

    #[derive(Clone)]
    struct AlwaysCoat;

    #[async_trait::async_trait]
    impl CoatCheckPort for AlwaysCoat {
        async fn check(&self, locations: Vec<Location>) -> Result<CoatDecision, CoatCheckError> {
            let by_location = locations
                .into_iter()
                .map(|loc| LocationRecommendation {
                    location: loc,
                    recommendation: CoatRecommendation::Coat,
                    reasons: vec!["feels like as low as 5.0°C (threshold 12°C)".into()],
                    temp_max_celsius: 8.0,
                    temp_min_celsius: 3.0,
                    feels_like_min_celsius: 5.0,
                    precipitation_mm: 0.0,
                    wind_speed_max_kmh: 5.0,
                })
                .collect();
            Ok(CoatDecision {
                overall: CoatRecommendation::Coat,
                by_location,
                overall_reason: "London: feels like as low as 5.0°C".into(),
            })
        }
    }

    #[derive(Clone)]
    struct AlwaysRainJacket;

    #[async_trait::async_trait]
    impl CoatCheckPort for AlwaysRainJacket {
        async fn check(&self, locations: Vec<Location>) -> Result<CoatDecision, CoatCheckError> {
            let by_location = locations
                .into_iter()
                .map(|loc| LocationRecommendation {
                    location: loc,
                    recommendation: CoatRecommendation::RainJacket,
                    reasons: vec!["8.0 mm precipitation expected".into()],
                    temp_max_celsius: 22.0,
                    temp_min_celsius: 15.0,
                    feels_like_min_celsius: 14.0,
                    precipitation_mm: 8.0,
                    wind_speed_max_kmh: 5.0,
                })
                .collect();
            Ok(CoatDecision {
                overall: CoatRecommendation::RainJacket,
                by_location,
                overall_reason: "London: 8.0 mm precipitation expected".into(),
            })
        }
    }

    #[derive(Clone)]
    struct WeatherDown;

    #[async_trait::async_trait]
    impl CoatCheckPort for WeatherDown {
        async fn check(&self, _locations: Vec<Location>) -> Result<CoatDecision, CoatCheckError> {
            Err(CoatCheckError::WeatherUnavailable("timeout".into()))
        }
    }

    // ── Fake FeedbackPort impls ────────────────────────────────────────────

    #[derive(Clone)]
    struct FakeFeedback;

    #[async_trait::async_trait]
    impl FeedbackPort for FakeFeedback {
        async fn save_prediction(&self, _decision: &CoatDecision) -> Result<Uuid, FeedbackError> {
            Ok(Uuid::new_v4())
        }

        async fn register_contact(
            &self,
            _prediction_id: Uuid,
            contact: &str,
        ) -> Result<(), FeedbackError> {
            if !contact.contains('@') {
                return Err(FeedbackError::InvalidContact);
            }
            Ok(())
        }

        async fn get_prediction(&self, id: Uuid) -> Result<Prediction, FeedbackError> {
            Ok(Prediction {
                id,
                recommendation: "coat".into(),
                reason: "London: feels like as low as 5.0°C".into(),
                locations: vec![],
                created_at: chrono::Utc::now(),
            })
        }

        async fn submit_feedback(
            &self,
            _prediction_id: Uuid,
            _accurate: bool,
            _comment: Option<String>,
        ) -> Result<(), FeedbackError> {
            Ok(())
        }
    }

    #[derive(Clone)]
    struct AlreadyRatedFeedback;

    #[async_trait::async_trait]
    impl FeedbackPort for AlreadyRatedFeedback {
        async fn save_prediction(&self, _decision: &CoatDecision) -> Result<Uuid, FeedbackError> {
            Ok(Uuid::new_v4())
        }

        async fn register_contact(
            &self,
            _prediction_id: Uuid,
            _contact: &str,
        ) -> Result<(), FeedbackError> {
            Ok(())
        }

        async fn get_prediction(&self, _id: Uuid) -> Result<Prediction, FeedbackError> {
            Err(FeedbackError::LinkExpired)
        }

        async fn submit_feedback(
            &self,
            _prediction_id: Uuid,
            _accurate: bool,
            _comment: Option<String>,
        ) -> Result<(), FeedbackError> {
            Err(FeedbackError::AlreadyRated)
        }
    }

    // ── Test helpers ───────────────────────────────────────────────────────

    fn server<P: CoatCheckPort + Clone + 'static>(service: P) -> TestServer {
        TestServer::new(router(service, FakeFeedback))
    }

    fn server_with_feedback<P, F>(service: P, feedback: F) -> TestServer
    where
        P: CoatCheckPort + Clone + 'static,
        F: FeedbackPort + Clone + 'static,
    {
        TestServer::new(router(service, feedback))
    }

    #[tokio::test]
    async fn returns_no() {
        let resp = server(AlwaysNo)
            .post("/coat-check")
            .json(&serde_json::json!({
                "locations": [{"lat": 51.5, "lon": -0.1, "label": "London"}]
            }))
            .await;

        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["recommendation"], "no");
        assert!(body["prediction_id"].is_string());
    }

    #[tokio::test]
    async fn returns_coat() {
        let resp = server(AlwaysCoat)
            .post("/coat-check")
            .json(&serde_json::json!({
                "locations": [{"lat": 51.5, "lon": -0.1}]
            }))
            .await;

        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["recommendation"], "coat");
        assert!(body["prediction_id"].is_string());
    }

    #[tokio::test]
    async fn returns_rain_jacket() {
        let resp = server(AlwaysRainJacket)
            .post("/coat-check")
            .json(&serde_json::json!({
                "locations": [{"lat": 51.5, "lon": -0.1}]
            }))
            .await;

        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["recommendation"], "rain_jacket");
    }

    #[tokio::test]
    async fn invalid_latitude_returns_422() {
        let resp = server(AlwaysNo)
            .post("/coat-check")
            .json(&serde_json::json!({
                "locations": [{"lat": 999.0, "lon": 0.0}]
            }))
            .await;

        resp.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["error"], "invalid_location");
    }

    #[tokio::test]
    async fn weather_down_returns_502() {
        let resp = server(WeatherDown)
            .post("/coat-check")
            .json(&serde_json::json!({
                "locations": [{"lat": 51.5, "lon": -0.1}]
            }))
            .await;

        resp.assert_status(StatusCode::BAD_GATEWAY);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["error"], "weather_unavailable");
    }

    #[tokio::test]
    async fn health_endpoint() {
        let resp = server(AlwaysNo).get("/health").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn ready_when_weather_up() {
        let resp = server(AlwaysNo).get("/ready").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn not_ready_when_weather_down() {
        let resp = server(WeatherDown).get("/ready").await;
        resp.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn openapi_spec_generates() {
        let spec = ApiDoc::openapi();
        let json = spec.to_pretty_json().unwrap();
        assert!(json.contains("/coat-check"));
        assert!(json.contains("CoatCheckRequest"));
        assert!(json.contains("CoatCheckResponse"));
    }

    #[tokio::test]
    async fn register_contact_ok() {
        let s = server(AlwaysNo);
        let prediction_id = Uuid::new_v4().to_string();
        let resp = s
            .post("/feedback/register")
            .json(&serde_json::json!({
                "prediction_id": prediction_id,
                "contact": "test@example.com"
            }))
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn register_contact_invalid() {
        let s = server(AlwaysNo);
        let resp = s
            .post("/feedback/register")
            .json(&serde_json::json!({
                "prediction_id": Uuid::new_v4().to_string(),
                "contact": "notanemail"
            }))
            .await;
        resp.assert_status(StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_prediction_ok() {
        let s = server(AlwaysNo);
        let id = Uuid::new_v4();
        let resp = s.get(&format!("/predictions/{id}")).await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["recommendation"], "coat");
    }

    #[tokio::test]
    async fn get_prediction_invalid_id() {
        let s = server(AlwaysNo);
        let resp = s.get("/predictions/not-a-uuid").await;
        resp.assert_status(StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_prediction_link_expired_returns_410() {
        let s = server_with_feedback(AlwaysNo, AlreadyRatedFeedback);
        let resp = s.get(&format!("/predictions/{}", Uuid::new_v4())).await;
        resp.assert_status(StatusCode::GONE);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["error"], "link_expired");
    }

    #[tokio::test]
    async fn submit_feedback_ok() {
        let s = server(AlwaysNo);
        let resp = s
            .post("/feedback/submit")
            .json(&serde_json::json!({
                "prediction_id": Uuid::new_v4().to_string(),
                "accurate": true,
                "comment": "spot on!"
            }))
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn submit_feedback_already_rated_returns_409() {
        let s = server_with_feedback(AlwaysNo, AlreadyRatedFeedback);
        let resp = s
            .post("/feedback/submit")
            .json(&serde_json::json!({
                "prediction_id": Uuid::new_v4().to_string(),
                "accurate": true,
            }))
            .await;
        resp.assert_status(StatusCode::CONFLICT);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["error"], "already_rated");
    }

    #[tokio::test]
    async fn submit_feedback_comment_too_long_returns_422() {
        let s = server(AlwaysNo);
        let long_comment = "x".repeat(1001);
        let resp = s
            .post("/feedback/submit")
            .json(&serde_json::json!({
                "prediction_id": Uuid::new_v4().to_string(),
                "accurate": true,
                "comment": long_comment,
            }))
            .await;
        resp.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["error"], "comment_too_long");
    }
}
