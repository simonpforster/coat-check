use uuid::Uuid;

pub struct FeedbackEntry {
    pub prediction_id: Uuid,
    pub recommendation: String,
    pub prediction_at: String,
    pub feedback_brought: String,
    pub feedback_should_have_brought: String,
    pub feedback_comment: String,
    pub feedback_at: String,
    pub matched: bool,
}

pub struct FeedbackDetail {
    pub prediction_id: Uuid,
    pub recommendation: String,
    pub reason: String,
    pub locations_json: String,
    pub actual_weather_json: Option<String>,
    pub prediction_at: String,
    pub feedback_brought: String,
    pub feedback_should_have_brought: String,
    pub feedback_comment: String,
    pub feedback_at: String,
    pub matched: bool,
}

pub struct FeedbackStats {
    pub total: i64,
    pub matched: i64,
    pub match_rate: String,
}

pub struct FeedbackPage {
    pub entries: Vec<FeedbackEntry>,
    pub stats: FeedbackStats,
    pub page: i64,
    pub total_pages: i64,
}
