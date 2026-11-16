//! 제한 시간이 업로드 중에도 걸리는지 확인한다(받기를 멈추는 TCP 서버, 이 PC의 임시 포트).
//! 소켓 버퍼를 넘는 본문은 `backend` 테스트가 보내고(48MB), OpenAI 쪽은 그림을 줄여 올려 버퍼에 들어갈 수 있으므로
//! 멈춘 상대에서 시간 초과·재시도 1회로 끝나는 것만 본다.
//! httpx는 읽기·쓰기 한 번마다 180초(`httpx.Timeout(180)`)를 재서 업로드가 멈추면 `WriteTimeout` →
//! SDK `APITimeoutError` → 재시도 1회 → 실패한다. reqwest `read_timeout`은 요청을 보낼 때 한 번 맞춘 타이머라
//! 업로드와 응답 머리 대기를 합쳐 잰다(`docs/design/migration.md` "알려진 차이"). 멈춘 상대에서는 둘 다 시간 초과로 끝난다.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use image::ImageEncoder as _;
use nyangmeong_character::backend::Backend;
use nyangmeong_character::clients::{CharacterClient, OpenAiClient};
use nyangmeong_character::config::Settings;
use tokio::net::{TcpListener, TcpStream};

/// 연결만 받고 아무것도 읽지 않는다. 연결은 열어 둔다(닫으면 쓰기가 시간 초과가 아니라 오류로 끝남)
async fn stalled_server() -> (String, Arc<Mutex<Vec<TcpStream>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let held = Arc::new(Mutex::new(Vec::new()));
    let keep = held.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            keep.lock().unwrap().push(stream);
        }
    });
    (format!("http://{addr}"), held)
}

/// 소켓 버퍼보다 훨씬 큰 본문
fn big() -> Vec<u8> {
    vec![7u8; 48 * 1024 * 1024]
}

/// 압축이 안 되는 PNG : OpenAI 클라이언트는 올리기 전에 그림을 읽어 다시 만들므로 올바른 PNG여야 한다
fn noise_png() -> Vec<u8> {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let pixels: Vec<u8> = (0..1000 * 1000 * 4)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect();
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&pixels, 1000, 1000, image::ExtendedColorType::Rgba8)
        .unwrap();
    out
}

#[tokio::test]
async fn openai_upload_that_stalls_times_out_and_is_retried_once() {
    let (base, held) = stalled_server().await;
    let settings = Settings::from_lookup(|name| match name {
        "OPENAI_API_KEY" => Some("test-key".into()),
        "OPENAI_BASE_URL" => Some(format!("{base}/v1")),
        "CHARACTER_TIMEOUT_SEC" => Some("1".into()),
        _ => None,
    });
    let client = OpenAiClient::from_settings(&settings).unwrap();

    let started = Instant::now();
    let result = client.describe_item(&noise_png(), "MYPC0001", "모자").await;

    assert!(result.is_err());
    // 1초 제한 × 2번(첫 요청 + 재시도 1회) + 재시도 대기 0.5초 안팎 : 계속 기다렸다면 여기까지 오지 못한다
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(held.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn backend_upload_that_stalls_times_out_without_retry() {
    let (base, held) = stalled_server().await;
    let backend = Backend::new(&base);

    let started = Instant::now();
    let result = backend
        .request(reqwest::Method::POST, "/internal/x", 1.0, |builder| {
            builder.body(big())
        })
        .await;

    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(held.lock().unwrap().len(), 1);
}
