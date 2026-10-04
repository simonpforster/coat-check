use tracing::info;
use uuid::Uuid;

use crate::ports::outbound::{NotificationSendError, NotificationSenderPort};

#[derive(Clone)]
pub struct LogNotifier {
    base_url: String,
}

impl LogNotifier {
    pub fn new(base_url: String) -> Self {
        Self { base_url }
    }
}

#[async_trait::async_trait]
impl NotificationSenderPort for LogNotifier {
    async fn send_feedback_request(
        &self,
        to: &str,
        prediction_id: Uuid,
    ) -> Result<(), NotificationSendError> {
        let feedback_url = format!("{}/feedback?token={}", self.base_url, prediction_id);
        info!(contact = %to, url = %feedback_url, "feedback notification ready");
        Ok(())
    }
}
