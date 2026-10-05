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

    async fn register_email(&self, _prediction_id: &str, _email: &str) -> Result<(), WebPortError> {
        Ok(())
    }

    async fn get_prediction(
        &self,
        _prediction_id: &str,
    ) -> Result<PredictionResponse, WebPortError> {
        Ok(PredictionResponse {
            recommendation: "coat".into(),
            reason: "test reason".into(),
        })
    }

    async fn submit_feedback(
        &self,
        _prediction_id: &str,
        _brought: &str,
        _should_have_brought: &str,
        _comment: Option<&str>,
    ) -> Result<(), WebPortError> {
        Ok(())
    }

    async fn unsubscribe(&self, _contact: &str) -> Result<(), WebPortError> {
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
        .add_header("origin", "http://localhost")
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
        .add_header("origin", "http://localhost")
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
        .add_header("origin", "http://localhost")
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
        .add_header("origin", "http://localhost")
        .form(&TestForm {
            locations: locations_json(&[("51.5", "-0.1", "London")]),
        })
        .await;
    resp.assert_status_ok();
    let body = resp.text();
    assert!(body.contains("Weather service unavailable"));
}

#[tokio::test]
async fn check_invalid_json() {
    let s = server(FakeWebPort::always_no());
    let resp = s
        .post("/check")
        .add_header("origin", "http://localhost")
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
        .add_header("origin", "http://localhost")
        .form(&serde_json::json!({"prediction_id": "pred-123", "email": "test@example.com"}))
        .await;
    resp.assert_status_ok();
    let body = resp.text();
    assert!(body.contains("send you a feedback request later today"));
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
    assert!(body.contains("How was our prediction?"));
    assert!(body.contains("What did you bring?"));
    assert!(body.contains("Submit feedback"));
}

#[tokio::test]
async fn feedback_submission_success() {
    let s = server(FakeWebPort::always_coat());
    let resp = s
        .post("/feedback")
        .add_header("origin", "http://localhost")
        .form(&serde_json::json!({"token": "pred-123", "brought": "coat", "should_have_brought": "coat"}))
        .await;
    resp.assert_status_ok();
    let body = resp.text();
    assert!(body.contains("Thank you for your feedback!"));
}
