use futures::future::try_join_all;

use crate::{
    domain::{
        error::DomainError,
        location::Location,
        recommendation::{self, CoatDecision, CoatRecommendation},
    },
    ports::{
        inbound::{CoatCheckError, CoatCheckPort},
        outbound::WeatherPort,
    },
};

#[derive(Clone)]
pub struct CoatCheckService<W: WeatherPort + Clone> {
    weather: W,
}

impl<W: WeatherPort + Clone> CoatCheckService<W> {
    pub fn new(weather: W) -> Self {
        Self { weather }
    }
}

#[async_trait::async_trait]
impl<W: WeatherPort + Clone> CoatCheckPort for CoatCheckService<W> {
    async fn check(&self, locations: Vec<Location>) -> Result<CoatDecision, CoatCheckError> {
        if locations.is_empty() {
            return Err(CoatCheckError::Domain(DomainError::NoLocations));
        }

        let futures: Vec<_> = locations
            .iter()
            .map(|loc| self.weather.fetch_daily_forecast(loc))
            .collect();

        let forecasts = try_join_all(futures)
            .await
            .map_err(|e| CoatCheckError::WeatherUnavailable(e.to_string()))?;

        let by_location: Vec<_> = forecasts.iter().map(recommendation::evaluate).collect();

        // Worst-case: Coat > RainJacket > No (Ord is derived on the enum)
        let overall = by_location
            .iter()
            .map(|r| r.recommendation.clone())
            .max()
            .unwrap_or(CoatRecommendation::No);

        let overall_reason = build_overall_reason(&by_location, &overall);

        Ok(CoatDecision {
            overall,
            by_location,
            overall_reason,
        })
    }
}

fn build_overall_reason(
    by_location: &[crate::domain::recommendation::LocationRecommendation],
    overall: &CoatRecommendation,
) -> String {
    if *overall == CoatRecommendation::No {
        return "No coat or jacket needed at any of your locations today.".to_string();
    }

    let triggered: Vec<String> = by_location
        .iter()
        .filter(|r| r.recommendation != CoatRecommendation::No)
        .map(|r| {
            let name = r
                .location
                .label
                .as_deref()
                .unwrap_or("unnamed location")
                .to_string();
            let first_reason = r.reasons.first().cloned().unwrap_or_default();
            format!("{name}: {first_reason}")
        })
        .collect();

    triggered.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{location::Location, weather::DailyForecast},
        ports::outbound::WeatherPortError,
    };
    use std::sync::Arc;

    #[derive(Clone)]
    struct FakeWeather {
        forecasts: Arc<Vec<DailyForecast>>,
        call_count: Arc<std::sync::Mutex<usize>>,
    }

    impl FakeWeather {
        fn new(forecasts: Vec<DailyForecast>) -> Self {
            Self {
                forecasts: Arc::new(forecasts),
                call_count: Arc::new(std::sync::Mutex::new(0)),
            }
        }
    }

    #[async_trait::async_trait]
    impl WeatherPort for FakeWeather {
        async fn fetch_daily_forecast(
            &self,
            _location: &Location,
        ) -> Result<DailyForecast, WeatherPortError> {
            let mut count = self.call_count.lock().unwrap();
            let idx = *count;
            *count += 1;
            self.forecasts
                .get(idx)
                .cloned()
                .ok_or_else(|| WeatherPortError::Network("no more forecasts".into()))
        }

        async fn fetch_daily_observation(
            &self,
            _location: &Location,
            _date: chrono::NaiveDate,
        ) -> Result<DailyForecast, WeatherPortError> {
            self.forecasts
                .first()
                .cloned()
                .ok_or_else(|| WeatherPortError::Network("no forecasts".into()))
        }
    }

    fn london() -> Location {
        Location::new(51.5, -0.1, Some("London".into())).unwrap()
    }

    fn warm_dry(loc: Location) -> DailyForecast {
        DailyForecast {
            location: loc,
            temp_max_celsius: 22.0,
            temp_min_celsius: 15.0,
            feels_like_min_celsius: 14.0,
            precipitation_mm: 0.0,
            wind_speed_max_kmh: 10.0,
            snowfall_cm: 0.0,
            weather_code: 0,
        }
    }

    fn cold_dry(loc: Location) -> DailyForecast {
        DailyForecast {
            location: loc,
            temp_max_celsius: 5.0,
            temp_min_celsius: 1.0,
            feels_like_min_celsius: -1.0,
            precipitation_mm: 0.0,
            wind_speed_max_kmh: 10.0,
            snowfall_cm: 0.0,
            weather_code: 0,
        }
    }

    fn warm_rainy(loc: Location) -> DailyForecast {
        DailyForecast {
            location: loc,
            temp_max_celsius: 22.0,
            temp_min_celsius: 15.0,
            feels_like_min_celsius: 14.0,
            precipitation_mm: 8.0,
            wind_speed_max_kmh: 10.0,
            snowfall_cm: 0.0,
            weather_code: 0,
        }
    }

    #[tokio::test]
    async fn no_coat_when_warm_and_dry() {
        let svc = CoatCheckService::new(FakeWeather::new(vec![warm_dry(london())]));
        let result = svc.check(vec![london()]).await.unwrap();
        assert_eq!(result.overall, CoatRecommendation::No);
    }

    #[tokio::test]
    async fn coat_when_cold() {
        let svc = CoatCheckService::new(FakeWeather::new(vec![cold_dry(london())]));
        let result = svc.check(vec![london()]).await.unwrap();
        assert_eq!(result.overall, CoatRecommendation::Coat);
    }

    #[tokio::test]
    async fn rain_jacket_when_warm_and_rainy() {
        let svc = CoatCheckService::new(FakeWeather::new(vec![warm_rainy(london())]));
        let result = svc.check(vec![london()]).await.unwrap();
        assert_eq!(result.overall, CoatRecommendation::RainJacket);
    }

    #[tokio::test]
    async fn worst_case_coat_beats_rain_jacket() {
        let bham = Location::new(52.4, -1.9, Some("Birmingham".into())).unwrap();
        let svc = CoatCheckService::new(FakeWeather::new(vec![
            warm_rainy(london()),
            cold_dry(bham.clone()),
        ]));
        let result = svc.check(vec![london(), bham]).await.unwrap();
        assert_eq!(result.overall, CoatRecommendation::Coat);
    }

    #[tokio::test]
    async fn worst_case_across_locations() {
        let bham = Location::new(52.4, -1.9, Some("Birmingham".into())).unwrap();
        let svc = CoatCheckService::new(FakeWeather::new(vec![
            warm_dry(london()),
            cold_dry(bham.clone()),
        ]));
        let result = svc.check(vec![london(), bham]).await.unwrap();
        assert_eq!(result.overall, CoatRecommendation::Coat);
        assert_eq!(result.by_location[0].recommendation, CoatRecommendation::No);
        assert_eq!(
            result.by_location[1].recommendation,
            CoatRecommendation::Coat
        );
    }

    #[tokio::test]
    async fn empty_locations_returns_error() {
        let svc = CoatCheckService::new(FakeWeather::new(vec![]));
        let result = svc.check(vec![]).await;
        assert!(matches!(
            result,
            Err(CoatCheckError::Domain(DomainError::NoLocations))
        ));
    }
}
