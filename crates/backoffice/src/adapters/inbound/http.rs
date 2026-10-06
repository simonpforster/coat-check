use std::sync::Arc;

use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Router,
};
use uuid::Uuid;

use crate::{
    domain::feedback::{FeedbackDetail, FeedbackEntry, FeedbackStats},
    ports::inbound::{DashboardError, DashboardPort},
};

#[derive(serde::Deserialize)]
struct PaginationParams {
    page: Option<i64>,
}

struct PaginationView {
    page: i64,
    total_pages: i64,
    has_prev: bool,
    has_next: bool,
}

#[derive(Template, WebTemplate)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    entries: Vec<FeedbackEntry>,
    stats: FeedbackStats,
    pagination: PaginationView,
}

#[derive(Template, WebTemplate)]
#[template(path = "detail.html")]
struct DetailTemplate {
    detail: FeedbackDetail,
}

async fn dashboard_handler(
    State(svc): State<Arc<dyn DashboardPort>>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let page_num = params.page.unwrap_or(1);
    let feedback_page = svc.get_feedback_page(page_num).await.unwrap();

    DashboardTemplate {
        entries: feedback_page.entries,
        stats: feedback_page.stats,
        pagination: PaginationView {
            page: feedback_page.page,
            total_pages: feedback_page.total_pages,
            has_prev: feedback_page.page > 1,
            has_next: feedback_page.page < feedback_page.total_pages,
        },
    }
}

async fn detail_handler(
    State(svc): State<Arc<dyn DashboardPort>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let prediction_id = match id.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid prediction id").into_response(),
    };

    match svc.get_feedback_detail(prediction_id).await {
        Ok(detail) => DetailTemplate { detail }.into_response(),
        Err(DashboardError::NotFound) => {
            (StatusCode::NOT_FOUND, "feedback entry not found").into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub fn router(dashboard: Arc<dyn DashboardPort>) -> Router {
    Router::new()
        .route("/", get(dashboard_handler))
        .route("/feedback/{id}", get(detail_handler))
        .route("/health", get(|| async { "ok" }))
        .with_state(dashboard)
}
