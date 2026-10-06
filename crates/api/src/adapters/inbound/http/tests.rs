use super::*;
use crate::{
    domain::{
        location::Location,
        prediction::Prediction,
        recommendation::{CoatDecision, CoatRecommendation, LocationRecommendation},
    },
    ports::inbound::{CoatCheckError, CoatCheckPort, FeedbackError, FeedbackPort},
};
use axum::http::StatusCode;
use axum_test::TestServer;
use uuid::Uuid;

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
                timezone: "Europe/London".into(),
                recommendation: CoatRecommendation::No,
                reasons: vec![],
                temp_max_celsius: 20.0,
                temp_min_celsius: 15.0,
                feels_like_min_celsius: 14.5,
                precipitation_mm: 0.0,
                wind_speed_max_kmh: 5.0,
                snowfall_cm: 0.0,
                weather_code: 0,
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
                timezone: "Europe/London".into(),
                recommendation: CoatRecommendation::Coat,
                reasons: vec!["feels like as low as 5.0°C (threshold 12°C)".into()],
                temp_max_celsius: 8.0,
                temp_min_celsius: 3.0,
                feels_like_min_celsius: 5.0,
                precipitation_mm: 0.0,
                wind_speed_max_kmh: 5.0,
                snowfall_cm: 0.0,
                weather_code: 0,
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
                timezone: "Europe/London".into(),
                recommendation: CoatRecommendation::RainJacket,
                reasons: vec!["8.0 mm precipitation expected".into()],
                temp_max_celsius: 22.0,
                temp_min_celsius: 15.0,
                feels_like_min_celsius: 14.0,
                precipitation_mm: 8.0,
                wind_speed_max_kmh: 5.0,
                snowfall_cm: 0.0,
                weather_code: 0,
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
        _brought: &str,
        _should_have_brought: &str,
        _comment: Option<String>,
    ) -> Result<(), FeedbackError> {
        Ok(())
    }

    async fn unsubscribe(&self, _contact: &str) -> Result<(), FeedbackError> {
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
        Err(FeedbackError::PredictionNotFound)
    }

    async fn submit_feedback(
        &self,
        _prediction_id: Uuid,
        _brought: &str,
        _should_have_brought: &str,
        _comment: Option<String>,
    ) -> Result<(), FeedbackError> {
        Err(FeedbackError::AlreadyRated)
    }

    async fn unsubscribe(&self, _contact: &str) -> Result<(), FeedbackError> {
        Ok(())
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
async fn submit_feedback_ok() {
    let s = server(AlwaysNo);
    let resp = s
        .post("/feedback/submit")
        .json(&serde_json::json!({
            "prediction_id": Uuid::new_v4().to_string(),
            "brought": "coat",
            "should_have_brought": "coat",
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
            "brought": "coat",
            "should_have_brought": "rain_jacket",
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
            "brought": "coat",
            "should_have_brought": "coat",
            "comment": long_comment,
        }))
        .await;
    resp.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["error"], "comment_too_long");
}
