#[derive(Debug)]
pub struct CheckResult {
    pub recommendation: String,
    pub reason: String,
    pub locations: Vec<LocationView>,
}

#[derive(Debug)]
pub struct LocationView {
    pub display_name: String,
    pub recommendation_label: String,
    pub temp_min: String,
    pub temp_max: String,
    pub feels_like: String,
    pub precipitation: String,
    pub wind: String,
    pub reasons: Vec<String>,
}
