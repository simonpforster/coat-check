mod dto;

pub use dto::*;

use std::{sync::Arc, time::Duration};

use axum::http::{HeaderValue, Method};
use axum::{
    extract::{Json, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use tower_http::{cors::CorsLayer, timeout::TimeoutLayer, trace::TraceLayer};
use tracing::{info, warn};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use uuid::Uuid;

use coat_check_common::Recommendation;

use crate::{
    domain::location::Location,
    ports::inbound::{CoatCheckError, CoatCheckPort, FeedbackError, FeedbackPort},
};

// ── App state ─────────────────────────────────────────────────────────────────

struct AppState<P: CoatCheckPort, F: FeedbackPort> {
    coat_check: P,
    feedback: F,
}

// ── Router ────────────────────────────────────────────────────────────────────

pub fn router<P, F>(coat_check: P, feedback: F) -> Router
where
    P: CoatCheckPort + Clone + 'static,
    F: FeedbackPort + Clone + 'static,
{
    let cors = build_cors_layer();

    Router::new()
        .route("/coat-check", post(coat_check_handler::<P, F>))
        .route("/feedback/register", post(register_contact_handler::<P, F>))
        .route("/predictions/{id}", get(get_prediction_handler::<P, F>))
        .route("/feedback/submit", post(submit_feedback_handler::<P, F>))
        .route("/feedback/unsubscribe", post(unsubscribe_handler::<P, F>))
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
        .layer(cors)
}

fn build_cors_layer() -> CorsLayer {
    let allowed_origins = std::env::var("CORS_ORIGINS").unwrap_or_default();

    if allowed_origins.is_empty() {
        return CorsLayer::permissive();
    }

    let origins: Vec<HeaderValue> = allowed_origins
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(tower_http::cors::Any)
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
                        timezone: r.timezone,
                        recommendation: r.recommendation.as_str().to_string(),
                        reasons: r.reasons,
                        temp_max_celsius: r.temp_max_celsius,
                        temp_min_celsius: r.temp_min_celsius,
                        feels_like_min_celsius: r.feels_like_min_celsius,
                        precipitation_mm: r.precipitation_mm,
                        wind_speed_max_kmh: r.wind_speed_max_kmh,
                        snowfall_cm: r.snowfall_cm,
                        weather_code: r.weather_code,
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

/// Register an email address to receive a feedback request later.
#[utoipa::path(
    post,
    path = "/feedback/register",
    request_body = RegisterContactRequest,
    responses(
        (status = 200, description = "Contact registered", body = StatusResponse),
        (status = 400, description = "Invalid prediction ID or contact", body = ErrorResponse),
        (status = 404, description = "Prediction not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    ),
    tag = "feedback"
)]
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
        Ok(()) => (
            StatusCode::OK,
            Json(StatusResponse {
                status: "ok".into(),
            }),
        )
            .into_response(),
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

/// Retrieve a prediction by ID.
#[utoipa::path(
    get,
    path = "/predictions/{id}",
    params(("id" = String, Path, description = "Prediction UUID")),
    responses(
        (status = 200, description = "Prediction found", body = PredictionResponse),
        (status = 400, description = "Invalid prediction ID", body = ErrorResponse),
        (status = 404, description = "Prediction not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    ),
    tag = "feedback"
)]
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

/// Submit feedback on a prediction.
#[utoipa::path(
    post,
    path = "/feedback/submit",
    request_body = SubmitFeedbackRequest,
    responses(
        (status = 200, description = "Feedback recorded", body = StatusResponse),
        (status = 400, description = "Invalid prediction ID", body = ErrorResponse),
        (status = 404, description = "Prediction not found", body = ErrorResponse),
        (status = 409, description = "Already rated", body = ErrorResponse),
        (status = 422, description = "Invalid recommendation value or comment too long", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    ),
    tag = "feedback"
)]
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

    if body.brought.parse::<Recommendation>().is_err()
        || body.should_have_brought.parse::<Recommendation>().is_err()
    {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorResponse {
                error: "invalid_recommendation".into(),
                detail: Some("brought and should_have_brought must be one of: no, umbrella, rain_jacket, coat".into()),
            }),
        )
            .into_response();
    }

    if let Some(ref comment) = body.comment {
        if comment.chars().count() > 1000 {
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
        .submit_feedback(
            prediction_id,
            &body.brought,
            &body.should_have_brought,
            body.comment,
        )
        .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(StatusResponse {
                status: "ok".into(),
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
        Err(FeedbackError::AlreadyRated) => (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "already_rated".into(),
                detail: Some("feedback already submitted for this prediction".into()),
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

/// Unsubscribe an email from pending feedback notifications.
#[utoipa::path(
    post,
    path = "/feedback/unsubscribe",
    params(("contact" = String, Query, description = "Email address to unsubscribe")),
    responses(
        (status = 200, description = "Unsubscribed (always succeeds)", body = StatusResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    ),
    tag = "feedback"
)]
async fn unsubscribe_handler<P, F>(
    State(state): State<Arc<AppState<P, F>>>,
    Query(params): Query<UnsubscribeRequest>,
) -> impl IntoResponse
where
    P: CoatCheckPort,
    F: FeedbackPort,
{
    match state.feedback.unsubscribe(&params.contact).await {
        Ok(()) => (
            StatusCode::OK,
            Json(StatusResponse {
                status: "ok".into(),
            }),
        )
            .into_response(),
        Err(e) => {
            warn!(error = %e, "failed to unsubscribe");
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

#[cfg(test)]
mod tests;
