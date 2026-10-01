mod adapters;
mod application;
mod domain;
mod ports;

use adapters::outbound::open_meteo::OpenMeteoClient;
use application::coat_check_service::CoatCheckService;

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

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
