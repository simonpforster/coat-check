use askama::Template;
use askama_web::WebTemplate;
use serde::Deserialize;

use coat_check_common::Recommendation;

use crate::domain::{recommendation::LocationView, suggestion::Suggestion};

pub const RECOMMENDATION_OPTIONS: &[(&str, &str)] = &[
    (Recommendation::No.as_str(), Recommendation::No.label()),
    (
        Recommendation::Umbrella.as_str(),
        Recommendation::Umbrella.label(),
    ),
    (
        Recommendation::RainJacket.as_str(),
        Recommendation::RainJacket.label(),
    ),
    (Recommendation::Coat.as_str(), Recommendation::Coat.label()),
];

// ── Config ──────────────────────────────────────────────────────────────────

pub struct WebConfig {
    pub base_url: Option<String>,
    pub ga_id: Option<String>,
}

pub(super) struct AppContext<P: super::WebPort> {
    pub service: P,
    pub config: WebConfig,
}

// ── Templates ───────────────────────────────────────────────────────────────

#[derive(Template, WebTemplate)]
#[template(path = "base.html")]
pub(super) struct IndexTemplate {
    pub base_url: Option<String>,
    pub ga_id: Option<String>,
}

#[derive(Template, WebTemplate)]
#[template(path = "result.html")]
pub(super) struct ResultTemplate {
    pub prediction_id: Option<String>,
    pub recommendation: String,
    pub overall_class: String,
    pub reason: String,
    pub locations: Vec<LocationView>,
}

#[derive(Template, WebTemplate)]
#[template(path = "email_success.html")]
pub(super) struct EmailSuccessTemplate;

#[derive(Template, WebTemplate)]
#[template(path = "feedback.html")]
pub(super) struct FeedbackTemplate {
    pub token: String,
    pub recommendation: String,
    pub reason: String,
    pub options: &'static [(&'static str, &'static str)],
}

#[derive(Template, WebTemplate)]
#[template(path = "feedback_thanks.html")]
pub(super) struct FeedbackThanksTemplate;

#[derive(Template, WebTemplate)]
#[template(path = "feedback_expired.html")]
pub(super) struct FeedbackExpiredTemplate;

#[derive(Template, WebTemplate)]
#[template(path = "unsubscribe_success.html")]
pub(super) struct UnsubscribeSuccessTemplate;

#[derive(Template, WebTemplate)]
#[template(path = "error.html")]
pub(super) struct ErrorTemplate {
    pub error: String,
    pub detail: Option<String>,
}

#[derive(Template, WebTemplate)]
#[template(path = "suggestions.html")]
pub(super) struct SuggestionsTemplate {
    pub suggestions: Vec<Suggestion>,
}

// ── Form DTOs ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(super) struct CheckForm {
    pub locations: String,
}

#[derive(Deserialize)]
pub(super) struct FormLocation {
    pub lat: String,
    pub lon: String,
    pub label: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct SearchQuery {
    pub q: String,
}

#[derive(Deserialize)]
pub(super) struct EmailForm {
    pub prediction_id: String,
    pub email: String,
}

#[derive(Deserialize)]
pub(super) struct FeedbackQuery {
    pub token: String,
}

#[derive(Deserialize)]
pub(super) struct FeedbackForm {
    pub token: String,
    pub brought: String,
    pub should_have_brought: String,
    pub comment: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct UnsubscribeForm {
    pub email: String,
}
