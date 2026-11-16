use std::net::SocketAddr;

use axum::{Router, routing::get};

const DEFAULT_PORT: u16 = 8100;

fn app() -> Router {
    Router::new().route("/health", get(health))
}

async fn health() -> &'static str {
    "ok"
}

fn port() -> u16 {
    std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

#[tokio::main]
async fn main() {
    // Bind all interfaces: nginx on instance 1 reaches us via the private IP
    let addr = SocketAddr::from(([0, 0, 0, 0], port()));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind port");
    println!("listening on {addr}");
    axum::serve(listener, app()).await.expect("server error");
}
