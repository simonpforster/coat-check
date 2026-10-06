mod adapters;
mod application;
mod domain;
mod ports;

use adapters::outbound::postgres::PgFeedbackStore;
use application::dashboard_service::DashboardService;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for backoffice");

    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect(&database_url)
        .await
        .expect("failed to connect to Postgres");

    let store = PgFeedbackStore::new(pool);
    let service = DashboardService::new(store);
    let app = adapters::inbound::http::router(std::sync::Arc::new(service));

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8080);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("backoffice listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|_| panic!("failed to bind to {addr}"));
    axum::serve(listener, app).await.expect("server error");
}
