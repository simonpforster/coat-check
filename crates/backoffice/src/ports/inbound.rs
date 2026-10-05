use thiserror::Error;
use uuid::Uuid;

use crate::domain::feedback::{FeedbackDetail, FeedbackPage};

#[derive(Debug, Error)]
pub enum DashboardError {
    #[error("store error: {0}")]
    Store(String),
    #[error("not found")]
    NotFound,
}

#[async_trait::async_trait]
pub trait DashboardPort: Send + Sync {
    async fn get_feedback_page(&self, page: i64) -> Result<FeedbackPage, DashboardError>;

    async fn get_feedback_detail(
        &self,
        prediction_id: Uuid,
    ) -> Result<FeedbackDetail, DashboardError>;
}
