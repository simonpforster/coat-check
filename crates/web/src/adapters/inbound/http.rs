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
    prediction_id: Option<String>,
    recommendation: String,
    overall_class: String,
    reason: String,
    locations: Vec<LocationView>,
}

#[derive(Template, WebTemplate)]
#[template(path = "email_success.html")]
struct EmailSuccessTemplate;

#[derive(Template, WebTemplate)]
#[template(path = "feedback.html")]
struct FeedbackTemplate {
    token: String,
    recommendation: String,
    reason: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "feedback_thanks.html")]
struct FeedbackThanksTemplate;

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

#[derive(Deserialize)]
struct EmailForm {
    prediction_id: String,
    email: String,
}

#[derive(Deserialize)]
struct FeedbackQuery {
    token: String,
}

#[derive(Deserialize)]
struct FeedbackForm {
    token: String,
    accurate: bool,
    comment: Option<String>,
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
        .route("/email", axum::routing::post(email_handler::<P>))
        .route(
            "/feedback",
            get(feedback_page::<P>).post(feedback_handler::<P>),
        )
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
            prediction_id: result.prediction_id,
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
        Err(e) => {
            warn!(error = %e, "unexpected error in check");
            ErrorTemplate {
                error: "Unexpected error".into(),
                detail: None,
            }
            .into_response()
        }
    }
}

async fn email_handler<P: WebPort>(
    State(ctx): State<Arc<AppContext<P>>>,
    Form(form): Form<EmailForm>,
) -> impl IntoResponse {
    match ctx
        .service
        .register_email(&form.prediction_id, &form.email)
        .await
    {
        Ok(()) => EmailSuccessTemplate.into_response(),
        Err(e) => {
            warn!(error = %e, "email registration failed");
            ErrorTemplate {
                error: "Could not register email".into(),
                detail: Some(e.to_string()),
            }
            .into_response()
        }
    }
}

async fn feedback_page<P: WebPort>(
    State(ctx): State<Arc<AppContext<P>>>,
    axum::extract::Query(query): axum::extract::Query<FeedbackQuery>,
) -> impl IntoResponse {
    match ctx.service.get_prediction(&query.token).await {
        Ok(prediction) => FeedbackTemplate {
            token: query.token,
            recommendation: prediction.recommendation,
            reason: prediction.reason,
        }
        .into_response(),
        Err(e) => {
            warn!(error = %e, "failed to load prediction for feedback");
            ErrorTemplate {
                error: "Prediction not found".into(),
                detail: Some("This feedback link may have expired.".into()),
            }
            .into_response()
        }
    }
}

async fn feedback_handler<P: WebPort>(
    State(ctx): State<Arc<AppContext<P>>>,
    Form(form): Form<FeedbackForm>,
) -> impl IntoResponse {
    match ctx
        .service
        .submit_feedback(&form.token, form.accurate, form.comment.as_deref())
        .await
    {
        Ok(()) => FeedbackThanksTemplate.into_response(),
        Err(e) => {
            warn!(error = %e, "feedback submission failed");
            ErrorTemplate {
                error: "Could not submit feedback".into(),
                detail: Some(e.to_string()),
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
        ports::{
            inbound::{LocationInput, WebPort, WebPortError},
            outbound::PredictionResponse,
        },
    };
    use axum_test::TestServer;

    // All test fakes share a default `FakeWebPort` that handles the feedback
    // methods, then each specialization overrides check_coat/search_locations.

    #[derive(Clone)]
    struct FakeWebPort {
        check_result: Option<Result<CheckResult, WebPortError>>,
        search_fn: Option<fn(&str) -> Result<Vec<Suggestion>, WebPortError>>,
    }

    impl FakeWebPort {
        fn always_coat() -> Self {
            Self {
                check_result: Some(Ok(CheckResult {
                    prediction_id: Some("pred-123".into()),
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
                            "feels like as low as 5.0\u{00b0}C (threshold 12\u{00b0}C)".into()
                        ],
                    }],
                })),
                search_fn: None,
            }
        }

        fn always_no() -> Self {
            Self {
                check_result: Some(Ok(CheckResult {
                    prediction_id: Some("pred-456".into()),
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
                })),
                search_fn: None,
            }
        }

        fn invalid_lat() -> Self {
            Self {
                check_result: Some(Err(WebPortError::InvalidLatitude)),
                search_fn: None,
            }
        }

        fn service_down() -> Self {
            Self {
                check_result: Some(Err(WebPortError::ServiceUnavailable(
                    "connection refused".into(),
                ))),
                search_fn: None,
            }
        }

        fn with_suggestions() -> Self {
            fn search(query: &str) -> Result<Vec<Suggestion>, WebPortError> {
                if query.trim().len() < 2 {
                    return Ok(vec![]);
                }
                Ok(vec![Suggestion {
                    name: "Reading, England, United Kingdom".into(),
                    lat: "51.456250".into(),
                    lon: "-0.971130".into(),
                }])
            }
            Self {
                check_result: Some(Err(WebPortError::ServiceUnavailable("unused".into()))),
                search_fn: Some(search),
            }
        }
    }

    #[async_trait::async_trait]
    impl WebPort for FakeWebPort {
        async fn check_coat(
            &self,
            _locations: Vec<LocationInput>,
        ) -> Result<CheckResult, WebPortError> {
            self.check_result.clone().unwrap()
        }

        async fn search_locations(&self, query: &str) -> Result<Vec<Suggestion>, WebPortError> {
            match self.search_fn {
                Some(f) => f(query),
                None => Ok(vec![]),
            }
        }

        async fn register_email(
            &self,
            _prediction_id: &str,
            _email: &str,
        ) -> Result<(), WebPortError> {
            Ok(())
        }

        async fn get_prediction(
            &self,
            prediction_id: &str,
        ) -> Result<PredictionResponse, WebPortError> {
            Ok(PredictionResponse {
                id: prediction_id.to_string(),
                recommendation: "coat".into(),
                reason: "test reason".into(),
            })
        }

        async fn submit_feedback(
            &self,
            _prediction_id: &str,
            _accurate: bool,
            _comment: Option<&str>,
        ) -> Result<(), WebPortError> {
            Ok(())
        }
    }

    fn config() -> WebConfig {
        WebConfig {
            base_url: None,
            ga_id: None,
        }
    }

    fn server(svc: FakeWebPort) -> TestServer {
        TestServer::new(router(svc, config()))
    }

    fn locations_json(locs: &[(&str, &str, &str)]) -> String {
        let arr: Vec<serde_json::Value> = locs
            .iter()
            .map(|(lat, lon, label)| serde_json::json!({"lat": lat, "lon": lon, "label": label}))
            .collect();
        serde_json::to_string(&arr).unwrap()
    }

    #[derive(serde::Serialize)]
    struct TestForm {
        locations: String,
    }

    #[tokio::test]
    async fn index_returns_html() {
        let s = server(FakeWebPort::always_no());
        let resp = s.get("/").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("Coat Check"));
        assert!(body.contains("<form"));
    }

    #[tokio::test]
    async fn check_returns_coat() {
        let s = server(FakeWebPort::always_coat());
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
        let s = server(FakeWebPort::always_no());
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
        let s = server(FakeWebPort::invalid_lat());
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
        let s = server(FakeWebPort::service_down());
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
        let s = server(FakeWebPort::always_no());
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
        let s = server(FakeWebPort::always_no());
        let resp = s.get("/health").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn search_short_query_returns_empty() {
        let s = server(FakeWebPort::with_suggestions());
        let resp = s.get("/search").add_query_param("q", "L").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(!body.contains("suggestion"));
    }

    #[tokio::test]
    async fn search_returns_suggestions() {
        let s = server(FakeWebPort::with_suggestions());
        let resp = s.get("/search").add_query_param("q", "Reading").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("suggestion"));
        assert!(body.contains("Reading"));
        assert!(body.contains("United Kingdom"));
    }

    #[tokio::test]
    async fn email_registration_success() {
        let s = server(FakeWebPort::always_coat());
        let resp = s
            .post("/email")
            .form(&serde_json::json!({"prediction_id": "pred-123", "email": "test@example.com"}))
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("Thank") || body.contains("email"));
    }

    #[tokio::test]
    async fn feedback_page_loads() {
        let s = server(FakeWebPort::always_coat());
        let resp = s
            .get("/feedback")
            .add_query_param("token", "pred-123")
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("feedback") || body.contains("Feedback"));
    }

    #[tokio::test]
    async fn feedback_submission_success() {
        let s = server(FakeWebPort::always_coat());
        let resp = s
            .post("/feedback")
            .form(&serde_json::json!({"token": "pred-123", "accurate": true}))
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("Thank") || body.contains("thank"));
    }
}
