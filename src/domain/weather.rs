use crate::domain::location::Location;

#[derive(Debug, Clone)]
pub struct DailyForecast {
    pub location: Location,
    pub temp_max_celsius: f64,
    pub temp_min_celsius: f64,
    pub feels_like_min_celsius: f64,
    pub precipitation_mm: f64,
    pub wind_speed_max_kmh: f64,
    pub snowfall_cm: f64,
    /// WMO weather interpretation code
    pub weather_code: u16,
}
