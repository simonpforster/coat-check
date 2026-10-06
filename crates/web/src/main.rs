mod adapters;
mod application;
mod domain;
mod ports;

use adapters::{
    inbound::http::WebConfig,
    outbound::{coat_check_api::CoatCheckApiClient, geocoding::OpenMeteoGeocodingClient},
};
use application::web_service::WebService;
use tokio::signal;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let api_url = std::env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".into());
    let base_url = std::env::var("BASE_URL").ok();
    let ga_id = std::env::var("GA_ID").ok();

    let api_client = CoatCheckApiClient::new(api_url.clone());
    let geocoding_client = OpenMeteoGeocodingClient::new();
    let service = WebService::new(api_client.clone(), geocoding_client, api_client);
    let config = WebConfig { base_url, ga_id };
    let app = adapters::inbound::http::router(service, config);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8080);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("coat-check-web listening on {} (API: {})", addr, api_url);

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
