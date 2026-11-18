//! `clients.py` OpenAiClient : `generate`(눈 판별 → 시트 생성 → 서기 정면 배치)·그림 참고 굽기·시간 초과 재시도.
//! 가짜 OpenAI를 이 PC의 임시 포트에 띄운다(실제 OpenAI는 부르지 않는다). 요청 모양의 Python 대조는 `contract/`가 맡는다.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::Response;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use nyangmeong_character::clients::{CharacterClient, CharacterRequest, OpenAiClient};
use nyangmeong_character::config::Settings;
use nyangmeong_character::engine::{self, BakeItem};
use serde_json::{Value, json};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/image");

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}/{name}")).unwrap()
}

#[derive(Clone, Debug)]
struct Seen {
    path: String,
    body: Vec<u8>,
    content_type: String,
}

type Step = (String, u16, Value, u64);

#[derive(Clone, Default)]
struct Fake {
    seen: Arc<Mutex<Vec<Seen>>>,
    /// 경로 끝(`/chat/completions`·`/images/edits`)별 답 순서: (상태, 본문, 지연 ms)
    script: Arc<Mutex<Vec<Step>>>,
}

async fn serve(State(fake): State<Fake>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let body = axum::body::to_bytes(body, usize::MAX)
        .await
        .unwrap()
        .to_vec();
    let path = parts.uri.path().to_owned();
    fake.seen.lock().unwrap().push(Seen {
        path: path.clone(),
        body,
        content_type: parts
            .headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned(),
    });
    let step = {
        let mut script = fake.script.lock().unwrap();
        let at = script
            .iter()
            .position(|(suffix, ..)| path.ends_with(suffix.as_str()));
        at.map(|at| script.remove(at))
    };
    let (status, payload, delay) = match step {
        Some((_, status, payload, delay)) => (status, payload, delay),
        None => (500, json!({"error": "스크립트 없음"}), 0),
    };
    tokio::time::sleep(Duration::from_millis(delay)).await;
    Response::builder()
        .status(StatusCode::from_u16(status).unwrap())
        .header("content-type", "application/json")
        .body(axum::body::Body::from(payload.to_string()))
        .unwrap()
}

async fn start(timeout_sec: &str) -> (OpenAiClient, Fake) {
    let fake = Fake::default();
    let app = Router::new().fallback(serve).with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let timeout = timeout_sec.to_owned();
    let settings = Settings::from_lookup(|name| match name {
        "OPENAI_API_KEY" => Some("test-key".into()),
        "OPENAI_BASE_URL" => Some(base.clone()),
        "CHARACTER_TIMEOUT_SEC" => Some(timeout.clone()),
        _ => None,
    });
    (OpenAiClient::from_settings(&settings).unwrap(), fake)
}

fn push(fake: &Fake, suffix: &str, status: u16, payload: Value, delay_ms: u64) {
    fake.script
        .lock()
        .unwrap()
        .push((suffix.to_owned(), status, payload, delay_ms));
}

fn image_reply(png: &[u8]) -> Value {
    json!({"data": [{"b64_json": STANDARD.encode(png)}]})
}

fn chat_reply(content: &str) -> Value {
    json!({"choices": [{"message": {"content": content}}]})
}

/// multipart 본문에서 (이름, 파일 이름) 순서와 글 칸 값
fn parts(seen: &Seen) -> Vec<(String, Option<String>, String)> {
    let boundary = seen
        .content_type
        .split("boundary=")
        .nth(1)
        .unwrap()
        .trim_matches('"');
    let text = String::from_utf8_lossy(&seen.body).into_owned();
    text.split(&format!("--{boundary}"))
        .filter(|chunk| chunk.contains("Content-Disposition"))
        .map(|chunk| {
            let head = chunk.split("\r\n\r\n").next().unwrap();
            let name = head
                .split("name=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            let file = head
                .split("filename=\"")
                .nth(1)
                .map(|f| f.split('"').next().unwrap().to_owned());
            let value = chunk
                .split("\r\n\r\n")
                .nth(1)
                .unwrap_or("")
                .strip_suffix("\r\n")
                .unwrap_or("")
                .to_owned();
            (name.to_owned(), file, value)
        })
        .collect()
}

#[tokio::test]
async fn generate_detects_eyes_then_draws_the_sheet_and_places_the_front() {
    let (client, fake) = start("5").await;
    push(
        &fake,
        "/chat/completions",
        200,
        chat_reply(r#"{"shape": "almond", "color": "green"}"#),
        0,
    );
    push(
        &fake,
        "/images/edits",
        200,
        image_reply(&fixture("sheets/plain/bare.png")),
        0,
    );
    let request = CharacterRequest {
        photo: fixture("photos/exif3_900x500.png"),
        pet_se_cd: "PETC0002".into(),
        vrty_nm: Some("코숏".into()),
        colr_nm: None,
        chrc_cn: None,
    };

    let made = client.generate(request).await.unwrap();

    // 눈 판별 → 시트 생성 순서
    let seen = fake.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2);
    assert!(seen[0].path.ends_with("/chat/completions"));
    let chat: Value = serde_json::from_slice(&seen[0].body).unwrap();
    assert_eq!(
        chat["messages"][0]["content"][0]["text"],
        engine::eye_question("cat")
    );
    // 시트 생성 요청: 화풍 그림이 첫째, 사진이 마지막, 프롬프트는 눈 판독 결과 + 등록 정보
    let sent = parts(&seen[1]);
    let images: Vec<_> = sent
        .iter()
        .filter(|p| p.0 == "image[]")
        .map(|p| p.1.clone().unwrap())
        .collect();
    assert_eq!(images, ["style_ref.png", "photo.png"]);
    let eyes = engine::Eyes {
        shape: "almond".into(),
        color: Some("green".into()),
    };
    let prompt = engine::build_prompt("cat", &eyes, Some("코숏"), None, None);
    assert!(sent.iter().any(|p| p.0 == "prompt" && p.2 == prompt));
    // 결과: 512 PNG와 맨몸 시트·배치값
    assert_eq!(&made.sheet, &fixture("sheets/plain/bare.png"));
    assert!(made.layout.is_some());
    let png = nyangmeong_character::pil::open_rgba(&made.image).unwrap();
    assert_eq!((png.width, png.height), (512, 512));
}

#[tokio::test]
async fn generate_uses_default_eyes_when_the_vision_call_fails() {
    let (client, fake) = start("5").await;
    push(&fake, "/chat/completions", 400, json!({"error": "x"}), 0);
    push(
        &fake,
        "/images/edits",
        200,
        image_reply(&fixture("sheets/plain/bare.png")),
        0,
    );
    let request = CharacterRequest {
        photo: fixture("photos/exif3_900x500.png"),
        pet_se_cd: "PETC0001".into(),
        vrty_nm: None,
        colr_nm: None,
        chrc_cn: None,
    };

    client.generate(request).await.unwrap();

    let seen = fake.seen.lock().unwrap().clone();
    let prompt = engine::build_prompt("dog", &engine::Eyes::default(), None, None, None);
    assert!(
        parts(&seen[1])
            .iter()
            .any(|p| p.0 == "prompt" && p.2 == prompt)
    );
}

#[tokio::test]
async fn generate_without_eyes_in_the_sheet_returns_an_empty_image() {
    let (client, fake) = start("5").await;
    push(&fake, "/chat/completions", 200, chat_reply("{}"), 0);
    push(
        &fake,
        "/images/edits",
        200,
        image_reply(&fixture("sheets/noeyes/bare.png")),
        0,
    );
    let request = CharacterRequest {
        photo: fixture("photos/exif3_900x500.png"),
        pet_se_cd: "PETC0001".into(),
        vrty_nm: None,
        colr_nm: None,
        chrc_cn: None,
    };

    let made = client.generate(request).await.unwrap();

    assert!(made.image.is_empty());
}

#[tokio::test]
async fn bake_with_an_item_image_sends_the_sheet_first_and_the_item_last() {
    let (client, fake) = start("5").await;
    push(&fake, "/images/edits", 200, image_reply(b"WORN"), 0);
    let item = BakeItem {
        item_sn: 5,
        regn_cd: "MYPC0003".into(),
        bake_hint: None,
        by_image: true,
    };

    let worn = client
        .bake(
            &fixture("sheets/plain/bare.png"),
            "PETC0001",
            &item,
            Some(&fixture("items/hat.png")),
        )
        .await
        .unwrap();

    assert_eq!(worn, b"WORN");
    let seen = fake.seen.lock().unwrap().clone();
    let sent = parts(&seen[0]);
    let images: Vec<_> = sent
        .iter()
        .filter(|p| p.0 == "image[]")
        .map(|p| p.1.clone().unwrap())
        .collect();
    assert_eq!(images, ["character.png", "item.png"]);
    let prompt = engine::build_wear_prompt("dog", &item, true);
    assert!(sent.iter().any(|p| p.0 == "prompt" && p.2 == prompt));
}

#[tokio::test]
async fn bake_with_an_unknown_species_fails_without_calling_openai() {
    let (client, fake) = start("5").await;
    let item = BakeItem {
        item_sn: 5,
        regn_cd: "MYPC0002".into(),
        bake_hint: Some("h".into()),
        by_image: false,
    };

    let result = client.bake(b"S", "PETC9999", &item, None).await;

    assert!(result.is_err());
    assert!(fake.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn timeout_is_retried_once_then_fails() {
    // 읽기 제한 0.3초, 답은 1초 뒤 : 두 번 다 시간 초과 → 요청 2건 뒤 실패
    let (client, fake) = start("0.3").await;
    push(&fake, "/chat/completions", 200, chat_reply("a cap"), 1000);
    push(&fake, "/chat/completions", 200, chat_reply("a cap"), 1000);

    let result = client
        .describe_item(&fixture("items/hat.png"), "MYPC0001", "모자")
        .await;

    assert!(result.is_err());
    assert_eq!(fake.seen.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn timeout_then_success_returns_the_second_answer() {
    let (client, fake) = start("0.3").await;
    push(&fake, "/chat/completions", 200, chat_reply("slow"), 1000);
    push(&fake, "/chat/completions", 200, chat_reply("- a cap."), 0);

    let hint = client
        .describe_item(&fixture("items/hat.png"), "MYPC0001", "모자")
        .await
        .unwrap();

    assert_eq!(hint, "a cap");
    assert_eq!(fake.seen.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn insufficient_quota_429_is_retried_once_then_fails() {
    // 크레딧 부족 429 : 재시도는 그대로(요청 2건), 결과는 같은 실패. 사유는 운영 로그에만
    let (client, fake) = start("5").await;
    let quota = json!({"error": {"message": "You exceeded your current quota", "type": "insufficient_quota", "code": "insufficient_quota"}});
    push(&fake, "/chat/completions", 429, quota.clone(), 0);
    push(&fake, "/chat/completions", 429, quota, 0);

    let result = client
        .describe_item(&fixture("items/hat.png"), "MYPC0001", "모자")
        .await;

    assert!(result.is_err());
    assert_eq!(fake.seen.lock().unwrap().len(), 2);
}
