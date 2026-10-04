mod adapters;
mod application;
mod domain;
mod ports;

use adapters::outbound::{email::SmtpEmailSender, open_meteo::OpenMeteoClient, postgres::PgStore};
use application::{coat_check_service::CoatCheckService, feedback_service::FeedbackService};
use sqlx::postgres::PgPoolOptions;
use tokio::signal;

use crate::{
    domain::{prediction::Prediction, recommendation::CoatDecision},
    ports::inbound::{FeedbackError, FeedbackPort},
};

/// No-op feedback implementation used when DATABASE_URL is not configured.
#[derive(Clone)]
struct NoopFeedback;

#[async_trait::async_trait]
impl FeedbackPort for NoopFeedback {
    async fn save_prediction(&self, _decision: &CoatDecision) -> Result<uuid::Uuid, FeedbackError> {
        Ok(uuid::Uuid::nil())
    }

    async fn register_contact(
        &self,
        _prediction_id: uuid::Uuid,
        _contact: &str,
    ) -> Result<(), FeedbackError> {
        Err(FeedbackError::Database(
            "feedback not configured".to_string(),
        ))
    }

    async fn get_prediction(&self, _id: uuid::Uuid) -> Result<Prediction, FeedbackError> {
        Err(FeedbackError::PredictionNotFound)
    }

    async fn submit_feedback(
        &self,
        _prediction_id: uuid::Uuid,
        _accurate: bool,
        _comment: Option<String>,
    ) -> Result<(), FeedbackError> {
        Err(FeedbackError::Database(
            "feedback not configured".to_string(),
        ))
    }
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let weather_client = OpenMeteoClient::new();
    let coat_check_service = CoatCheckService::new(weather_client.clone());

    let database_url = std::env::var("DATABASE_URL").ok();

    let app = if let Some(db_url) = database_url {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&db_url)
            .await
            .expect("failed to connect to Postgres");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("failed to run database migrations");

        let pg_store = PgStore::new(pool);
        let feedback_delay_minutes: i64 = std::env::var("FEEDBACK_DELAY_MINUTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(480); // default 8 hours
        let feedback_service = FeedbackService::new(
            pg_store.clone(),
            weather_client.clone(),
            feedback_delay_minutes,
        );

        // Background notification worker
        let base_url = std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3000".into());
        let smtp_host = std::env::var("SMTP_HOST").ok();
        let from_email =
            std::env::var("FROM_EMAIL").unwrap_or_else(|_| "noreply@coatcheck.app".into());

        if let Some(host) = smtp_host {
            match SmtpEmailSender::new(&host, &from_email, base_url) {
                Ok(sender) => {
                    tokio::spawn(application::notification_worker::run(pg_store, sender));
                    tracing::info!("notification worker started");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to configure SMTP, email worker disabled");
                }
            }
        } else {
            tracing::info!("SMTP_HOST not set, email worker disabled");
        }

        tracing::info!("feedback enabled (Postgres connected)");
        adapters::inbound::http::router(coat_check_service, feedback_service)
    } else {
        tracing::info!("DATABASE_URL not set, feedback disabled");
        adapters::inbound::http::router(coat_check_service, NoopFeedback)
    };

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8080);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("coat-check listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|_| panic!("failed to bind to {addr}"));
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

async fn shutdown_signal() {
    let ctrl_c = signal::ctrl_c();
    #[cfg(unix)]
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
        .expect("failed to register SIGTERM handler");
    #[cfg(unix)]
    tokio::select! {
        _ = ctrl_c => {}
        _ = sigterm.recv() => {}
    }
    #[cfg(not(unix))]
    ctrl_c.await.ok();
    tracing::info!("shutdown signal received, draining connections");
}
