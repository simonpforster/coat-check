mod dto;

pub use dto::WebConfig;

use std::sync::Arc;

use axum::{
    extract::{Form, State},
    http::{header, HeaderMap, Method, StatusCode},
    middleware::{self, Next},
    response::IntoResponse,
    routing::get,
    Router,
};
use tower_http::trace::TraceLayer;
use tracing::warn;

use crate::ports::inbound::{LocationInput, WebPort, WebPortError};

use dto::*;

// ── CSRF protection ────────────────────────────────────────────────────────

/// Extract hostname from a URL string (e.g. "http://localhost:3000/path" → "localhost")
fn extract_host(url: &str) -> Option<&str> {
    let after_scheme = url.find("://").map(|i| &url[i + 3..]).unwrap_or(url);
    let host_port = after_scheme.split('/').next()?;
    Some(host_port.split(':').next().unwrap_or(host_port))
}

async fn csrf_check(
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> impl IntoResponse {
    if request.method() != Method::POST {
        return next.run(request).await;
    }

    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let host_name = host.split(':').next().unwrap_or(host);

    let origin_host = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .and_then(extract_host);

    let referer_host = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .and_then(extract_host);

    // If neither Origin nor Referer is present, allow (non-browser client).
    // Only reject when Origin/Referer IS present and doesn't match the Host.
    let allowed = origin_host
        .or(referer_host)
        .map(|h| h == host_name)
        .unwrap_or(true);

    if allowed {
        next.run(request).await
    } else {
        warn!("CSRF check failed: origin/referer mismatch");
        StatusCode::FORBIDDEN.into_response()
    }
}

// ── Router ──────────────────────────────────────────────────────────────────

pub fn router<P>(service: P, config: WebConfig) -> Router
where
    P: WebPort + Clone + 'static,
{
    Router::new()
        .route("/", get(index::<P>))
        .route("/search", get(search_handler::<P>))
        .route("/check", axum::routing::post(check_handler::<P>))
        .route("/email", axum::routing::post(email_handler::<P>))
        .route(
            "/feedback",
            get(feedback_page::<P>).post(feedback_handler::<P>),
        )
        .route(
            "/unsubscribe",
            axum::routing::post(unsubscribe_handler::<P>),
        )
        .route("/offline", get(offline_handler))
        .route("/manifest.json", get(manifest_handler))
        .route("/sw.js", get(sw_handler))
        .route("/health", get(|| async { "ok" }))
        .with_state(Arc::new(AppContext { service, config }))
        .layer(middleware::from_fn(csrf_check))
        .layer(TraceLayer::new_for_http())
}

// ── PWA ────────────────────────────────────────────────────────────────────

async fn offline_handler() -> OfflineTemplate {
    OfflineTemplate
}

async fn manifest_handler() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/manifest+json")],
        include_str!("../../../../static/manifest.json"),
    )
}

async fn sw_handler() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        include_str!("../../../../static/sw.js"),
    )
}

// ── Handlers ────────────────────────────────────────────────────────────────

async fn index<P: WebPort>(State(ctx): State<Arc<AppContext<P>>>) -> IndexTemplate {
    IndexTemplate {
        base_url: ctx.config.base_url.clone(),
        ga_id: ctx.config.ga_id.clone(),
    }
}

async fn search_handler<P: WebPort>(
    State(ctx): State<Arc<AppContext<P>>>,
    axum::extract::Query(query): axum::extract::Query<SearchQuery>,
) -> impl IntoResponse {
    match ctx.service.search_locations(&query.q).await {
        Ok(suggestions) => SuggestionsTemplate { suggestions }.into_response(),
        Err(e) => {
            warn!(error = %e, "search failed");
            SuggestionsTemplate {
                suggestions: vec![],
            }
            .into_response()
        }
    }
}

async fn check_handler<P: WebPort>(
    State(ctx): State<Arc<AppContext<P>>>,
    Form(form): Form<CheckForm>,
) -> impl IntoResponse {
    let form_locations: Vec<FormLocation> = match serde_json::from_str(&form.locations) {
        Ok(locs) => locs,
        Err(_) => {
            return ErrorTemplate {
                error: "Invalid request".into(),
                detail: Some("Could not parse locations.".into()),
            }
            .into_response();
        }
    };

    let locations: Vec<LocationInput> = form_locations
        .into_iter()
        .map(|l| LocationInput {
            lat: l.lat,
            lon: l.lon,
            label: l.label,
        })
        .collect();

    match ctx.service.check_coat(locations).await {
        Ok(result) => ResultTemplate {
            prediction_id: result.prediction_id,
            overall_class: result.recommendation.clone(),
            recommendation: result.recommendation,
            reason: result.reason,
            locations: result.locations,
        }
        .into_response(),
        Err(WebPortError::NoLocations) => ErrorTemplate {
            error: "No locations selected".into(),
            detail: Some("Please add at least one location.".into()),
        }
        .into_response(),
        Err(WebPortError::InvalidLatitude) => ErrorTemplate {
            error: "Invalid location".into(),
            detail: Some("Could not parse latitude.".into()),
        }
        .into_response(),
        Err(WebPortError::InvalidLongitude) => ErrorTemplate {
            error: "Invalid location".into(),
            detail: Some("Could not parse longitude.".into()),
        }
        .into_response(),
        Err(WebPortError::ServiceUnavailable(msg)) => {
            warn!(error = %msg, "API unreachable");
            ErrorTemplate {
                error: "Weather service unavailable".into(),
                detail: Some(msg),
            }
            .into_response()
        }
        Err(WebPortError::UpstreamError { error, detail }) => {
            warn!(error = %error, "API returned error");
            ErrorTemplate { error, detail }.into_response()
        }
        Err(WebPortError::GeocodingFailed(msg)) => {
            warn!(error = %msg, "geocoding failed");
            ErrorTemplate {
                error: "Geocoding failed".into(),
                detail: Some(msg),
            }
            .into_response()
        }
        Err(e) => {
            warn!(error = %e, "unexpected error in check");
            ErrorTemplate {
                error: "Unexpected error".into(),
                detail: None,
            }
            .into_response()
        }
    }
}

async fn email_handler<P: WebPort>(
    State(ctx): State<Arc<AppContext<P>>>,
    Form(form): Form<EmailForm>,
) -> impl IntoResponse {
    match ctx
        .service
        .register_email(&form.prediction_id, &form.email)
        .await
    {
        Ok(()) => EmailSuccessTemplate.into_response(),
        Err(e) => {
            warn!(error = %e, "email registration failed");
            ErrorTemplate {
                error: "Could not register email".into(),
                detail: Some(e.to_string()),
            }
            .into_response()
        }
    }
}

async fn feedback_page<P: WebPort>(
    State(ctx): State<Arc<AppContext<P>>>,
    axum::extract::Query(query): axum::extract::Query<FeedbackQuery>,
) -> impl IntoResponse {
    match ctx.service.get_prediction(&query.token).await {
        Ok(prediction) => FeedbackTemplate {
            token: query.token,
            recommendation: prediction.recommendation,
            reason: prediction.reason,
            options: RECOMMENDATION_OPTIONS,
        }
        .into_response(),
        Err(e) => {
            warn!(error = %e, "failed to load prediction for feedback");
            FeedbackExpiredTemplate.into_response()
        }
    }
}

async fn feedback_handler<P: WebPort>(
    State(ctx): State<Arc<AppContext<P>>>,
    Form(form): Form<FeedbackForm>,
) -> impl IntoResponse {
    match ctx
        .service
        .submit_feedback(
            &form.token,
            &form.brought,
            &form.should_have_brought,
            form.comment.as_deref(),
        )
        .await
    {
        Ok(()) => FeedbackThanksTemplate.into_response(),
        Err(e) => {
            warn!(error = %e, "feedback submission failed");
            FeedbackExpiredTemplate.into_response()
        }
    }
}

async fn unsubscribe_handler<P: WebPort>(
    State(ctx): State<Arc<AppContext<P>>>,
    Form(form): Form<UnsubscribeForm>,
) -> impl IntoResponse {
    match ctx.service.unsubscribe(&form.email).await {
        Ok(()) => UnsubscribeSuccessTemplate.into_response(),
        Err(e) => {
            warn!(error = %e, "unsubscribe failed");
            ErrorTemplate {
                error: "Could not process unsubscribe".into(),
                detail: Some(e.to_string()),
            }
            .into_response()
        }
    }
}

#[cfg(test)]
mod tests;
