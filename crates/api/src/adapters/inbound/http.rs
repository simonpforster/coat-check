use std::{sync::Arc, time::Duration};

use axum::{
    extract::{Json, State},
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

use crate::{
    domain::location::Location,
    ports::inbound::{CoatCheckError, CoatCheckPort},
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

// ── Router ────────────────────────────────────────────────────────────────────

pub fn router<P>(service: P) -> Router
where
    P: CoatCheckPort + Clone + 'static,
{
    Router::new()
        .route("/coat-check", post(coat_check_handler::<P>))
        .route("/health", get(|| async { "ok" }))
        .route("/ready", get(ready_handler::<P>))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .with_state(Arc::new(service))
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::GATEWAY_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(CorsLayer::permissive())
}

// ── Handler ───────────────────────────────────────────────────────────────────

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
async fn coat_check_handler<P>(
    State(service): State<Arc<P>>,
    Json(body): Json<CoatCheckRequest>,
) -> impl IntoResponse
where
    P: CoatCheckPort,
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

    match service.check(locations).await {
        Ok(decision) => {
            info!(
                recommendation = decision.overall.as_str(),
                "coat-check response"
            );
            let response = CoatCheckResponse {
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

async fn ready_handler<P>(State(service): State<Arc<P>>) -> impl IntoResponse
where
    P: CoatCheckPort,
{
    let probe = Location::new(0.0, 0.0, None).unwrap();
    match service.check(vec![probe]).await {
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
            recommendation::{CoatDecision, CoatRecommendation, LocationRecommendation},
        },
        ports::inbound::{CoatCheckError, CoatCheckPort},
    };
    use axum_test::TestServer;

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

    fn server<P: CoatCheckPort + Clone + 'static>(service: P) -> TestServer {
        TestServer::new(router(service))
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
    async fn openapi_spec_generates() {
        let spec = ApiDoc::openapi();
        let json = spec.to_pretty_json().unwrap();
        assert!(json.contains("/coat-check"));
        assert!(json.contains("CoatCheckRequest"));
        assert!(json.contains("CoatCheckResponse"));
    }
}
