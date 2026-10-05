use crate::{
    domain::{
        recommendation::{CheckResult, LocationView},
        suggestion::Suggestion,
    },
    ports::{
        inbound::{LocationInput, WebPort, WebPortError},
        outbound::{
            CoatCheckApiError, CoatCheckApiPort, CoatCheckLocation, FeedbackApiPort, GeocodingPort,
            PredictionResponse,
        },
    },
};

#[derive(Clone)]
pub struct WebService<
    C: CoatCheckApiPort + Clone,
    G: GeocodingPort + Clone,
    F: FeedbackApiPort + Clone,
> {
    coat_check: C,
    geocoding: G,
    feedback: F,
}

impl<C: CoatCheckApiPort + Clone, G: GeocodingPort + Clone, F: FeedbackApiPort + Clone>
    WebService<C, G, F>
{
    pub fn new(coat_check: C, geocoding: G, feedback: F) -> Self {
        Self {
            coat_check,
            geocoding,
            feedback,
        }
    }
}

#[async_trait::async_trait]
impl<C: CoatCheckApiPort + Clone, G: GeocodingPort + Clone, F: FeedbackApiPort + Clone> WebPort
    for WebService<C, G, F>
{
    async fn check_coat(&self, locations: Vec<LocationInput>) -> Result<CheckResult, WebPortError> {
        if locations.is_empty() {
            return Err(WebPortError::NoLocations);
        }

        let api_locations = locations
            .into_iter()
            .map(|l| {
                let lat: f64 = l.lat.parse().map_err(|_| WebPortError::InvalidLatitude)?;
                let lon: f64 = l.lon.parse().map_err(|_| WebPortError::InvalidLongitude)?;
                let label = l.label.filter(|s| !s.is_empty());
                Ok(CoatCheckLocation { lat, lon, label })
            })
            .collect::<Result<Vec<_>, WebPortError>>()?;

        let resp = self
            .coat_check
            .check(api_locations)
            .await
            .map_err(|e| match e {
                CoatCheckApiError::Network(msg) => WebPortError::ServiceUnavailable(msg),
                CoatCheckApiError::Upstream { error, detail } => {
                    WebPortError::UpstreamError { error, detail }
                }
            })?;

        Ok(CheckResult {
            prediction_id: resp.prediction_id,
            recommendation: resp.recommendation.clone(),
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
        })
    }

    async fn search_locations(&self, query: &str) -> Result<Vec<Suggestion>, WebPortError> {
        let q = query.trim();
        if q.len() < 2 {
            return Ok(vec![]);
        }

        let results = self
            .geocoding
            .geocode(q)
            .await
            .map_err(|e| WebPortError::GeocodingFailed(e.to_string()))?;

        Ok(results
            .into_iter()
            .take(5)
            .map(|r| Suggestion {
                lat: format!("{:.6}", r.latitude),
                lon: format!("{:.6}", r.longitude),
                name: r.display_name(),
            })
            .collect())
    }

    async fn register_email(&self, prediction_id: &str, email: &str) -> Result<(), WebPortError> {
        self.feedback
            .register_email(prediction_id, email)
            .await
            .map_err(|e| WebPortError::FeedbackError(e.to_string()))
    }

    async fn get_prediction(
        &self,
        prediction_id: &str,
    ) -> Result<PredictionResponse, WebPortError> {
        self.feedback
            .get_prediction(prediction_id)
            .await
            .map_err(|e| WebPortError::FeedbackError(e.to_string()))
    }

    async fn submit_feedback(
        &self,
        prediction_id: &str,
        brought: &str,
        should_have_brought: &str,
        comment: Option<&str>,
    ) -> Result<(), WebPortError> {
        self.feedback
            .submit_feedback(prediction_id, brought, should_have_brought, comment)
            .await
            .map_err(|e| WebPortError::FeedbackError(e.to_string()))
    }

    async fn unsubscribe(&self, contact: &str) -> Result<(), WebPortError> {
        self.feedback
            .unsubscribe(contact)
            .await
            .map_err(|e| WebPortError::FeedbackError(e.to_string()))
    }
}

#[cfg(test)]
mod tests;
