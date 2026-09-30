use serde::Deserialize;
use tracing::{info, warn};

use crate::{
    domain::{location::Location, weather::DailyForecast},
    ports::outbound::{WeatherPort, WeatherPortError},
};

#[derive(Clone)]
pub struct OpenMeteoClient {
    http: reqwest::Client,
    base_url: String,
}

impl OpenMeteoClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build HTTP client"),
            base_url: "https://api.open-meteo.com".to_string(),
        }
    }

    #[cfg(test)]
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
        }
    }
}

impl Default for OpenMeteoClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl WeatherPort for OpenMeteoClient {
    async fn fetch_daily_forecast(
        &self,
        location: &Location,
    ) -> Result<DailyForecast, WeatherPortError> {
        let url = format!(
            "{}/v1/forecast\
             ?latitude={}&longitude={}\
             &daily=temperature_2m_max,temperature_2m_min,\
             apparent_temperature_min,\
             precipitation_sum,wind_speed_10m_max,snowfall_sum,weather_code\
             &forecast_days=1\
             &timezone=auto",
            self.base_url, location.latitude, location.longitude
        );

        info!(
            latitude = location.latitude,
            longitude = location.longitude,
            "fetching forecast from Open-Meteo"
        );

        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| WeatherPortError::Network(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!(status = status.as_u16(), body = %body, "Open-Meteo returned error");
            return Err(WeatherPortError::Upstream {
                status: status.as_u16(),
                body,
            });
        }

        let parsed: OpenMeteoResponse = response
            .json()
            .await
            .map_err(|e| WeatherPortError::Parse(e.to_string()))?;

        info!(
            latitude = location.latitude,
            longitude = location.longitude,
            "forecast received"
        );

        to_domain_forecast(location, parsed)
    }
}

// ── Serde DTOs (private to this module) ──────────────────────────────────────

#[derive(Debug, Deserialize)]
struct OpenMeteoResponse {
    daily: OpenMeteoDailyData,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoDailyData {
    temperature_2m_max: Vec<f64>,
    temperature_2m_min: Vec<f64>,
    apparent_temperature_min: Vec<f64>,
    precipitation_sum: Vec<f64>,
    wind_speed_10m_max: Vec<f64>,
    snowfall_sum: Vec<f64>,
    weather_code: Vec<u16>,
}

fn first_f64(v: &[f64], name: &str) -> Result<f64, WeatherPortError> {
    v.first()
        .copied()
        .ok_or_else(|| WeatherPortError::Parse(format!("missing field '{name}' in response")))
}

fn to_domain_forecast(
    location: &Location,
    resp: OpenMeteoResponse,
) -> Result<DailyForecast, WeatherPortError> {
    let d = &resp.daily;
    Ok(DailyForecast {
        location: location.clone(),
        temp_max_celsius: first_f64(&d.temperature_2m_max, "temperature_2m_max")?,
        temp_min_celsius: first_f64(&d.temperature_2m_min, "temperature_2m_min")?,
        feels_like_min_celsius: first_f64(&d.apparent_temperature_min, "apparent_temperature_min")?,
        precipitation_mm: first_f64(&d.precipitation_sum, "precipitation_sum")?,
        wind_speed_max_kmh: first_f64(&d.wind_speed_10m_max, "wind_speed_10m_max")?,
        snowfall_cm: d.snowfall_sum.first().copied().unwrap_or(0.0),
        weather_code: d.weather_code.first().copied().unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn london() -> Location {
        Location::new(51.5074, -0.1278, Some("London".into())).unwrap()
    }

    fn mock_response_body() -> &'static str {
        r#"{
            "latitude": 51.5,
            "longitude": -0.1278,
            "timezone": "Europe/London",
            "daily": {
                "time": ["2024-01-15"],
                "temperature_2m_max": [13.2],
                "temperature_2m_min": [7.1],
                "apparent_temperature_max": [11.0],
                "apparent_temperature_min": [8.3],
                "precipitation_sum": [12.1],
                "wind_speed_10m_max": [28.0],
                "snowfall_sum": [0.0],
                "weather_code": [61]
            }
        }"#
    }

    #[test]
    fn parse_response_to_domain() {
        let parsed: OpenMeteoResponse = serde_json::from_str(mock_response_body()).unwrap();
        let loc = london();
        let forecast = to_domain_forecast(&loc, parsed).unwrap();

        assert_eq!(forecast.temp_max_celsius, 13.2);
        assert_eq!(forecast.feels_like_min_celsius, 8.3);
        assert_eq!(forecast.precipitation_mm, 12.1);
        assert_eq!(forecast.weather_code, 61);
    }

    #[test]
    fn missing_field_returns_parse_error() {
        let bad = r#"{"daily": {"temperature_2m_max": [], "temperature_2m_min": [7.1],
            "apparent_temperature_max": [11.0], "apparent_temperature_min": [8.3],
            "precipitation_sum": [0.0], "wind_speed_10m_max": [10.0],
            "snowfall_sum": [0.0], "weather_code": [0]}}"#;
        let parsed: OpenMeteoResponse = serde_json::from_str(bad).unwrap();
        let result = to_domain_forecast(&london(), parsed);
        assert!(matches!(result, Err(WeatherPortError::Parse(_))));
    }
}
