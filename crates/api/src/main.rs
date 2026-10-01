mod adapters;
mod application;
mod domain;
mod ports;

use adapters::outbound::open_meteo::OpenMeteoClient;
use application::coat_check_service::CoatCheckService;
use tokio::signal;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let weather_client = OpenMeteoClient::new();
    let service = CoatCheckService::new(weather_client);
    let app = adapters::inbound::http::router(service);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8080);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("coat-check listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect(&format!("failed to bind to {addr}"));
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
