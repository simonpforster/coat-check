use tokio::time::{interval, Duration};
use tracing::{info, warn};

use crate::ports::outbound::{FeedbackStoreError, FeedbackStorePort, NotificationSenderPort};

pub async fn run<F, N>(store: F, sender: N)
where
    F: FeedbackStorePort + Clone + 'static,
    N: NotificationSenderPort + Clone + 'static,
{
    let mut tick = interval(Duration::from_secs(60));
    loop {
        tick.tick().await;
        process_batch(&store, &sender).await;
    }
}

async fn process_batch<F: FeedbackStorePort, N: NotificationSenderPort>(store: &F, sender: &N) {
    match store.fetch_ready_notifications().await {
        Ok(notifications) => {
            for notification in notifications {
                match store.get_prediction(notification.prediction_id).await {
                    Err(FeedbackStoreError::NotFound) => {
                        info!(id = %notification.id, prediction_id = %notification.prediction_id, "prediction deleted, removing orphaned notification");
                        if let Err(e) = store.delete_notification(notification.id).await {
                            warn!(error = %e, id = %notification.id, "failed to delete orphaned notification");
                        }
                        continue;
                    }
                    Err(e) => {
                        warn!(error = %e, id = %notification.id, "failed to verify prediction, skipping notification");
                        continue;
                    }
                    Ok(_) => {}
                }

                match sender
                    .send_feedback_request(&notification.contact, notification.prediction_id)
                    .await
                {
                    Ok(()) => {
                        if let Err(e) = store.mark_notification_sent(notification.id).await {
                            warn!(error = %e, id = %notification.id, "failed to mark notification as sent");
                        } else {
                            info!(contact = %notification.contact, "feedback notification sent");
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, contact = %notification.contact, "failed to send feedback notification");
                        if let Err(mark_err) = store
                            .mark_notification_failed(notification.id, &e.to_string())
                            .await
                        {
                            warn!(error = %mark_err, id = %notification.id, "failed to record notification failure");
                        }
                    }
                }
            }
        }
        Err(e) => {
            warn!(error = %e, "failed to fetch ready notifications from queue");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::prediction::{Feedback, Prediction, PredictionLocation},
        ports::outbound::{
            FeedbackStoreError, FeedbackStorePort, NotificationSendError, NotificationSenderPort,
            QueuedNotification,
        },
    };
    use chrono::{DateTime, Utc};
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    #[derive(Clone)]
    struct MockStore {
        notifications: Vec<QueuedNotification>,
        known_predictions: Arc<Mutex<Vec<Uuid>>>,
        sent: Arc<Mutex<Vec<Uuid>>>,
        failed: Arc<Mutex<Vec<(Uuid, String)>>>,
        deleted: Arc<Mutex<Vec<Uuid>>>,
    }

    impl MockStore {
        fn with_notifications(notifications: Vec<QueuedNotification>) -> Self {
            let prediction_ids: Vec<Uuid> = notifications.iter().map(|n| n.prediction_id).collect();
            Self {
                notifications,
                known_predictions: Arc::new(Mutex::new(prediction_ids)),
                sent: Arc::new(Mutex::new(vec![])),
                failed: Arc::new(Mutex::new(vec![])),
                deleted: Arc::new(Mutex::new(vec![])),
            }
        }
    }

    #[async_trait::async_trait]
    impl FeedbackStorePort for MockStore {
        async fn save_prediction(
            &self,
            _rec: &str,
            _reason: &str,
            _locs: &[PredictionLocation],
        ) -> Result<Uuid, FeedbackStoreError> {
            unimplemented!()
        }

        async fn get_prediction(&self, id: Uuid) -> Result<Prediction, FeedbackStoreError> {
            if self.known_predictions.lock().unwrap().contains(&id) {
                Ok(Prediction {
                    id,
                    recommendation: "coat".into(),
                    reason: "test".into(),
                    locations: vec![],
                    created_at: Utc::now(),
                })
            } else {
                Err(FeedbackStoreError::NotFound)
            }
        }

        async fn enqueue_notification(
            &self,
            _pid: Uuid,
            _contact: &str,
            _send_after: DateTime<Utc>,
        ) -> Result<(), FeedbackStoreError> {
            unimplemented!()
        }

        async fn fetch_ready_notifications(
            &self,
        ) -> Result<Vec<QueuedNotification>, FeedbackStoreError> {
            Ok(self.notifications.clone())
        }

        async fn delete_notification(&self, id: Uuid) -> Result<(), FeedbackStoreError> {
            self.deleted.lock().unwrap().push(id);
            Ok(())
        }

        async fn mark_notification_sent(&self, id: Uuid) -> Result<(), FeedbackStoreError> {
            self.sent.lock().unwrap().push(id);
            Ok(())
        }

        async fn mark_notification_failed(
            &self,
            id: Uuid,
            error: &str,
        ) -> Result<(), FeedbackStoreError> {
            self.failed.lock().unwrap().push((id, error.to_string()));
            Ok(())
        }

        async fn get_notification_sent_at(
            &self,
            _prediction_id: Uuid,
        ) -> Result<DateTime<Utc>, FeedbackStoreError> {
            unimplemented!()
        }

        async fn record_feedback(
            &self,
            _prediction: &Prediction,
            _feedback: &Feedback,
            _actual_weather: Option<&[PredictionLocation]>,
        ) -> Result<(), FeedbackStoreError> {
            unimplemented!()
        }
    }

    #[derive(Clone)]
    struct AlwaysSendsSender;

    #[async_trait::async_trait]
    impl NotificationSenderPort for AlwaysSendsSender {
        async fn send_feedback_request(
            &self,
            _to: &str,
            _prediction_id: Uuid,
        ) -> Result<(), NotificationSendError> {
            Ok(())
        }
    }

    #[derive(Clone)]
    struct AlwaysFailsSender;

    #[async_trait::async_trait]
    impl NotificationSenderPort for AlwaysFailsSender {
        async fn send_feedback_request(
            &self,
            _to: &str,
            _prediction_id: Uuid,
        ) -> Result<(), NotificationSendError> {
            Err(NotificationSendError::SendFailed("smtp timeout".into()))
        }
    }

    fn notification(id: Uuid, prediction_id: Uuid) -> QueuedNotification {
        QueuedNotification {
            id,
            prediction_id,
            contact: "test@example.com".into(),
        }
    }

    #[tokio::test]
    async fn empty_batch_does_nothing() {
        let store = MockStore::with_notifications(vec![]);
        process_batch(&store, &AlwaysSendsSender).await;
        assert!(store.sent.lock().unwrap().is_empty());
        assert!(store.failed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn successful_send_marks_sent() {
        let nid = Uuid::new_v4();
        let pid = Uuid::new_v4();
        let store = MockStore::with_notifications(vec![notification(nid, pid)]);
        process_batch(&store, &AlwaysSendsSender).await;
        assert_eq!(*store.sent.lock().unwrap(), vec![nid]);
        assert!(store.failed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn failed_send_marks_failed() {
        let nid = Uuid::new_v4();
        let pid = Uuid::new_v4();
        let store = MockStore::with_notifications(vec![notification(nid, pid)]);
        process_batch(&store, &AlwaysFailsSender).await;
        assert!(store.sent.lock().unwrap().is_empty());
        let failed = store.failed.lock().unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].0, nid);
        assert!(failed[0].1.contains("smtp timeout"));
    }

    #[tokio::test]
    async fn processes_multiple_notifications() {
        let n1 = notification(Uuid::new_v4(), Uuid::new_v4());
        let n2 = notification(Uuid::new_v4(), Uuid::new_v4());
        let id1 = n1.id;
        let id2 = n2.id;
        let store = MockStore::with_notifications(vec![n1, n2]);
        process_batch(&store, &AlwaysSendsSender).await;
        let sent = store.sent.lock().unwrap();
        assert_eq!(sent.len(), 2);
        assert!(sent.contains(&id1));
        assert!(sent.contains(&id2));
    }

    #[tokio::test]
    async fn orphaned_notification_deleted() {
        let nid = Uuid::new_v4();
        let orphan_pid = Uuid::new_v4();
        let store = MockStore::with_notifications(vec![notification(nid, orphan_pid)]);
        store.known_predictions.lock().unwrap().clear();
        process_batch(&store, &AlwaysSendsSender).await;
        assert!(store.sent.lock().unwrap().is_empty());
        assert_eq!(*store.deleted.lock().unwrap(), vec![nid]);
    }
}
