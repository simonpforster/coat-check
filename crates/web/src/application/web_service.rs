use crate::{
    domain::{
        recommendation::{CheckResult, LocationView},
        suggestion::Suggestion,
    },
    ports::{
        inbound::{LocationInput, WebPort, WebPortError},
        outbound::{CoatCheckApiError, CoatCheckApiPort, CoatCheckLocation, GeocodingPort},
    },
};

#[derive(Clone)]
pub struct WebService<C: CoatCheckApiPort + Clone, G: GeocodingPort + Clone> {
    coat_check: C,
    geocoding: G,
}

impl<C: CoatCheckApiPort + Clone, G: GeocodingPort + Clone> WebService<C, G> {
    pub fn new(coat_check: C, geocoding: G) -> Self {
        Self {
            coat_check,
            geocoding,
        }
    }
}

#[async_trait::async_trait]
impl<C: CoatCheckApiPort + Clone, G: GeocodingPort + Clone> WebPort for WebService<C, G> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::outbound::{
        CoatCheckApiError, CoatCheckApiPort, CoatCheckLocation, CoatCheckLocationResult,
        CoatCheckResult, GeocodingError, GeocodingPort, GeocodingResult,
    };

    #[derive(Clone)]
    struct FakeCoatCheckApi {
        result: Result<CoatCheckResult, CoatCheckApiError>,
    }

    impl FakeCoatCheckApi {
        fn coat() -> Self {
            Self {
                result: Ok(CoatCheckResult {
                    recommendation: "coat".into(),
                    reason: "London: feels like as low as 5.0\u{00b0}C".into(),
                    locations: vec![CoatCheckLocationResult {
                        label: Some("London".into()),
                        lat: 51.5,
                        lon: -0.1,
                        recommendation: "coat".into(),
                        reasons: vec![
                            "feels like as low as 5.0\u{00b0}C (threshold 12\u{00b0}C)".into()
                        ],
                        temp_max_celsius: 8.0,
                        temp_min_celsius: 3.0,
                        feels_like_min_celsius: 5.0,
                        precipitation_mm: 0.0,
                        wind_speed_max_kmh: 5.0,
                    }],
                }),
            }
        }

        fn no_coat() -> Self {
            Self {
                result: Ok(CoatCheckResult {
                    recommendation: "no".into(),
                    reason: "No coat or jacket needed at any of your locations today.".into(),
                    locations: vec![CoatCheckLocationResult {
                        label: Some("London".into()),
                        lat: 51.5,
                        lon: -0.1,
                        recommendation: "no".into(),
                        reasons: vec![],
                        temp_max_celsius: 20.0,
                        temp_min_celsius: 15.0,
                        feels_like_min_celsius: 14.5,
                        precipitation_mm: 0.0,
                        wind_speed_max_kmh: 5.0,
                    }],
                }),
            }
        }

        fn network_error() -> Self {
            Self {
                result: Err(CoatCheckApiError::Network("connection refused".into())),
            }
        }

        fn upstream_error() -> Self {
            Self {
                result: Err(CoatCheckApiError::Upstream {
                    error: "internal_error".into(),
                    detail: Some("something went wrong".into()),
                }),
            }
        }
    }

    #[async_trait::async_trait]
    impl CoatCheckApiPort for FakeCoatCheckApi {
        async fn check(
            &self,
            _locations: Vec<CoatCheckLocation>,
        ) -> Result<CoatCheckResult, CoatCheckApiError> {
            match &self.result {
                Ok(r) => Ok(CoatCheckResult {
                    recommendation: r.recommendation.clone(),
                    reason: r.reason.clone(),
                    locations: r
                        .locations
                        .iter()
                        .map(|l| CoatCheckLocationResult {
                            label: l.label.clone(),
                            lat: l.lat,
                            lon: l.lon,
                            recommendation: l.recommendation.clone(),
                            reasons: l.reasons.clone(),
                            temp_max_celsius: l.temp_max_celsius,
                            temp_min_celsius: l.temp_min_celsius,
                            feels_like_min_celsius: l.feels_like_min_celsius,
                            precipitation_mm: l.precipitation_mm,
                            wind_speed_max_kmh: l.wind_speed_max_kmh,
                        })
                        .collect(),
                }),
                Err(CoatCheckApiError::Network(msg)) => {
                    Err(CoatCheckApiError::Network(msg.clone()))
                }
                Err(CoatCheckApiError::Upstream { error, detail }) => {
                    Err(CoatCheckApiError::Upstream {
                        error: error.clone(),
                        detail: detail.clone(),
                    })
                }
            }
        }
    }

    #[derive(Clone)]
    struct FakeGeocoding {
        results: Vec<GeocodingResult>,
        should_fail: bool,
    }

    impl FakeGeocoding {
        fn with_results(results: Vec<GeocodingResult>) -> Self {
            Self {
                results,
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                results: vec![],
                should_fail: true,
            }
        }
    }

    #[async_trait::async_trait]
    impl GeocodingPort for FakeGeocoding {
        async fn geocode(&self, _query: &str) -> Result<Vec<GeocodingResult>, GeocodingError> {
            if self.should_fail {
                return Err(GeocodingError::RequestFailed("connection refused".into()));
            }
            Ok(self
                .results
                .iter()
                .map(|r| GeocodingResult {
                    name: r.name.clone(),
                    latitude: r.latitude,
                    longitude: r.longitude,
                    country: r.country.clone(),
                    admin1: r.admin1.clone(),
                })
                .collect())
        }
    }

    fn empty_geocoding() -> FakeGeocoding {
        FakeGeocoding::with_results(vec![])
    }

    fn service(
        api: FakeCoatCheckApi,
        geo: FakeGeocoding,
    ) -> WebService<FakeCoatCheckApi, FakeGeocoding> {
        WebService::new(api, geo)
    }

    fn london() -> LocationInput {
        LocationInput {
            lat: "51.5".into(),
            lon: "-0.1".into(),
            label: Some("London".into()),
        }
    }

    #[tokio::test]
    async fn check_coat_returns_coat_result() {
        let svc = service(FakeCoatCheckApi::coat(), empty_geocoding());
        let result = svc.check_coat(vec![london()]).await.unwrap();
        assert_eq!(result.recommendation, "coat");
        assert_eq!(result.locations[0].display_name, "London");
        assert_eq!(result.locations[0].recommendation_label, "Coat");
    }

    #[tokio::test]
    async fn check_coat_returns_no_coat_result() {
        let svc = service(FakeCoatCheckApi::no_coat(), empty_geocoding());
        let result = svc.check_coat(vec![london()]).await.unwrap();
        assert_eq!(result.recommendation, "no");
        assert_eq!(result.locations[0].recommendation_label, "All clear");
    }

    #[tokio::test]
    async fn check_coat_no_locations() {
        let svc = service(FakeCoatCheckApi::no_coat(), empty_geocoding());
        let err = svc.check_coat(vec![]).await.unwrap_err();
        assert!(matches!(err, WebPortError::NoLocations));
    }

    #[tokio::test]
    async fn check_coat_invalid_latitude() {
        let svc = service(FakeCoatCheckApi::no_coat(), empty_geocoding());
        let input = LocationInput {
            lat: "abc".into(),
            lon: "-0.1".into(),
            label: Some("London".into()),
        };
        let err = svc.check_coat(vec![input]).await.unwrap_err();
        assert!(matches!(err, WebPortError::InvalidLatitude));
    }

    #[tokio::test]
    async fn check_coat_invalid_longitude() {
        let svc = service(FakeCoatCheckApi::no_coat(), empty_geocoding());
        let input = LocationInput {
            lat: "51.5".into(),
            lon: "xyz".into(),
            label: Some("London".into()),
        };
        let err = svc.check_coat(vec![input]).await.unwrap_err();
        assert!(matches!(err, WebPortError::InvalidLongitude));
    }

    #[tokio::test]
    async fn check_coat_network_error() {
        let svc = service(FakeCoatCheckApi::network_error(), empty_geocoding());
        let err = svc.check_coat(vec![london()]).await.unwrap_err();
        assert!(matches!(err, WebPortError::ServiceUnavailable(_)));
    }

    #[tokio::test]
    async fn check_coat_upstream_error() {
        let svc = service(FakeCoatCheckApi::upstream_error(), empty_geocoding());
        let err = svc.check_coat(vec![london()]).await.unwrap_err();
        assert!(matches!(err, WebPortError::UpstreamError { .. }));
    }

    #[tokio::test]
    async fn check_coat_no_label_uses_coords() {
        let api = FakeCoatCheckApi {
            result: Ok(CoatCheckResult {
                recommendation: "no".into(),
                reason: "All clear.".into(),
                locations: vec![CoatCheckLocationResult {
                    label: None,
                    lat: 51.5,
                    lon: -0.1,
                    recommendation: "no".into(),
                    reasons: vec![],
                    temp_max_celsius: 20.0,
                    temp_min_celsius: 15.0,
                    feels_like_min_celsius: 14.5,
                    precipitation_mm: 0.0,
                    wind_speed_max_kmh: 5.0,
                }],
            }),
        };
        let svc = service(api, empty_geocoding());
        let input = LocationInput {
            lat: "51.5".into(),
            lon: "-0.1".into(),
            label: None,
        };
        let result = svc.check_coat(vec![input]).await.unwrap();
        assert!(result.locations[0].display_name.contains("51.50"));
    }

    #[tokio::test]
    async fn search_returns_suggestions() {
        let geo = FakeGeocoding::with_results(vec![GeocodingResult {
            name: "Reading".into(),
            latitude: 51.45625,
            longitude: -0.97113,
            country: Some("United Kingdom".into()),
            admin1: Some("England".into()),
        }]);
        let svc = service(FakeCoatCheckApi::no_coat(), geo);
        let results = svc.search_locations("Reading").await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].name.contains("Reading"));
        assert!(results[0].name.contains("United Kingdom"));
    }

    #[tokio::test]
    async fn search_short_query_returns_empty() {
        let svc = service(FakeCoatCheckApi::no_coat(), empty_geocoding());
        let results = svc.search_locations("L").await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn search_geocoding_failure() {
        let svc = service(FakeCoatCheckApi::no_coat(), FakeGeocoding::failing());
        let err = svc.search_locations("London").await.unwrap_err();
        assert!(matches!(err, WebPortError::GeocodingFailed(_)));
    }
}
