use std::net::SocketAddr;

use nyangmeong_character::app::{AppState, app};
use nyangmeong_character::config::Settings;
use nyangmeong_character::logs;

const DEFAULT_PORT: u16 = 8100;

fn port() -> u16 {
    std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

#[tokio::main]
async fn main() {
    let settings = Settings::from_env();
    if settings.backend_base_url.is_empty() {
        logs::warning(
            "app.main",
            "BACKEND_BASE_URL이 비어 있어 /ai/** 가 502로 답한다",
        );
    }
    // Bind all interfaces: nginx on instance 1 reaches us via the private IP
    let addr = SocketAddr::from(([0, 0, 0, 0], port()));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind port");
    println!("listening on {addr}");
    axum::serve(listener, app(AppState::new(settings)))
        .await
        .expect("server error");
}
