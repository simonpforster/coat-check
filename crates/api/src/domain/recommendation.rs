use crate::domain::{location::Location, weather::DailyForecast};

pub use coat_check_common::Recommendation as CoatRecommendation;

const FEELS_LIKE_THRESHOLD: f64 = 12.0;
const TEMP_MAX_THRESHOLD: f64 = 15.0;
const LIGHT_PRECIP_THRESHOLD: f64 = 1.0;
const HEAVY_PRECIP_THRESHOLD: f64 = 5.0;
const WIND_THRESHOLD: f64 = 40.0;
const THUNDERSTORM_CODE: u16 = 95;

#[derive(Debug, Clone)]
pub struct LocationRecommendation {
    pub location: Location,
    pub timezone: String,
    pub recommendation: CoatRecommendation,
    pub reasons: Vec<String>,
    pub temp_max_celsius: f64,
    pub temp_min_celsius: f64,
    pub feels_like_min_celsius: f64,
    pub precipitation_mm: f64,
    pub wind_speed_max_kmh: f64,
    pub snowfall_cm: f64,
    pub weather_code: u16,
}

#[derive(Debug, Clone)]
pub struct CoatDecision {
    pub overall: CoatRecommendation,
    pub by_location: Vec<LocationRecommendation>,
    pub overall_reason: String,
}

/// Pure domain function — no I/O, no side effects.
/// Evaluates a single location's daily forecast and returns a recommendation.
///
/// Cold-weather triggers (coat): feels-like, temp max, wind, snowfall.
/// Wet-weather triggers (rain jacket): precipitation, thunderstorm.
/// If any cold trigger fires → Coat. If only wet triggers → RainJacket.
pub fn evaluate(forecast: &DailyForecast) -> LocationRecommendation {
    let mut reasons: Vec<String> = Vec::new();
    let mut needs_coat = false;

    // Cold-weather triggers → full coat
    if forecast.feels_like_min_celsius < FEELS_LIKE_THRESHOLD {
        needs_coat = true;
        reasons.push(format!(
            "feels like as low as {:.1}°C (threshold {}°C)",
            forecast.feels_like_min_celsius, FEELS_LIKE_THRESHOLD
        ));
    }

    if forecast.temp_max_celsius < TEMP_MAX_THRESHOLD {
        needs_coat = true;
        reasons.push(format!(
            "high of only {:.1}°C today",
            forecast.temp_max_celsius
        ));
    }

    if forecast.snowfall_cm > 0.0 {
        needs_coat = true;
        reasons.push(format!("{:.1} cm of snow", forecast.snowfall_cm));
    }

    if forecast.wind_speed_max_kmh >= WIND_THRESHOLD {
        needs_coat = true;
        reasons.push(format!(
            "wind up to {:.0} km/h — a coat will cut the chill",
            forecast.wind_speed_max_kmh
        ));
    }

    // Wet-weather triggers
    let mut needs_rain_jacket = false;
    let mut needs_umbrella = false;

    if forecast.precipitation_mm >= HEAVY_PRECIP_THRESHOLD {
        needs_rain_jacket = true;
        reasons.push(format!(
            "{:.1} mm precipitation expected",
            forecast.precipitation_mm
        ));
    } else if forecast.precipitation_mm >= LIGHT_PRECIP_THRESHOLD {
        needs_umbrella = true;
        reasons.push(format!(
            "{:.1} mm precipitation expected",
            forecast.precipitation_mm
        ));
    }

    if forecast.weather_code >= THUNDERSTORM_CODE {
        needs_rain_jacket = true;
        reasons.push("thunderstorms forecast".to_string());
    }

    let recommendation = if needs_coat {
        CoatRecommendation::Coat
    } else if needs_rain_jacket {
        CoatRecommendation::RainJacket
    } else if needs_umbrella {
        CoatRecommendation::Umbrella
    } else {
        CoatRecommendation::No
    };

    LocationRecommendation {
        location: forecast.location.clone(),
        timezone: forecast.timezone.clone(),
        recommendation,
        reasons,
        temp_max_celsius: forecast.temp_max_celsius,
        temp_min_celsius: forecast.temp_min_celsius,
        feels_like_min_celsius: forecast.feels_like_min_celsius,
        precipitation_mm: forecast.precipitation_mm,
        wind_speed_max_kmh: forecast.wind_speed_max_kmh,
        snowfall_cm: forecast.snowfall_cm,
        weather_code: forecast.weather_code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::location::Location;

    fn loc() -> Location {
        Location::new(51.5, -0.1, None).unwrap()
    }

    fn warm_dry() -> DailyForecast {
        DailyForecast {
            location: loc(),
            timezone: "Europe/London".into(),
            temp_max_celsius: 22.0,
            temp_min_celsius: 15.0,
            feels_like_min_celsius: 14.0,
            precipitation_mm: 0.0,
            wind_speed_max_kmh: 10.0,
            snowfall_cm: 0.0,
            weather_code: 0,
        }
    }

    #[test]
    fn no_coat_warm_dry() {
        let r = evaluate(&warm_dry());
        assert_eq!(r.recommendation, CoatRecommendation::No);
        assert!(r.reasons.is_empty());
    }

    #[test]
    fn coat_cold_feels_like() {
        let mut f = warm_dry();
        f.feels_like_min_celsius = 8.0;
        let r = evaluate(&f);
        assert_eq!(r.recommendation, CoatRecommendation::Coat);
        assert!(r.reasons.iter().any(|s| s.contains("feels like")));
    }

    #[test]
    fn coat_low_max_temp() {
        let mut f = warm_dry();
        f.temp_max_celsius = 12.0;
        let r = evaluate(&f);
        assert_eq!(r.recommendation, CoatRecommendation::Coat);
        assert!(r.reasons.iter().any(|s| s.contains("high of only")));
    }

    #[test]
    fn umbrella_light_rain() {
        let mut f = warm_dry();
        f.precipitation_mm = 3.0;
        let r = evaluate(&f);
        assert_eq!(r.recommendation, CoatRecommendation::Umbrella);
        assert!(r.reasons.iter().any(|s| s.contains("precipitation")));
    }

    #[test]
    fn rain_jacket_heavy_rain() {
        let mut f = warm_dry();
        f.precipitation_mm = 8.0;
        let r = evaluate(&f);
        assert_eq!(r.recommendation, CoatRecommendation::RainJacket);
        assert!(r.reasons.iter().any(|s| s.contains("precipitation")));
    }

    #[test]
    fn rain_jacket_at_heavy_threshold() {
        let mut f = warm_dry();
        f.precipitation_mm = 5.0;
        let r = evaluate(&f);
        assert_eq!(r.recommendation, CoatRecommendation::RainJacket);
    }

    #[test]
    fn umbrella_just_below_heavy_threshold() {
        let mut f = warm_dry();
        f.precipitation_mm = 4.9;
        let r = evaluate(&f);
        assert_eq!(r.recommendation, CoatRecommendation::Umbrella);
    }

    #[test]
    fn coat_cold_and_rainy() {
        let mut f = warm_dry();
        f.feels_like_min_celsius = 8.0;
        f.precipitation_mm = 8.0;
        let r = evaluate(&f);
        assert_eq!(r.recommendation, CoatRecommendation::Coat);
        assert_eq!(r.reasons.len(), 2);
    }

    #[test]
    fn no_coat_trace_precipitation() {
        let mut f = warm_dry();
        f.precipitation_mm = 0.5;
        let r = evaluate(&f);
        assert_eq!(r.recommendation, CoatRecommendation::No);
    }

    #[test]
    fn coat_snowfall() {
        let mut f = warm_dry();
        f.snowfall_cm = 2.0;
        let r = evaluate(&f);
        assert_eq!(r.recommendation, CoatRecommendation::Coat);
        assert!(r.reasons.iter().any(|s| s.contains("snow")));
    }

    #[test]
    fn coat_high_wind() {
        let mut f = warm_dry();
        f.wind_speed_max_kmh = 50.0;
        let r = evaluate(&f);
        assert_eq!(r.recommendation, CoatRecommendation::Coat);
        assert!(r.reasons.iter().any(|s| s.contains("wind")));
    }

    #[test]
    fn rain_jacket_warm_thunderstorm() {
        let mut f = warm_dry();
        f.weather_code = 95;
        let r = evaluate(&f);
        assert_eq!(r.recommendation, CoatRecommendation::RainJacket);
        assert!(r.reasons.iter().any(|s| s.contains("thunderstorm")));
    }
}
