use std::sync::Arc;

use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Form, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::signal;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

// ── API client ───────────────────────────────────────────────────────────────

#[derive(Clone)]
struct ApiClient {
    http: reqwest::Client,
    base_url: String,
}

impl ApiClient {
    fn new(base_url: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("failed to build HTTP client"),
            base_url,
        }
    }

    async fn check(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        let url = format!("{}/coat-check", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(request)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body: ApiErrorResponse = resp.json().await.unwrap_or_else(|_| ApiErrorResponse {
                error: format!("HTTP {status}"),
                detail: None,
            });
            return Err(ApiError::Upstream {
                error: body.error,
                detail: body.detail,
            });
        }

        resp.json()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))
    }
}

// ── API DTOs ─────────────────────────────────────────────────────────────────

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

enum ApiError {
    Network(String),
    Upstream {
        error: String,
        detail: Option<String>,
    },
}

// ── Form DTO ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CheckForm {
    lat: String,
    lon: String,
    label: String,
}

// ── Templates ────────────────────────────────────────────────────────────────

#[derive(Template, WebTemplate)]
#[template(path = "base.html")]
struct IndexTemplate;

#[derive(Template, WebTemplate)]
#[template(path = "result.html")]
struct ResultTemplate {
    recommendation: String,
    overall_class: String,
    reason: String,
    locations: Vec<LocationView>,
}

struct LocationView {
    display_name: String,
    recommendation_label: String,
    temp_min: String,
    temp_max: String,
    feels_like: String,
    precipitation: String,
    wind: String,
    reasons: Vec<String>,
}

#[derive(Template, WebTemplate)]
#[template(path = "error.html")]
struct ErrorTemplate {
    error: String,
    detail: Option<String>,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn index() -> IndexTemplate {
    IndexTemplate
}

async fn check_handler(
    State(client): State<Arc<ApiClient>>,
    Form(form): Form<CheckForm>,
) -> impl IntoResponse {
    let lat: f64 = match form.lat.parse() {
        Ok(v) => v,
        Err(_) => {
            return ErrorTemplate {
                error: "Invalid location".into(),
                detail: Some("Could not parse latitude.".into()),
            }
            .into_response();
        }
    };
    let lon: f64 = match form.lon.parse() {
        Ok(v) => v,
        Err(_) => {
            return ErrorTemplate {
                error: "Invalid location".into(),
                detail: Some("Could not parse longitude.".into()),
            }
            .into_response();
        }
    };
    let label = if form.label.is_empty() {
        None
    } else {
        Some(form.label)
    };
    let locations = vec![ApiLocation { lat, lon, label }];

    info!(location_count = locations.len(), "coat-check request");

    let request = ApiRequest { locations };
    match client.check(&request).await {
        Ok(resp) => ResultTemplate {
            overall_class: resp.recommendation.clone(),
            recommendation: resp.recommendation,
            reason: resp.reason,
            locations: resp
                .locations
                .into_iter()
                .map(|r| {
                    let display_name = r
                        .label
                        .unwrap_or_else(|| format!("{:.2}, {:.2}", r.lat, r.lon));
                    let recommendation_label = match r.recommendation.as_str() {
                        "no" => "All clear",
                        "umbrella" => "Umbrella",
                        "rain_jacket" => "Rain jacket",
                        "coat" => "Coat",
                        _ => "Unknown",
                    }
                    .to_string();
                    LocationView {
                        display_name,
                        recommendation_label,
                        temp_min: format!("{:.1}\u{00b0}C", r.temp_min_celsius),
                        temp_max: format!("{:.1}\u{00b0}C", r.temp_max_celsius),
                        feels_like: format!("{:.1}\u{00b0}C", r.feels_like_min_celsius),
                        precipitation: format!("{:.1}", r.precipitation_mm),
                        wind: format!("{:.0}", r.wind_speed_max_kmh),
                        reasons: r.reasons,
                    }
                })
                .collect(),
        }
        .into_response(),
        Err(ApiError::Network(msg)) => {
            warn!(error = %msg, "API unreachable");
            ErrorTemplate {
                error: "Weather service unavailable".into(),
                detail: Some(msg),
            }
            .into_response()
        }
        Err(ApiError::Upstream { error, detail }) => {
            warn!(error = %error, "API returned error");
            ErrorTemplate { error, detail }.into_response()
        }
    }
}

// ── Router ───────────────────────────────────────────────────────────────────

fn router(client: ApiClient) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/check", axum::routing::post(check_handler))
        .route("/health", get(|| async { "ok" }))
        .with_state(Arc::new(client))
        .layer(TraceLayer::new_for_http())
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let api_url = std::env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".into());
    let client = ApiClient::new(api_url.clone());
    let app = router(client);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("coat-check-web listening on {} (API: {})", addr, api_url);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|_| panic!("failed to bind to {addr}"));
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

async fn shutdown_signal() {
    let ctrl_c = signal::ctrl_c();
    #[cfg(unix)]
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
        .expect("failed to register SIGTERM handler");
    #[cfg(unix)]
    tokio::select! {
        _ = ctrl_c => {}
        _ = sigterm.recv() => {}
    }
    #[cfg(not(unix))]
    ctrl_c.await.ok();
    tracing::info!("shutdown signal received, draining connections");
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[derive(serde::Serialize)]
    struct TestForm {
        lat: &'static str,
        lon: &'static str,
        label: &'static str,
    }

    async fn test_server(mock_url: &str) -> TestServer {
        let client = ApiClient::new(mock_url.to_string());
        TestServer::new(router(client))
    }

    fn coat_response_json() -> serde_json::Value {
        serde_json::json!({
            "recommendation": "coat",
            "reason": "London: feels like as low as 5.0\u{00b0}C",
            "locations": [{
                "label": "London",
                "lat": 51.5,
                "lon": -0.1,
                "recommendation": "coat",
                "reasons": ["feels like as low as 5.0\u{00b0}C (threshold 12\u{00b0}C)"],
                "temp_max_celsius": 8.0,
                "temp_min_celsius": 3.0,
                "feels_like_min_celsius": 5.0,
                "precipitation_mm": 0.0,
                "wind_speed_max_kmh": 5.0
            }]
        })
    }

    fn no_coat_response_json() -> serde_json::Value {
        serde_json::json!({
            "recommendation": "no",
            "reason": "No coat or jacket needed at any of your locations today.",
            "locations": [{
                "label": "London",
                "lat": 51.5,
                "lon": -0.1,
                "recommendation": "no",
                "reasons": [],
                "temp_max_celsius": 20.0,
                "temp_min_celsius": 15.0,
                "feels_like_min_celsius": 14.5,
                "precipitation_mm": 0.0,
                "wind_speed_max_kmh": 5.0
            }]
        })
    }

    #[tokio::test]
    async fn index_returns_html() {
        let mock = MockServer::start().await;
        let server = test_server(&mock.uri()).await;
        let resp = server.get("/").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("Coat Check"));
        assert!(body.contains("<form"));
    }

    #[tokio::test]
    async fn check_returns_coat() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/coat-check"))
            .respond_with(ResponseTemplate::new(200).set_body_json(coat_response_json()))
            .mount(&mock)
            .await;

        let server = test_server(&mock.uri()).await;
        let resp = server
            .post("/check")
            .form(&TestForm {
                lat: "51.5",
                lon: "-0.1",
                label: "London",
            })
            .await;

        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("Bring a coat"));
        assert!(body.contains("London"));
    }

    #[tokio::test]
    async fn check_returns_no_coat() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/coat-check"))
            .respond_with(ResponseTemplate::new(200).set_body_json(no_coat_response_json()))
            .mount(&mock)
            .await;

        let server = test_server(&mock.uri()).await;
        let resp = server
            .post("/check")
            .form(&TestForm {
                lat: "51.5",
                lon: "-0.1",
                label: "London",
            })
            .await;

        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("No coat needed"));
    }

    #[tokio::test]
    async fn check_invalid_lat() {
        let mock = MockServer::start().await;
        let server = test_server(&mock.uri()).await;
        let resp = server
            .post("/check")
            .form(&TestForm {
                lat: "abc",
                lon: "-0.1",
                label: "London",
            })
            .await;

        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("Invalid location"));
    }

    #[tokio::test]
    async fn check_api_down() {
        let server = test_server("http://127.0.0.1:1").await;
        let resp = server
            .post("/check")
            .form(&TestForm {
                lat: "51.5",
                lon: "-0.1",
                label: "London",
            })
            .await;

        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("unavailable") || body.contains("error"));
    }

    #[tokio::test]
    async fn health_endpoint() {
        let mock = MockServer::start().await;
        let server = test_server(&mock.uri()).await;
        let resp = server.get("/health").await;
        resp.assert_status_ok();
    }
}
