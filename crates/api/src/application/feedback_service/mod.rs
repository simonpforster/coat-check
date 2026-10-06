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
                timezone: r.timezone.clone(),
                recommendation: r.recommendation.as_str().to_string(),
                reasons: r.reasons.clone(),
                temp_max_celsius: r.temp_max_celsius,
                temp_min_celsius: r.temp_min_celsius,
                feels_like_min_celsius: r.feels_like_min_celsius,
                precipitation_mm: r.precipitation_mm,
                wind_speed_max_kmh: r.wind_speed_max_kmh,
                snowfall_cm: r.snowfall_cm,
                weather_code: r.weather_code,
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
        self.store.get_prediction(id).await.map_err(|e| match e {
            FeedbackStoreError::NotFound => FeedbackError::PredictionNotFound,
            other => FeedbackError::Database(other.to_string()),
        })
    }

    async fn submit_feedback(
        &self,
        prediction_id: Uuid,
        brought: &str,
        should_have_brought: &str,
        comment: Option<String>,
    ) -> Result<(), FeedbackError> {
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

        let feedback = Feedback {
            brought: brought.to_string(),
            should_have_brought: should_have_brought.to_string(),
            comment,
        };

        self.store
            .record_feedback(&prediction, &feedback, actual_weather.as_deref())
            .await
            .map_err(|e| match e {
                FeedbackStoreError::AlreadyExists => FeedbackError::AlreadyRated,
                other => FeedbackError::Database(other.to_string()),
            })
    }

    async fn unsubscribe(&self, contact: &str) -> Result<(), FeedbackError> {
        if let Err(e) = self
            .store
            .cancel_pending_notifications_for_contact(contact)
            .await
        {
            tracing::warn!(error = %e, "unsubscribe store error (suppressed)");
        }
        Ok(())
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
                timezone: obs.timezone,
                recommendation: String::new(),
                reasons: vec![],
                temp_max_celsius: obs.temp_max_celsius,
                temp_min_celsius: obs.temp_min_celsius,
                feels_like_min_celsius: obs.feels_like_min_celsius,
                precipitation_mm: obs.precipitation_mm,
                wind_speed_max_kmh: obs.wind_speed_max_kmh,
                snowfall_cm: obs.snowfall_cm,
                weather_code: obs.weather_code,
            })
        }
    });

    try_join_all(futures).await
}

#[cfg(test)]
mod tests;
