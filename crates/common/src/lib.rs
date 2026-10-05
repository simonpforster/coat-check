/// The four coat-check recommendation levels.
///
/// Used for predictions, feedback responses, and display labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Recommendation {
    /// No outerwear needed.
    No = 0,
    /// Light rain — bring an umbrella.
    Umbrella = 1,
    /// Heavier rain — bring a rain jacket.
    RainJacket = 2,
    /// Bring a proper coat.
    Coat = 3,
}

impl Recommendation {
    pub const ALL: &[Recommendation] = &[
        Recommendation::No,
        Recommendation::Umbrella,
        Recommendation::RainJacket,
        Recommendation::Coat,
    ];

    pub const fn as_str(&self) -> &'static str {
        match self {
            Recommendation::No => "no",
            Recommendation::Umbrella => "umbrella",
            Recommendation::RainJacket => "rain_jacket",
            Recommendation::Coat => "coat",
        }
    }

    /// Parse a stored string into its display label, falling back to the raw value.
    pub fn from_str_label(s: &str) -> String {
        match s.parse::<Recommendation>() {
            Ok(r) => r.label().to_string(),
            Err(()) => s.to_string(),
        }
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Recommendation::No => "Nothing",
            Recommendation::Umbrella => "Umbrella",
            Recommendation::RainJacket => "Rain jacket",
            Recommendation::Coat => "Coat",
        }
    }
}

impl std::str::FromStr for Recommendation {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "no" => Ok(Recommendation::No),
            "umbrella" => Ok(Recommendation::Umbrella),
            "rain_jacket" => Ok(Recommendation::RainJacket),
            "coat" => Ok(Recommendation::Coat),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for r in Recommendation::ALL {
            assert_eq!(r.as_str().parse::<Recommendation>(), Ok(*r));
        }
    }

    #[test]
    fn ordering() {
        assert!(Recommendation::Coat > Recommendation::RainJacket);
        assert!(Recommendation::RainJacket > Recommendation::Umbrella);
        assert!(Recommendation::Umbrella > Recommendation::No);
    }

    #[test]
    fn invalid_returns_err() {
        assert!("poncho".parse::<Recommendation>().is_err());
    }
}
