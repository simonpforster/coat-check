use crate::domain::error::DomainError;

#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    pub label: Option<String>,
}

impl Location {
    pub fn new(latitude: f64, longitude: f64, label: Option<String>) -> Result<Self, DomainError> {
        if !(-90.0..=90.0).contains(&latitude) {
            return Err(DomainError::InvalidLatitude(latitude));
        }
        if !(-180.0..=180.0).contains(&longitude) {
            return Err(DomainError::InvalidLongitude(longitude));
        }
        Ok(Self {
            latitude,
            longitude,
            label,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_location() {
        let loc = Location::new(51.5074, -0.1278, Some("London".into()));
        assert!(loc.is_ok());
    }

    #[test]
    fn invalid_latitude() {
        assert!(matches!(
            Location::new(91.0, 0.0, None),
            Err(DomainError::InvalidLatitude(_))
        ));
    }

    #[test]
    fn invalid_longitude() {
        assert!(matches!(
            Location::new(0.0, 181.0, None),
            Err(DomainError::InvalidLongitude(_))
        ));
    }
}
