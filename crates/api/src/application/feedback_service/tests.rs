use super::*;
use crate::{
    domain::{
        location::Location,
        recommendation::{CoatRecommendation, LocationRecommendation},
        weather::DailyForecast,
    },
    ports::outbound::{
        FeedbackStoreError, FeedbackStorePort, QueuedNotification, WeatherPort, WeatherPortError,
    },
};
use chrono::{DateTime, NaiveDate, Utc};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct FakeStore {
    predictions: Arc<Mutex<Vec<Prediction>>>,
}

impl FakeStore {
    fn new() -> Self {
        Self {
            predictions: Arc::new(Mutex::new(vec![])),
        }
    }
}

#[async_trait::async_trait]
impl FeedbackStorePort for FakeStore {
    async fn save_prediction(
        &self,
        recommendation: &str,
        reason: &str,
        locations: &[PredictionLocation],
    ) -> Result<Uuid, FeedbackStoreError> {
        let id = Uuid::new_v4();
        self.predictions.lock().unwrap().push(Prediction {
            id,
            recommendation: recommendation.to_string(),
            reason: reason.to_string(),
            locations: locations.to_vec(),
            created_at: Utc::now(),
        });
        Ok(id)
    }

    async fn get_prediction(&self, id: Uuid) -> Result<Prediction, FeedbackStoreError> {
        self.predictions
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or(FeedbackStoreError::NotFound)
    }

    async fn enqueue_notification(
        &self,
        _prediction_id: Uuid,
        _contact: &str,
        _send_after: DateTime<Utc>,
    ) -> Result<(), FeedbackStoreError> {
        Ok(())
    }

    async fn fetch_ready_notifications(
        &self,
    ) -> Result<Vec<QueuedNotification>, FeedbackStoreError> {
        Ok(vec![])
    }

    async fn delete_notification(&self, _id: Uuid) -> Result<(), FeedbackStoreError> {
        Ok(())
    }

    async fn mark_notification_sent(&self, _id: Uuid) -> Result<(), FeedbackStoreError> {
        Ok(())
    }

    async fn mark_notification_failed(
        &self,
        _id: Uuid,
        _error: &str,
    ) -> Result<(), FeedbackStoreError> {
        Ok(())
    }

    async fn record_feedback(
        &self,
        _prediction: &Prediction,
        _feedback: &Feedback,
        _actual_weather: Option<&[PredictionLocation]>,
    ) -> Result<(), FeedbackStoreError> {
        Ok(())
    }

    async fn cancel_pending_notifications_for_contact(
        &self,
        _contact: &str,
    ) -> Result<(), FeedbackStoreError> {
        Ok(())
    }
}

#[derive(Clone)]
struct FakeWeather;

#[async_trait::async_trait]
impl WeatherPort for FakeWeather {
    async fn fetch_daily_forecast(
        &self,
        location: &Location,
    ) -> Result<DailyForecast, WeatherPortError> {
        Ok(DailyForecast {
            location: location.clone(),
            timezone: "Europe/London".into(),
            temp_max_celsius: 10.0,
            temp_min_celsius: 5.0,
            feels_like_min_celsius: 3.0,
            precipitation_mm: 0.0,
            wind_speed_max_kmh: 10.0,
            snowfall_cm: 0.0,
            weather_code: 0,
        })
    }

    async fn fetch_daily_observation(
        &self,
        location: &Location,
        _date: NaiveDate,
    ) -> Result<DailyForecast, WeatherPortError> {
        Ok(DailyForecast {
            location: location.clone(),
            timezone: "Europe/London".into(),
            temp_max_celsius: 12.0,
            temp_min_celsius: 6.0,
            feels_like_min_celsius: 4.0,
            precipitation_mm: 2.0,
            wind_speed_max_kmh: 15.0,
            snowfall_cm: 0.0,
            weather_code: 3,
        })
    }
}

fn coat_decision() -> CoatDecision {
    let loc = Location::new(51.5, -0.1, Some("London".into())).unwrap();
    CoatDecision {
        overall: CoatRecommendation::Coat,
        by_location: vec![LocationRecommendation {
            location: loc,
            timezone: "Europe/London".into(),
            recommendation: CoatRecommendation::Coat,
            reasons: vec!["feels like as low as 5.0\u{00b0}C".into()],
            temp_max_celsius: 8.0,
            temp_min_celsius: 3.0,
            feels_like_min_celsius: 5.0,
            precipitation_mm: 0.0,
            wind_speed_max_kmh: 10.0,
            snowfall_cm: 0.0,
            weather_code: 0,
        }],
        overall_reason: "London: feels like as low as 5.0\u{00b0}C".into(),
    }
}

fn service() -> FeedbackService<FakeStore, FakeWeather> {
    FeedbackService::new(FakeStore::new(), FakeWeather, 480)
}

fn service_with_store(store: FakeStore) -> FeedbackService<FakeStore, FakeWeather> {
    FeedbackService::new(store, FakeWeather, 480)
}

#[tokio::test]
async fn save_prediction_returns_uuid() {
    let svc = service();
    let id = svc.save_prediction(&coat_decision()).await.unwrap();
    assert!(!id.is_nil());
}

#[tokio::test]
async fn register_email_valid() {
    let store = FakeStore::new();
    let svc = service_with_store(store);
    let id = svc.save_prediction(&coat_decision()).await.unwrap();
    assert!(svc.register_contact(id, "test@example.com").await.is_ok());
}

#[tokio::test]
async fn register_contact_invalid() {
    let svc = service();
    let id = Uuid::new_v4();
    let err = svc.register_contact(id, "notanemail").await.unwrap_err();
    assert!(matches!(err, FeedbackError::InvalidContact));
}

#[tokio::test]
async fn get_prediction_found() {
    let store = FakeStore::new();
    let svc = service_with_store(store);
    let id = svc.save_prediction(&coat_decision()).await.unwrap();
    let prediction = svc.get_prediction(id).await.unwrap();
    assert_eq!(prediction.recommendation, "coat");
}

#[tokio::test]
async fn get_prediction_not_found() {
    let svc = service();
    let err = svc.get_prediction(Uuid::new_v4()).await.unwrap_err();
    assert!(matches!(err, FeedbackError::PredictionNotFound));
}

#[tokio::test]
async fn submit_feedback_ok() {
    let decision = coat_decision();
    let store = FakeStore::new();
    let svc = service_with_store(store);
    let id = svc.save_prediction(&decision).await.unwrap();
    assert!(svc.submit_feedback(id, "coat", "coat", None).await.is_ok());
}

#[tokio::test]
async fn submit_feedback_prediction_not_found() {
    let svc = service();
    let err = svc
        .submit_feedback(Uuid::new_v4(), "coat", "coat", None)
        .await
        .unwrap_err();
    assert!(matches!(err, FeedbackError::PredictionNotFound));
}

#[tokio::test]
async fn register_contact_prediction_not_found() {
    let svc = service();
    let err = svc
        .register_contact(Uuid::new_v4(), "test@example.com")
        .await
        .unwrap_err();
    assert!(matches!(err, FeedbackError::PredictionNotFound));
}

// ── Specialized fakes for edge-case tests ─────────────────────────────

#[derive(Clone)]
struct AlreadyExistsStore {
    inner: FakeStore,
}

#[async_trait::async_trait]
impl FeedbackStorePort for AlreadyExistsStore {
    async fn save_prediction(
        &self,
        rec: &str,
        reason: &str,
        locs: &[PredictionLocation],
    ) -> Result<Uuid, FeedbackStoreError> {
        self.inner.save_prediction(rec, reason, locs).await
    }

    async fn get_prediction(&self, id: Uuid) -> Result<Prediction, FeedbackStoreError> {
        self.inner.get_prediction(id).await
    }

    async fn enqueue_notification(
        &self,
        pid: Uuid,
        contact: &str,
        send_after: DateTime<Utc>,
    ) -> Result<(), FeedbackStoreError> {
        self.inner
            .enqueue_notification(pid, contact, send_after)
            .await
    }

    async fn fetch_ready_notifications(
        &self,
    ) -> Result<Vec<QueuedNotification>, FeedbackStoreError> {
        Ok(vec![])
    }

    async fn delete_notification(&self, _id: Uuid) -> Result<(), FeedbackStoreError> {
        Ok(())
    }

    async fn mark_notification_sent(&self, _id: Uuid) -> Result<(), FeedbackStoreError> {
        Ok(())
    }

    async fn mark_notification_failed(
        &self,
        _id: Uuid,
        _error: &str,
    ) -> Result<(), FeedbackStoreError> {
        Ok(())
    }

    async fn record_feedback(
        &self,
        _prediction: &Prediction,
        _feedback: &Feedback,
        _actual_weather: Option<&[PredictionLocation]>,
    ) -> Result<(), FeedbackStoreError> {
        Err(FeedbackStoreError::AlreadyExists)
    }

    async fn cancel_pending_notifications_for_contact(
        &self,
        _contact: &str,
    ) -> Result<(), FeedbackStoreError> {
        Ok(())
    }
}

#[derive(Clone)]
struct FailingWeather;

#[async_trait::async_trait]
impl WeatherPort for FailingWeather {
    async fn fetch_daily_forecast(
        &self,
        _location: &Location,
    ) -> Result<DailyForecast, WeatherPortError> {
        Err(WeatherPortError::Network("timeout".into()))
    }

    async fn fetch_daily_observation(
        &self,
        _location: &Location,
        _date: NaiveDate,
    ) -> Result<DailyForecast, WeatherPortError> {
        Err(WeatherPortError::Network("timeout".into()))
    }
}

#[tokio::test]
async fn submit_feedback_already_rated() {
    let store = AlreadyExistsStore {
        inner: FakeStore::new(),
    };
    let svc = FeedbackService::new(store.clone(), FakeWeather, 480);
    let id = svc.save_prediction(&coat_decision()).await.unwrap();
    let err = svc
        .submit_feedback(id, "coat", "coat", None)
        .await
        .unwrap_err();
    assert!(matches!(err, FeedbackError::AlreadyRated));
}

#[tokio::test]
async fn submit_feedback_records_without_actual_weather_on_failure() {
    let store = FakeStore::new();
    let svc = FeedbackService::new(store.clone(), FailingWeather, 480);
    let id = svc.save_prediction(&coat_decision()).await.unwrap();
    assert!(svc.submit_feedback(id, "coat", "coat", None).await.is_ok());
}

// ── Sanitisation tests ───────────────────────────────────────────────

#[test]
fn sanitise_strips_html_tags() {
    let input = Some("<script>alert('xss')</script>hello <b>world</b>".into());
    let result = super::sanitise_comment(input);
    assert_eq!(result, Some("hello world".to_string()));
}

#[test]
fn sanitise_trims_whitespace() {
    let input = Some("  hello world  ".into());
    let result = super::sanitise_comment(input);
    assert_eq!(result, Some("hello world".to_string()));
}

#[test]
fn sanitise_collapses_empty_to_none() {
    let input = Some("   ".into());
    assert_eq!(super::sanitise_comment(input), None);
}

#[test]
fn sanitise_collapses_html_only_to_none() {
    let input = Some("<script>alert(1)</script>".into());
    assert_eq!(super::sanitise_comment(input), None);
}

#[test]
fn sanitise_passes_through_none() {
    assert_eq!(super::sanitise_comment(None), None);
}

#[test]
fn sanitise_preserves_plain_text() {
    let input = Some("Great prediction, thanks!".into());
    assert_eq!(
        super::sanitise_comment(input),
        Some("Great prediction, thanks!".to_string())
    );
}
