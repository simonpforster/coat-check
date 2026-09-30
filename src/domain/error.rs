use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("latitude {0} is outside the valid range -90..90")]
    InvalidLatitude(f64),
    #[error("longitude {0} is outside the valid range -180..180")]
    InvalidLongitude(f64),
    #[error("at least one location must be provided")]
    NoLocations,
}
