use std::collections::HashMap;

use reqwest::Client;
use serde::Serialize;
use uuid::Uuid;

use crate::ports::outbound::{NotificationSendError, NotificationSenderPort};

#[derive(Serialize)]
struct SendEmailRequest {
    from: String,
    to: Vec<String>,
    reply_to: Vec<String>,
    subject: String,
    html: String,
    text: String,
    headers: HashMap<String, String>,
}

#[derive(Clone)]
pub struct ResendEmailSender {
    client: Client,
    api_key: String,
    from: String,
    base_url: String,
    api_url: String,
}

impl ResendEmailSender {
    pub fn new(api_key: String, from: String, base_url: String, api_url: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            from,
            base_url,
            api_url,
        }
    }
}

#[async_trait::async_trait]
impl NotificationSenderPort for ResendEmailSender {
    async fn send_feedback_request(
        &self,
        to: &str,
        prediction_id: Uuid,
        prediction_date: chrono::NaiveDate,
    ) -> Result<(), NotificationSendError> {
        let feedback_url = format!("{}/feedback?token={}", self.base_url, prediction_id);

        let unsubscribe_url = format!(
            "{}/feedback/unsubscribe?contact={}",
            self.api_url,
            urlencoding::encode(to)
        );

        let mut headers = HashMap::new();
        headers.insert("List-Unsubscribe".into(), format!("<{unsubscribe_url}>"));
        headers.insert(
            "List-Unsubscribe-Post".into(),
            "List-Unsubscribe=One-Click".into(),
        );

        let body = SendEmailRequest {
            from: self.from.clone(),
            to: vec![to.to_string()],
            reply_to: vec!["simon@coat-check.org".into()],
            subject: format!(
                "Did you need a coat on {}?",
                prediction_date.format("%b %-d")
            ),
            html: include_str!("../../../templates/feedback_email.html")
                .replace("{{feedback_url}}", &feedback_url),
            text: include_str!("../../../templates/feedback_email.txt")
                .replace("{{feedback_url}}", &feedback_url),
            headers,
        };

        let resp = self
            .client
            .post("https://api.resend.com/emails")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| NotificationSendError::SendFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(NotificationSendError::SendFailed(format!(
                "Resend API {status}: {text}"
            )));
        }

        Ok(())
    }
}
