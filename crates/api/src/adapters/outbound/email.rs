use lettre::{message::Mailbox, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use uuid::Uuid;

use crate::ports::outbound::{NotificationSendError, NotificationSenderPort};

#[derive(Clone)]
pub struct SmtpEmailSender {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
    base_url: String,
}

impl SmtpEmailSender {
    pub fn new(smtp_host: &str, from_address: &str, base_url: String) -> Result<Self, String> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::relay(smtp_host)
            .map_err(|e| e.to_string())?
            .build();

        let from: Mailbox = from_address
            .parse()
            .map_err(|e: lettre::address::AddressError| e.to_string())?;

        Ok(Self {
            transport,
            from,
            base_url,
        })
    }
}

#[async_trait::async_trait]
impl NotificationSenderPort for SmtpEmailSender {
    async fn send_feedback_request(
        &self,
        to: &str,
        prediction_id: Uuid,
        _prediction_date: chrono::NaiveDate,
    ) -> Result<(), NotificationSendError> {
        let to_mailbox: Mailbox = to.parse().map_err(|e: lettre::address::AddressError| {
            NotificationSendError::SendFailed(e.to_string())
        })?;

        let feedback_url = format!("{}/feedback?token={}", self.base_url, prediction_id);

        let body = format!(
            "Hi!\n\n\
             Earlier today you used Coat Check for a weather recommendation.\n\
             Was it accurate? Let us know:\n\n\
             {}\n\n\
             Thanks!",
            feedback_url
        );

        let email = Message::builder()
            .from(self.from.clone())
            .to(to_mailbox)
            .subject("How was our coat recommendation?")
            .body(body)
            .map_err(|e| NotificationSendError::SendFailed(e.to_string()))?;

        self.transport
            .send(email)
            .await
            .map_err(|e| NotificationSendError::SendFailed(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_with_valid_inputs() {
        let result = SmtpEmailSender::new(
            "smtp.example.com",
            "test@example.com",
            "https://example.com".into(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn new_with_invalid_from_address_fails() {
        let result = SmtpEmailSender::new(
            "smtp.example.com",
            "not-an-email",
            "https://example.com".into(),
        );
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn send_to_invalid_address_returns_error() {
        let sender = SmtpEmailSender::new(
            "smtp.example.com",
            "test@example.com",
            "https://example.com".into(),
        )
        .unwrap();

        let result = sender
            .send_feedback_request(
                "not-an-email",
                Uuid::new_v4(),
                chrono::NaiveDate::from_ymd_opt(2026, 10, 5).unwrap(),
            )
            .await;
        assert!(result.is_err());
    }
}
