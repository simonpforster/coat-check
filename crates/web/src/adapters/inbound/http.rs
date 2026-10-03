use std::sync::Arc;

use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Form, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use serde::Deserialize;
use tower_http::trace::TraceLayer;
use tracing::warn;

use crate::{
    domain::{recommendation::LocationView, suggestion::Suggestion},
    ports::inbound::{LocationInput, WebPort, WebPortError},
};

// ── Config ──────────────────────────────────────────────────────────────────

pub struct WebConfig {
    pub base_url: Option<String>,
    pub ga_id: Option<String>,
}

struct AppContext<P: WebPort> {
    service: P,
    config: WebConfig,
}

// ── Templates ───────────────────────────────────────────────────────────────

#[derive(Template, WebTemplate)]
#[template(path = "base.html")]
struct IndexTemplate {
    base_url: Option<String>,
    ga_id: Option<String>,
}

#[derive(Template, WebTemplate)]
#[template(path = "result.html")]
struct ResultTemplate {
    recommendation: String,
    overall_class: String,
    reason: String,
    locations: Vec<LocationView>,
}

#[derive(Template, WebTemplate)]
#[template(path = "error.html")]
struct ErrorTemplate {
    error: String,
    detail: Option<String>,
}

#[derive(Template, WebTemplate)]
#[template(path = "suggestions.html")]
struct SuggestionsTemplate {
    suggestions: Vec<Suggestion>,
}

// ── Form DTOs ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CheckForm {
    locations: String,
}

#[derive(Deserialize)]
struct FormLocation {
    lat: String,
    lon: String,
    label: Option<String>,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

// ── Router ──────────────────────────────────────────────────────────────────

pub fn router<P>(service: P, config: WebConfig) -> Router
where
    P: WebPort + Clone + 'static,
{
    Router::new()
        .route("/", get(index::<P>))
        .route("/search", get(search_handler::<P>))
        .route("/check", axum::routing::post(check_handler::<P>))
        .route("/health", get(|| async { "ok" }))
        .with_state(Arc::new(AppContext { service, config }))
        .layer(TraceLayer::new_for_http())
}

// ── Handlers ────────────────────────────────────────────────────────────────

async fn index<P: WebPort>(State(ctx): State<Arc<AppContext<P>>>) -> IndexTemplate {
    IndexTemplate {
        base_url: ctx.config.base_url.clone(),
        ga_id: ctx.config.ga_id.clone(),
    }
}

async fn search_handler<P: WebPort>(
    State(ctx): State<Arc<AppContext<P>>>,
    axum::extract::Query(query): axum::extract::Query<SearchQuery>,
) -> impl IntoResponse {
    match ctx.service.search_locations(&query.q).await {
        Ok(suggestions) => SuggestionsTemplate { suggestions }.into_response(),
        Err(e) => {
            warn!(error = %e, "search failed");
            SuggestionsTemplate {
                suggestions: vec![],
            }
            .into_response()
        }
    }
}

async fn check_handler<P: WebPort>(
    State(ctx): State<Arc<AppContext<P>>>,
    Form(form): Form<CheckForm>,
) -> impl IntoResponse {
    let form_locations: Vec<FormLocation> = match serde_json::from_str(&form.locations) {
        Ok(locs) => locs,
        Err(_) => {
            return ErrorTemplate {
                error: "Invalid request".into(),
                detail: Some("Could not parse locations.".into()),
            }
            .into_response();
        }
    };

    let locations: Vec<LocationInput> = form_locations
        .into_iter()
        .map(|l| LocationInput {
            lat: l.lat,
            lon: l.lon,
            label: l.label,
        })
        .collect();

    match ctx.service.check_coat(locations).await {
        Ok(result) => ResultTemplate {
            overall_class: result.recommendation.clone(),
            recommendation: result.recommendation,
            reason: result.reason,
            locations: result.locations,
        }
        .into_response(),
        Err(WebPortError::NoLocations) => ErrorTemplate {
            error: "No locations selected".into(),
            detail: Some("Please add at least one location.".into()),
        }
        .into_response(),
        Err(WebPortError::InvalidLatitude) => ErrorTemplate {
            error: "Invalid location".into(),
            detail: Some("Could not parse latitude.".into()),
        }
        .into_response(),
        Err(WebPortError::InvalidLongitude) => ErrorTemplate {
            error: "Invalid location".into(),
            detail: Some("Could not parse longitude.".into()),
        }
        .into_response(),
        Err(WebPortError::ServiceUnavailable(msg)) => {
            warn!(error = %msg, "API unreachable");
            ErrorTemplate {
                error: "Weather service unavailable".into(),
                detail: Some(msg),
            }
            .into_response()
        }
        Err(WebPortError::UpstreamError { error, detail }) => {
            warn!(error = %error, "API returned error");
            ErrorTemplate { error, detail }.into_response()
        }
        Err(WebPortError::GeocodingFailed(msg)) => {
            warn!(error = %msg, "unexpected geocoding error in check");
            ErrorTemplate {
                error: "Unexpected error".into(),
                detail: Some(msg),
            }
            .into_response()
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            recommendation::{CheckResult, LocationView},
            suggestion::Suggestion,
        },
        ports::inbound::{LocationInput, WebPort, WebPortError},
    };
    use axum_test::TestServer;

    #[derive(Clone)]
    struct AlwaysCoat;

    #[async_trait::async_trait]
    impl WebPort for AlwaysCoat {
        async fn check_coat(
            &self,
            _locations: Vec<LocationInput>,
        ) -> Result<CheckResult, WebPortError> {
            Ok(CheckResult {
                recommendation: "coat".into(),
                reason: "London: feels like as low as 5.0\u{00b0}C".into(),
                locations: vec![LocationView {
                    display_name: "London".into(),
                    recommendation_label: "Coat".into(),
                    temp_min: "3.0\u{00b0}C".into(),
                    temp_max: "8.0\u{00b0}C".into(),
                    feels_like: "5.0\u{00b0}C".into(),
                    precipitation: "0.0".into(),
                    wind: "5".into(),
                    reasons: vec![
                        "feels like as low as 5.0\u{00b0}C (threshold 12\u{00b0}C)".into(),
                    ],
                }],
            })
        }

        async fn search_locations(
            &self,
            _query: &str,
        ) -> Result<Vec<Suggestion>, WebPortError> {
            Ok(vec![])
        }
    }

    #[derive(Clone)]
    struct AlwaysNo;

    #[async_trait::async_trait]
    impl WebPort for AlwaysNo {
        async fn check_coat(
            &self,
            _locations: Vec<LocationInput>,
        ) -> Result<CheckResult, WebPortError> {
            Ok(CheckResult {
                recommendation: "no".into(),
                reason: "No coat or jacket needed at any of your locations today.".into(),
                locations: vec![LocationView {
                    display_name: "London".into(),
                    recommendation_label: "All clear".into(),
                    temp_min: "15.0\u{00b0}C".into(),
                    temp_max: "20.0\u{00b0}C".into(),
                    feels_like: "14.5\u{00b0}C".into(),
                    precipitation: "0.0".into(),
                    wind: "5".into(),
                    reasons: vec![],
                }],
            })
        }

        async fn search_locations(
            &self,
            _query: &str,
        ) -> Result<Vec<Suggestion>, WebPortError> {
            Ok(vec![])
        }
    }

    #[derive(Clone)]
    struct InvalidLat;

    #[async_trait::async_trait]
    impl WebPort for InvalidLat {
        async fn check_coat(
            &self,
            _locations: Vec<LocationInput>,
        ) -> Result<CheckResult, WebPortError> {
            Err(WebPortError::InvalidLatitude)
        }

        async fn search_locations(
            &self,
            _query: &str,
        ) -> Result<Vec<Suggestion>, WebPortError> {
            Ok(vec![])
        }
    }

    #[derive(Clone)]
    struct ServiceDown;

    #[async_trait::async_trait]
    impl WebPort for ServiceDown {
        async fn check_coat(
            &self,
            _locations: Vec<LocationInput>,
        ) -> Result<CheckResult, WebPortError> {
            Err(WebPortError::ServiceUnavailable(
                "connection refused".into(),
            ))
        }

        async fn search_locations(
            &self,
            _query: &str,
        ) -> Result<Vec<Suggestion>, WebPortError> {
            Ok(vec![])
        }
    }

    #[derive(Clone)]
    struct WithSuggestions;

    #[async_trait::async_trait]
    impl WebPort for WithSuggestions {
        async fn check_coat(
            &self,
            _locations: Vec<LocationInput>,
        ) -> Result<CheckResult, WebPortError> {
            Err(WebPortError::ServiceUnavailable("unused".into()))
        }

        async fn search_locations(
            &self,
            query: &str,
        ) -> Result<Vec<Suggestion>, WebPortError> {
            if query.trim().len() < 2 {
                return Ok(vec![]);
            }
            Ok(vec![Suggestion {
                name: "Reading, England, United Kingdom".into(),
                lat: "51.456250".into(),
                lon: "-0.971130".into(),
            }])
        }
    }

    fn config() -> WebConfig {
        WebConfig {
            base_url: None,
            ga_id: None,
        }
    }

    fn server<P: WebPort + Clone + 'static>(svc: P) -> TestServer {
        TestServer::new(router(svc, config()))
    }

    fn locations_json(locs: &[(&str, &str, &str)]) -> String {
        let arr: Vec<serde_json::Value> = locs
            .iter()
            .map(|(lat, lon, label)| {
                serde_json::json!({"lat": lat, "lon": lon, "label": label})
            })
            .collect();
        serde_json::to_string(&arr).unwrap()
    }

    #[derive(serde::Serialize)]
    struct TestForm {
        locations: String,
    }

    #[tokio::test]
    async fn index_returns_html() {
        let s = server(AlwaysNo);
        let resp = s.get("/").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("Coat Check"));
        assert!(body.contains("<form"));
    }

    #[tokio::test]
    async fn check_returns_coat() {
        let s = server(AlwaysCoat);
        let resp = s
            .post("/check")
            .form(&TestForm {
                locations: locations_json(&[("51.5", "-0.1", "London")]),
            })
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("Bring a coat"));
        assert!(body.contains("London"));
    }

    #[tokio::test]
    async fn check_returns_no_coat() {
        let s = server(AlwaysNo);
        let resp = s
            .post("/check")
            .form(&TestForm {
                locations: locations_json(&[("51.5", "-0.1", "London")]),
            })
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("No coat needed"));
    }

    #[tokio::test]
    async fn check_invalid_lat() {
        let s = server(InvalidLat);
        let resp = s
            .post("/check")
            .form(&TestForm {
                locations: locations_json(&[("abc", "-0.1", "London")]),
            })
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("Invalid location"));
    }

    #[tokio::test]
    async fn check_api_down() {
        let s = server(ServiceDown);
        let resp = s
            .post("/check")
            .form(&TestForm {
                locations: locations_json(&[("51.5", "-0.1", "London")]),
            })
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("unavailable") || body.contains("error"));
    }

    #[tokio::test]
    async fn check_invalid_json() {
        let s = server(AlwaysNo);
        let resp = s
            .post("/check")
            .form(&TestForm {
                locations: "not json".into(),
            })
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("Invalid request"));
    }

    #[tokio::test]
    async fn health_endpoint() {
        let s = server(AlwaysNo);
        let resp = s.get("/health").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn search_short_query_returns_empty() {
        let s = server(WithSuggestions);
        let resp = s.get("/search").add_query_param("q", "L").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(!body.contains("suggestion"));
    }

    #[tokio::test]
    async fn search_returns_suggestions() {
        let s = server(WithSuggestions);
        let resp = s
            .get("/search")
            .add_query_param("q", "Reading")
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("suggestion"));
        assert!(body.contains("Reading"));
        assert!(body.contains("United Kingdom"));
    }
}
