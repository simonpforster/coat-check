use coat_check_common::Recommendation;
use uuid::Uuid;

use crate::{
    domain::feedback::{FeedbackDetail, FeedbackEntry, FeedbackPage, FeedbackStats},
    ports::{
        inbound::{DashboardError, DashboardPort},
        outbound::FeedbackStorePort,
    },
};

const PAGE_SIZE: i64 = 50;

#[derive(Clone)]
pub struct DashboardService<S: FeedbackStorePort + Clone> {
    store: S,
}

impl<S: FeedbackStorePort + Clone> DashboardService<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl<S: FeedbackStorePort + Clone> DashboardPort for DashboardService<S> {
    async fn get_feedback_page(&self, page: i64) -> Result<FeedbackPage, DashboardError> {
        let totals = self
            .store
            .get_totals()
            .await
            .map_err(|e| DashboardError::Store(e.to_string()))?;

        let match_rate = if totals.total > 0 {
            format!(
                "{:.0}%",
                (totals.matched as f64 / totals.total as f64) * 100.0
            )
        } else {
            "—".into()
        };

        let total_pages = (totals.total + PAGE_SIZE - 1) / PAGE_SIZE;
        let page = page.max(1).min(total_pages.max(1));
        let offset = (page - 1) * PAGE_SIZE;

        let rows = self
            .store
            .list_feedback(PAGE_SIZE, offset)
            .await
            .map_err(|e| DashboardError::Store(e.to_string()))?;

        let entries = rows
            .into_iter()
            .map(|row| {
                let matched = row.feedback_brought == row.feedback_should_have_brought;
                FeedbackEntry {
                    prediction_id: row.prediction_id,
                    recommendation: Recommendation::from_str_label(&row.recommendation),
                    prediction_at: row.prediction_at.format("%Y-%m-%d %H:%M").to_string(),
                    feedback_brought: Recommendation::from_str_label(&row.feedback_brought),
                    feedback_should_have_brought: Recommendation::from_str_label(
                        &row.feedback_should_have_brought,
                    ),
                    feedback_comment: row.feedback_comment.unwrap_or_default(),
                    feedback_at: row.feedback_at.format("%Y-%m-%d %H:%M").to_string(),
                    matched,
                }
            })
            .collect();

        Ok(FeedbackPage {
            entries,
            stats: FeedbackStats {
                total: totals.total,
                matched: totals.matched,
                match_rate,
            },
            page,
            total_pages,
        })
    }

    async fn get_feedback_detail(
        &self,
        prediction_id: Uuid,
    ) -> Result<FeedbackDetail, DashboardError> {
        let row = self
            .store
            .get_feedback_detail(prediction_id)
            .await
            .map_err(|e| DashboardError::Store(e.to_string()))?
            .ok_or(DashboardError::NotFound)?;

        let matched = row.feedback_brought == row.feedback_should_have_brought;

        Ok(FeedbackDetail {
            prediction_id: row.prediction_id,
            recommendation: Recommendation::from_str_label(&row.recommendation),
            reason: row.reason,
            locations_json: serde_json::to_string_pretty(&row.locations_json).unwrap_or_default(),
            actual_weather_json: row
                .actual_weather_json
                .map(|v| serde_json::to_string_pretty(&v).unwrap_or_default()),
            prediction_at: row.prediction_at.format("%Y-%m-%d %H:%M").to_string(),
            feedback_brought: Recommendation::from_str_label(&row.feedback_brought),
            feedback_should_have_brought: Recommendation::from_str_label(
                &row.feedback_should_have_brought,
            ),
            feedback_comment: row.feedback_comment.unwrap_or_default(),
            feedback_at: row.feedback_at.format("%Y-%m-%d %H:%M").to_string(),
            matched,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::outbound::{
        FeedbackDetailRow, FeedbackRow, FeedbackStoreError, FeedbackTotals,
    };
    use chrono::Utc;
    use uuid::Uuid;

    #[derive(Clone)]
    struct FakeStore {
        rows: Vec<FeedbackRow>,
        totals: FeedbackTotals,
    }

    #[async_trait::async_trait]
    impl FeedbackStorePort for FakeStore {
        async fn get_totals(&self) -> Result<FeedbackTotals, FeedbackStoreError> {
            Ok(FeedbackTotals {
                total: self.totals.total,
                matched: self.totals.matched,
            })
        }

        async fn list_feedback(
            &self,
            limit: i64,
            offset: i64,
        ) -> Result<Vec<FeedbackRow>, FeedbackStoreError> {
            Ok(self
                .rows
                .iter()
                .skip(offset as usize)
                .take(limit as usize)
                .cloned()
                .collect())
        }

        async fn get_feedback_detail(
            &self,
            _prediction_id: Uuid,
        ) -> Result<Option<FeedbackDetailRow>, FeedbackStoreError> {
            Ok(None)
        }
    }

    fn make_row(brought: &str, should: &str) -> FeedbackRow {
        FeedbackRow {
            prediction_id: Uuid::new_v4(),
            recommendation: "coat".into(),
            prediction_at: Utc::now(),
            feedback_brought: brought.into(),
            feedback_should_have_brought: should.into(),
            feedback_comment: None,
            feedback_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn empty_store_returns_empty_page() {
        let svc = DashboardService::new(FakeStore {
            rows: vec![],
            totals: FeedbackTotals {
                total: 0,
                matched: 0,
            },
        });
        let page = svc.get_feedback_page(1).await.unwrap();
        assert!(page.entries.is_empty());
        assert_eq!(page.stats.total, 0);
        assert_eq!(page.stats.match_rate, "—");
        assert_eq!(page.page, 1);
        assert_eq!(page.total_pages, 0);
    }

    #[tokio::test]
    async fn stats_reflect_full_dataset() {
        let svc = DashboardService::new(FakeStore {
            rows: vec![make_row("coat", "coat"), make_row("umbrella", "coat")],
            totals: FeedbackTotals {
                total: 2,
                matched: 1,
            },
        });
        let page = svc.get_feedback_page(1).await.unwrap();
        assert_eq!(page.stats.total, 2);
        assert_eq!(page.stats.matched, 1);
        assert_eq!(page.stats.match_rate, "50%");
    }

    #[tokio::test]
    async fn page_clamped_to_valid_range() {
        let svc = DashboardService::new(FakeStore {
            rows: vec![make_row("coat", "coat")],
            totals: FeedbackTotals {
                total: 1,
                matched: 1,
            },
        });
        let page = svc.get_feedback_page(999).await.unwrap();
        assert_eq!(page.page, 1);

        let page = svc.get_feedback_page(0).await.unwrap();
        assert_eq!(page.page, 1);
    }

    #[tokio::test]
    async fn labels_converted_from_stored_values() {
        let svc = DashboardService::new(FakeStore {
            rows: vec![make_row("rain_jacket", "coat")],
            totals: FeedbackTotals {
                total: 1,
                matched: 0,
            },
        });
        let page = svc.get_feedback_page(1).await.unwrap();
        assert_eq!(page.entries[0].feedback_brought, "Rain jacket");
        assert_eq!(page.entries[0].feedback_should_have_brought, "Coat");
        assert!(!page.entries[0].matched);
    }
}
