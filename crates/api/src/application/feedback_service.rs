use chrono::{Duration, Utc};
use email_address::EmailAddress;
use futures::future::try_join_all;
use tracing::warn;
use uuid::Uuid;

use crate::{
    domain::{
        location::Location,
        prediction::{Feedback, Prediction, PredictionLocation},
        recommendation::CoatDecision,
    },
    ports::{
        inbound::{FeedbackError, FeedbackPort},
        outbound::{FeedbackStoreError, FeedbackStorePort, WeatherPort},
    },
};

const FEEDBACK_WINDOW_DAYS: i64 = 7;

#[derive(Clone)]
pub struct FeedbackService<F: FeedbackStorePort + Clone, W: WeatherPort + Clone> {
    store: F,
    weather: W,
    feedback_delay_minutes: i64,
}

impl<F: FeedbackStorePort + Clone, W: WeatherPort + Clone> FeedbackService<F, W> {
    pub fn new(store: F, weather: W, feedback_delay_minutes: i64) -> Self {
        Self {
            store,
            weather,
            feedback_delay_minutes,
        }
    }
}

impl<F: FeedbackStorePort + Clone, W: WeatherPort + Clone> FeedbackService<F, W> {
    async fn check_feedback_window(&self, prediction_id: Uuid) -> Result<(), FeedbackError> {
        match self.store.get_notification_sent_at(prediction_id).await {
            Ok(sent_at) => {
                let expiry = sent_at + Duration::days(FEEDBACK_WINDOW_DAYS);
                if Utc::now() > expiry {
                    return Err(FeedbackError::LinkExpired);
                }
                Ok(())
            }
            Err(FeedbackStoreError::NotFound) => Ok(()),
            Err(e) => Err(FeedbackError::Database(e.to_string())),
        }
    }
}

#[async_trait::async_trait]
impl<F: FeedbackStorePort + Clone, W: WeatherPort + Clone> FeedbackPort for FeedbackService<F, W> {
    async fn save_prediction(&self, decision: &CoatDecision) -> Result<Uuid, FeedbackError> {
        let locations: Vec<PredictionLocation> = decision
            .by_location
            .iter()
            .map(|r| PredictionLocation {
                label: r.location.label.clone(),
                lat: r.location.latitude,
                lon: r.location.longitude,
                recommendation: r.recommendation.as_str().to_string(),
                reasons: r.reasons.clone(),
                temp_max_celsius: r.temp_max_celsius,
                temp_min_celsius: r.temp_min_celsius,
                feels_like_min_celsius: r.feels_like_min_celsius,
                precipitation_mm: r.precipitation_mm,
                wind_speed_max_kmh: r.wind_speed_max_kmh,
            })
            .collect();

        self.store
            .save_prediction(
                decision.overall.as_str(),
                &decision.overall_reason,
                &locations,
            )
            .await
            .map_err(|e| FeedbackError::Database(e.to_string()))
    }

    async fn register_contact(
        &self,
        prediction_id: Uuid,
        contact: &str,
    ) -> Result<(), FeedbackError> {
        if !EmailAddress::is_valid(contact) {
            return Err(FeedbackError::InvalidContact);
        }

        self.store
            .get_prediction(prediction_id)
            .await
            .map_err(|e| match e {
                FeedbackStoreError::NotFound => FeedbackError::PredictionNotFound,
                other => FeedbackError::Database(other.to_string()),
            })?;

        let send_after = Utc::now() + Duration::minutes(self.feedback_delay_minutes);

        match self
            .store
            .enqueue_notification(prediction_id, contact, send_after)
            .await
        {
            Ok(()) | Err(FeedbackStoreError::AlreadyExists) => Ok(()),
            Err(other) => Err(FeedbackError::Database(other.to_string())),
        }
    }

    async fn get_prediction(&self, id: Uuid) -> Result<Prediction, FeedbackError> {
        let prediction = self.store.get_prediction(id).await.map_err(|e| match e {
            FeedbackStoreError::NotFound => FeedbackError::PredictionNotFound,
            other => FeedbackError::Database(other.to_string()),
        })?;

        self.check_feedback_window(id).await?;

        Ok(prediction)
    }

    async fn submit_feedback(
        &self,
        prediction_id: Uuid,
        accurate: bool,
        comment: Option<String>,
    ) -> Result<(), FeedbackError> {
        self.check_feedback_window(prediction_id).await?;

        let prediction = self
            .store
            .get_prediction(prediction_id)
            .await
            .map_err(|e| match e {
                FeedbackStoreError::NotFound => FeedbackError::PredictionNotFound,
                other => FeedbackError::Database(other.to_string()),
            })?;

        let prediction_date = prediction.created_at.date_naive();

        let actual_weather = match fetch_actuals(
            &self.weather,
            &prediction.locations,
            prediction_date,
        )
        .await
        {
            Ok(actuals) => Some(actuals),
            Err(e) => {
                warn!(error = %e, "failed to fetch actual weather, recording feedback without it");
                None
            }
        };

        let feedback = Feedback { accurate, comment };

        self.store
            .record_feedback(&prediction, &feedback, actual_weather.as_deref())
            .await
            .map_err(|e| match e {
                FeedbackStoreError::AlreadyExists => FeedbackError::AlreadyRated,
                other => FeedbackError::Database(other.to_string()),
            })
    }
}

async fn fetch_actuals<W: WeatherPort>(
    weather: &W,
    locations: &[PredictionLocation],
    date: chrono::NaiveDate,
) -> Result<Vec<PredictionLocation>, String> {
    let futures = locations.iter().map(|loc| {
        let location =
            Location::new(loc.lat, loc.lon, loc.label.clone()).map_err(|e| e.to_string());
        async move {
            let location = location?;
            let obs = weather
                .fetch_daily_observation(&location, date)
                .await
                .map_err(|e| e.to_string())?;
            Ok(PredictionLocation {
                label: loc.label.clone(),
                lat: loc.lat,
                lon: loc.lon,
                recommendation: String::new(),
                reasons: vec![],
                temp_max_celsius: obs.temp_max_celsius,
                temp_min_celsius: obs.temp_min_celsius,
                feels_like_min_celsius: obs.feels_like_min_celsius,
                precipitation_mm: obs.precipitation_mm,
                wind_speed_max_kmh: obs.wind_speed_max_kmh,
            })
        }
    });

    try_join_all(futures).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            location::Location,
            recommendation::{CoatRecommendation, LocationRecommendation},
            weather::DailyForecast,
        },
        ports::outbound::{
            FeedbackStoreError, FeedbackStorePort, QueuedNotification, WeatherPort,
            WeatherPortError,
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

        async fn get_notification_sent_at(
            &self,
            _prediction_id: Uuid,
        ) -> Result<DateTime<Utc>, FeedbackStoreError> {
            Err(FeedbackStoreError::NotFound)
        }

        async fn record_feedback(
            &self,
            _prediction: &Prediction,
            _feedback: &Feedback,
            _actual_weather: Option<&[PredictionLocation]>,
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
                recommendation: CoatRecommendation::Coat,
                reasons: vec!["feels like as low as 5.0\u{00b0}C".into()],
                temp_max_celsius: 8.0,
                temp_min_celsius: 3.0,
                feels_like_min_celsius: 5.0,
                precipitation_mm: 0.0,
                wind_speed_max_kmh: 10.0,
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
        assert!(svc.submit_feedback(id, true, None).await.is_ok());
    }

    #[tokio::test]
    async fn submit_feedback_prediction_not_found() {
        let svc = service();
        let err = svc
            .submit_feedback(Uuid::new_v4(), true, None)
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
    struct ExpiredWindowStore {
        inner: FakeStore,
    }

    #[async_trait::async_trait]
    impl FeedbackStorePort for ExpiredWindowStore {
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

        async fn get_notification_sent_at(
            &self,
            _prediction_id: Uuid,
        ) -> Result<DateTime<Utc>, FeedbackStoreError> {
            Ok(Utc::now() - Duration::days(8))
        }

        async fn record_feedback(
            &self,
            _prediction: &Prediction,
            _feedback: &Feedback,
            _actual_weather: Option<&[PredictionLocation]>,
        ) -> Result<(), FeedbackStoreError> {
            Ok(())
        }
    }

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

        async fn get_notification_sent_at(
            &self,
            _prediction_id: Uuid,
        ) -> Result<DateTime<Utc>, FeedbackStoreError> {
            Err(FeedbackStoreError::NotFound)
        }

        async fn record_feedback(
            &self,
            _prediction: &Prediction,
            _feedback: &Feedback,
            _actual_weather: Option<&[PredictionLocation]>,
        ) -> Result<(), FeedbackStoreError> {
            Err(FeedbackStoreError::AlreadyExists)
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
    async fn feedback_window_expired_blocks_get_prediction() {
        let store = ExpiredWindowStore {
            inner: FakeStore::new(),
        };
        let svc = FeedbackService::new(store.clone(), FakeWeather, 480);
        let id = svc.save_prediction(&coat_decision()).await.unwrap();
        let err = svc.get_prediction(id).await.unwrap_err();
        assert!(matches!(err, FeedbackError::LinkExpired));
    }

    #[tokio::test]
    async fn feedback_window_expired_blocks_submit() {
        let store = ExpiredWindowStore {
            inner: FakeStore::new(),
        };
        let svc = FeedbackService::new(store.clone(), FakeWeather, 480);
        let id = svc.save_prediction(&coat_decision()).await.unwrap();
        let err = svc.submit_feedback(id, true, None).await.unwrap_err();
        assert!(matches!(err, FeedbackError::LinkExpired));
    }

    #[tokio::test]
    async fn submit_feedback_already_rated() {
        let store = AlreadyExistsStore {
            inner: FakeStore::new(),
        };
        let svc = FeedbackService::new(store.clone(), FakeWeather, 480);
        let id = svc.save_prediction(&coat_decision()).await.unwrap();
        let err = svc.submit_feedback(id, true, None).await.unwrap_err();
        assert!(matches!(err, FeedbackError::AlreadyRated));
    }

    #[tokio::test]
    async fn submit_feedback_records_without_actual_weather_on_failure() {
        let store = FakeStore::new();
        let svc = FeedbackService::new(store.clone(), FailingWeather, 480);
        let id = svc.save_prediction(&coat_decision()).await.unwrap();
        assert!(svc.submit_feedback(id, true, None).await.is_ok());
    }
}
