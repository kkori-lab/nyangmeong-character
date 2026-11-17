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
    // create_app 첫 줄 logs.configure : 다른 로그보다 먼저
    logs::configure(&settings.log_dir, Box::new(logs::local_today));
    // uvicorn.error처럼 처리 안 된 오류(패닉)도 파일에 남긴다 : 기본 출력 뒤 한 줄
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        let message = info.to_string().replace('\n', " ");
        logs::error("app.main", format!("panic: {message}"));
    }));
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
    logs::info("app.main", format!("listening on {addr}"));
    axum::serve(listener, app(AppState::new(settings)))
        .await
        .expect("server error");
}
