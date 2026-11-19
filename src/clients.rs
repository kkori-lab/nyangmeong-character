//! `app/character/clients.py` : OpenAI 호출 (눈 판별·시트 생성·아이템 굽기·굽기 지시 초안·픽셀 아이템 그림)
//!
//! Python은 `openai` SDK(3.42.1)를 쓴다. 여기는 같은 요청을 reqwest로 보낸다.
//! - SDK와 같게: 제한 시간 `CHARACTER_TIMEOUT_SEC`(연결·읽기 각각), `max_retries=1`(연결 실패·시간 초과·408·409·429·5xx를
//!   한 번 더, `x-should-retry`·`retry-after(-ms)` 따름, 기본 대기 0.375~0.5초)
//! - 차이: 큰 이미지 계산은 `heavy`(동시 수 `HEAVY_CONCURRENCY`)로 돌린다. Python은 `asyncio.to_thread` 기본 풀을 쓴다.
//!   결과는 같고 동시에 도는 수만 다르다(2번 서버는 RAM 1GB)

use std::future::Future;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use reqwest::multipart::{Form, Part};
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use crate::canvas::{self, Layout};
use crate::config::Settings;
use crate::engine::{self, BakeItem, Eyes};
use crate::logs;
use crate::pil;

const LOGGER: &str = "app.character.clients";
const IMAGE_SIZE: &str = "1024x1024";
const IMAGE_QUALITY: &str = "medium";
/// 큰 이미지 계산(1024 시트 분할·정규화 등)을 동시에 돌리는 수 : 2번 서버는 vCPU 2개·RAM 1GB, 한 번에 힙 약 25MB
pub const HEAVY_CONCURRENCY: usize = 2;
const MAX_RETRIES: u32 = 1;

static HEAVY: Semaphore = Semaphore::const_new(HEAVY_CONCURRENCY);

/// 큰 계산 : 동시 수를 제한하고 막는 일꾼 스레드에서 돌린다. 계산 중 패닉은 `Err`(Python이면 예외)
pub async fn heavy<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let _permit = HEAVY.acquire().await.expect("세마포어는 닫지 않음");
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|_| "계산 중 패닉".to_owned())
}

/// 외부 이미지 생성 API 오류
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharacterApiError;

impl std::fmt::Display for CharacterApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CharacterApiError")
    }
}

impl std::error::Error for CharacterApiError {}

#[derive(Clone, Debug)]
pub struct CharacterRequest {
    pub photo: Vec<u8>,
    pub pet_se_cd: String,
    pub vrty_nm: Option<String>,
    pub colr_nm: Option<String>,
    pub chrc_cn: Option<String>,
}

/// 512 결과와 굽기·합성에 다시 쓸 맨몸 시트·배치값. 눈을 못 찾으면 `image`가 빔(`NOT_PLACED`)
#[derive(Clone, Debug, Default)]
pub struct GeneratedCharacter {
    pub image: Vec<u8>,
    pub sheet: Vec<u8>,
    pub layout: Option<Layout>,
}

pub trait CharacterClient: Send + Sync {
    fn generate(
        &self,
        request: CharacterRequest,
    ) -> impl Future<Output = Result<GeneratedCharacter, CharacterApiError>> + Send;

    fn bake(
        &self,
        sheet: &[u8],
        pet_se_cd: &str,
        item: &BakeItem,
        item_image: Option<&[u8]>,
    ) -> impl Future<Output = Result<Vec<u8>, CharacterApiError>> + Send;

    fn describe_item(
        &self,
        image: &[u8],
        regn_cd: &str,
        item_nm: &str,
    ) -> impl Future<Output = Result<String, CharacterApiError>> + Send;

    fn draw_item(
        &self,
        image: &[u8],
        regn_cd: &str,
        item_nm: &str,
    ) -> impl Future<Output = Result<Vec<u8>, CharacterApiError>> + Send;
}

/// SDK 예외 이름(로그용) : `type(exc).__name__`
fn error_name(error: &SdkError) -> &'static str {
    match error {
        SdkError::Timeout => "APITimeoutError",
        SdkError::Connection => "APIConnectionError",
        SdkError::Status(400) => "BadRequestError",
        SdkError::Status(401) => "AuthenticationError",
        SdkError::Status(403) => "PermissionDeniedError",
        SdkError::Status(404) => "NotFoundError",
        SdkError::Status(409) => "ConflictError",
        SdkError::Status(422) => "UnprocessableEntityError",
        SdkError::Status(429) => "RateLimitError",
        SdkError::Status(s) if *s >= 500 => "InternalServerError",
        SdkError::Status(_) => "APIStatusError",
        SdkError::Shape(name) => name,
    }
}

#[derive(Debug)]
enum SdkError {
    Timeout,
    Connection,
    Status(u16),
    /// 응답은 받았지만 읽을 수 없음(JSON 아님·칸 없음) 또는 그 밖의 예외 이름
    Shape(&'static str),
}

pub struct OpenAiClient {
    http: reqwest::Client,
    base_url: String,
    key: String,
    image_model: String,
    vision_model: String,
}

impl OpenAiClient {
    /// 키가 비면 `None` : 업로드 검증 뒤 502 (`get_character_client`)
    pub fn from_settings(settings: &Settings) -> Option<Self> {
        if settings.openai_api_key.is_empty() {
            return None;
        }
        let timeout = Duration::from_secs_f64(settings.character_timeout_sec);
        let http = reqwest::Client::builder()
            .connect_timeout(timeout)
            .read_timeout(timeout)
            .build()
            .expect("HTTP 클라이언트 생성");
        Some(Self {
            http,
            base_url: settings.openai_base_url.trim_end_matches('/').to_owned(),
            key: settings.openai_api_key.clone(),
            image_model: settings.character_image_model.clone(),
            vision_model: settings.character_vision_model.clone(),
        })
    }

    /// SDK `_request` : 같은 요청을 `MAX_RETRIES`번 더 보낼 수 있다
    async fn post(
        &self,
        path: &str,
        build: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<Value, SdkError> {
        let url = format!("{}{path}", self.base_url);
        let mut retries_left = MAX_RETRIES;
        loop {
            let request = build(self.http.post(&url).bearer_auth(&self.key));
            let (error, retry_after) = match request.send().await {
                Ok(res) => {
                    let status = res.status().as_u16();
                    let headers = res.headers().clone();
                    if (200..300).contains(&status) {
                        let body = res.bytes().await.map_err(|e| transport_error(&e))?;
                        return serde_json::from_slice(&body)
                            .map_err(|_| SdkError::Shape("JSONDecodeError"));
                    }
                    let retry = match headers.get("x-should-retry").and_then(|v| v.to_str().ok()) {
                        Some("true") => true,
                        Some("false") => false,
                        _ => matches!(status, 408 | 409 | 429) || status >= 500,
                    };
                    let error = SdkError::Status(status);
                    if !retry || retries_left == 0 {
                        if status == 429 {
                            log_quota_code(res).await;
                        }
                        return Err(error);
                    }
                    (error, retry_after(&headers))
                }
                Err(e) => (transport_error(&e), None),
            };
            if retries_left == 0 {
                return Err(error);
            }
            let used = MAX_RETRIES - retries_left;
            retries_left -= 1;
            tokio::time::sleep(retry_delay(retry_after, used)).await;
        }
    }

    /// `images.edit` : 그림들을 `image[]`로, 순서 그대로 (첫째가 화풍 기준, 마지막이 사진 등)
    async fn images_edit(
        &self,
        prompt: &str,
        images: &[(&'static str, Vec<u8>)],
    ) -> Result<Vec<u8>, SdkError> {
        let value = self
            .post("/images/edits", |b| {
                // 보내는 순서도 SDK와 같게 : prompt, model, n, quality, size, 그림들
                let mut form = Form::new()
                    .text("prompt", prompt.to_owned())
                    .text("model", self.image_model.clone())
                    .text("n", "1")
                    .text("quality", IMAGE_QUALITY)
                    .text("size", IMAGE_SIZE);
                for (name, content) in images {
                    let part = Part::bytes(content.clone())
                        .file_name(*name)
                        .mime_str("image/png")
                        .expect("image/png");
                    form = form.part("image[]", part);
                }
                b.multipart(form)
            })
            .await?;
        // base64.b64decode(response.data[0].b64_json)
        let first = match value.get("data") {
            Some(Value::Array(items)) => items.first().ok_or(SdkError::Shape("IndexError"))?,
            _ => return Err(SdkError::Shape("TypeError")),
        };
        let encoded = first
            .get("b64_json")
            .and_then(Value::as_str)
            .ok_or(SdkError::Shape("TypeError"))?;
        let cleaned: String = encoded
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
            .collect();
        STANDARD
            .decode(cleaned)
            .map_err(|_| SdkError::Shape("Error"))
    }

    /// `chat.completions.create` : 글 + 그림 한 장 -> 답 글
    async fn vision(&self, question: &str, png: &[u8]) -> Result<String, SdkError> {
        let uri = format!("data:image/png;base64,{}", STANDARD.encode(png));
        let body = json!({
            "model": self.vision_model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": question},
                    {"type": "image_url", "image_url": {"url": uri}},
                ],
            }],
        });
        let value = self.post("/chat/completions", |b| b.json(&body)).await?;
        let message = value
            .pointer("/choices/0/message")
            .ok_or(SdkError::Shape("IndexError"))?;
        Ok(message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned())
    }

    /// `_detect_eyes` : 눈 판별 때문에 생성을 실패시키지 않음
    async fn detect_eyes(&self, photo: &[u8], species: &str) -> Eyes {
        match self.vision(&engine::eye_question(species), photo).await {
            Ok(answer) => engine::parse_eyes(&answer),
            Err(error) => {
                logs::warning(
                    LOGGER,
                    format!("눈 판별 실패, 기본값 사용: {}", error_name(&error)),
                );
                Eyes::default()
            }
        }
    }

    async fn render(&self, request: &CharacterRequest) -> Result<Rendered, &'static str> {
        let species = engine::species_of(&request.pet_se_cd).ok_or("KeyError")?;
        let raw = request.photo.clone();
        let photo = heavy(move || engine::normalize_photo(&raw))
            .await
            .map_err(|_| "RuntimeError")?
            .map_err(|e| engine::error_name(&e))?;
        let eyes = self.detect_eyes(&photo, species).await;
        let prompt = engine::build_prompt(
            species,
            &eyes,
            request.vrty_nm.as_deref(),
            request.colr_nm.as_deref(),
            request.chrc_cn.as_deref(),
        );
        // 순서 고정 : 프롬프트가 첫 그림을 화풍 기준, 마지막 그림을 사진으로 지칭
        let sheet = self
            .images_edit(
                &prompt,
                &[
                    ("style_ref.png", engine::STYLE_REF.to_vec()),
                    ("photo.png", photo),
                ],
            )
            .await
            .map_err(|e| error_name(&e))?;
        let for_front = sheet.clone();
        let placement = heavy(move || {
            let front = engine::stand_front(&for_front).map_err(|e| engine::error_name(&e))?;
            Ok::<_, &'static str>(canvas::place(&front))
        })
        .await
        .map_err(|_| "RuntimeError")??;
        Ok(Rendered { sheet, placement })
    }
}

struct Rendered {
    sheet: Vec<u8>,
    placement: Option<canvas::Placement>,
}

/// 429 본문 `{"error": {"code": ...}}`의 `code` (예: `insufficient_quota`).
/// 1~64자 ASCII 영숫자·`_`·`-`·`.`만 받는다. 본문의 나머지는 읽지 않는다
fn quota_code(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let code = value.get("error")?.get("code")?.as_str()?;
    let ok = (1..=64).contains(&code.len())
        && code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
    ok.then(|| code.to_owned())
}

/// 원본에 없는 운영 로그 한 줄(크레딧 부족 알림). 응답·Java 보고는 그대로 (`migration.md` "알려진 차이")
async fn log_quota_code(res: reqwest::Response) {
    let Ok(body) = res.bytes().await else {
        return;
    };
    if let Some(code) = quota_code(&body) {
        logs::warning(LOGGER, format!("OpenAI 429 사유: {code}"));
    }
}

fn transport_error(error: &reqwest::Error) -> SdkError {
    if error.is_timeout() {
        SdkError::Timeout
    } else {
        SdkError::Connection
    }
}

/// `retry-after-ms` 또는 `retry-after`(초) : 0 초과 60 이하일 때만 따름
fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<f64> {
    let seconds = if let Some(ms) = headers
        .get("retry-after-ms")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<f64>().ok())
    {
        ms / 1000.0
    } else {
        headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<f64>().ok())?
    };
    (seconds > 0.0 && seconds <= 60.0).then_some(seconds)
}

/// SDK 기본 대기 : `min(0.5 * 2**지금까지 다시 한 수, 8)` × (1 - 0.25 × 난수)
fn retry_delay(retry_after: Option<f64>, used: u32) -> Duration {
    if let Some(seconds) = retry_after {
        return Duration::from_secs_f64(seconds);
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let jitter = 1.0 - 0.25 * f64::from(nanos % 1_000_000) / 1_000_000.0;
    Duration::from_secs_f64((0.5 * 2f64.powi(used as i32)).min(8.0) * jitter)
}

impl CharacterClient for OpenAiClient {
    /// FN-MYP-010-08·17 : 눈 판별 -> 2×2 시트 1회 생성 -> 서기 정면 512 배치
    async fn generate(
        &self,
        request: CharacterRequest,
    ) -> Result<GeneratedCharacter, CharacterApiError> {
        let rendered = match self.render(&request).await {
            Ok(rendered) => rendered,
            Err(name) => {
                // 예외 본문 제외 : 요청 번호·내부 경로 포함 가능
                logs::error(LOGGER, format!("캐릭터 생성 실패: {name}"));
                return Err(CharacterApiError);
            }
        };
        let Some(placement) = rendered.placement else {
            logs::warning(LOGGER, "캐릭터 눈을 찾지 못함: 규격 실패로 보고");
            return Ok(GeneratedCharacter::default());
        };
        if placement.clipped {
            logs::warning(
                LOGGER,
                format!(
                    "캐릭터가 512 캔버스 밖으로 잘림: 배율 {:.3}",
                    placement.scale
                ),
            );
        }
        let layout = placement.layout();
        let image = heavy(move || pil::encode_png(&placement.image))
            .await
            .map_err(|_| CharacterApiError)?;
        Ok(GeneratedCharacter {
            image,
            sheet: rendered.sheet,
            layout: Some(layout),
        })
    }

    async fn bake(
        &self,
        sheet: &[u8],
        pet_se_cd: &str,
        item: &BakeItem,
        item_image: Option<&[u8]>,
    ) -> Result<Vec<u8>, CharacterApiError> {
        let attempt = async {
            let species = engine::species_of(pet_se_cd).ok_or("KeyError")?;
            let prompt = engine::build_wear_prompt(species, item, item_image.is_some());
            let mut images = vec![("character.png", sheet.to_vec())];
            if let Some(item_image) = item_image {
                // 그림 순서 : 맨몸 시트가 첫째, 아이템 그림이 마지막 (프롬프트 머리말과 짝)
                let raw = item_image.to_vec();
                let reference = heavy(move || engine::normalize_item_image(&raw))
                    .await
                    .map_err(|_| "RuntimeError")?
                    .map_err(|e| engine::error_name(&e))?;
                images.push(("item.png", reference));
            }
            self.images_edit(&prompt, &images)
                .await
                .map_err(|e| error_name(&e))
        };
        attempt.await.map_err(|name| {
            logs::error(
                LOGGER,
                format!("아이템 굽기 실패: itemSn={}, {name}", item.item_sn),
            );
            CharacterApiError
        })
    }

    /// FN-ADM-070-02 : 아이템 그림 -> 굽기 지시 한 문장
    async fn describe_item(
        &self,
        image: &[u8],
        regn_cd: &str,
        item_nm: &str,
    ) -> Result<String, CharacterApiError> {
        let attempt = async {
            let raw = image.to_vec();
            let png = heavy(move || engine::normalize_item_image(&raw))
                .await
                .map_err(|_| "RuntimeError")?
                .map_err(|e| engine::error_name(&e))?;
            let answer = self
                .vision(&engine::item_hint_question(regn_cd, item_nm), &png)
                .await
                .map_err(|e| error_name(&e))?;
            Ok::<_, &'static str>(engine::parse_bake_hint(&answer))
        };
        match attempt.await {
            Err(name) => {
                logs::error(LOGGER, format!("굽기 지시 초안 실패: {name}"));
                Err(CharacterApiError)
            }
            Ok(None) => {
                logs::error(LOGGER, "굽기 지시 초안 실패: 빈 답 또는 길이 초과");
                Err(CharacterApiError)
            }
            Ok(Some(hint)) => Ok(hint),
        }
    }

    /// FN-ADM-070-02 : 실사 아이템 사진 -> 단색 배경 픽셀 아이템 그림 (배경 떼기는 service)
    async fn draw_item(
        &self,
        image: &[u8],
        regn_cd: &str,
        item_nm: &str,
    ) -> Result<Vec<u8>, CharacterApiError> {
        let attempt = async {
            let raw = image.to_vec();
            let reference = heavy(move || engine::normalize_item_image(&raw))
                .await
                .map_err(|_| "RuntimeError")?
                .map_err(|e| engine::error_name(&e))?;
            // 순서 고정 : 프롬프트가 첫 그림을 화풍 기준, 마지막 그림을 사진으로 지칭
            self.images_edit(
                &engine::item_pixel_prompt(regn_cd, item_nm),
                &[
                    ("style_ref.png", engine::STYLE_REF.to_vec()),
                    ("item.png", reference),
                ],
            )
            .await
            .map_err(|e| error_name(&e))
        };
        attempt.await.map_err(|name| {
            logs::error(LOGGER, format!("픽셀 아이템 그림 생성 실패: {name}"));
            CharacterApiError
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    #[test]
    fn retry_after_follows_the_sdk_rules() {
        let headers = |pairs: &[(&'static str, &'static str)]| {
            let mut map = HeaderMap::new();
            for (name, value) in pairs {
                map.insert(*name, HeaderValue::from_static(value));
            }
            map
        };
        // retry-after-ms가 먼저, 0 초과 60 이하만 따른다
        assert_eq!(
            retry_after(&headers(&[("retry-after-ms", "250")])),
            Some(0.25)
        );
        assert_eq!(retry_after(&headers(&[("retry-after", "3")])), Some(3.0));
        assert_eq!(
            retry_after(&headers(&[("retry-after-ms", "500"), ("retry-after", "9")])),
            Some(0.5)
        );
        assert_eq!(retry_after(&headers(&[("retry-after", "61")])), None);
        assert_eq!(retry_after(&headers(&[("retry-after", "0")])), None);
        assert_eq!(retry_after(&headers(&[("retry-after", "soon")])), None);
        assert_eq!(retry_after(&headers(&[])), None);
    }

    #[test]
    fn retry_delay_is_between_three_quarters_and_the_full_backoff() {
        assert_eq!(retry_delay(Some(2.0), 0), Duration::from_secs(2));
        for _ in 0..20 {
            let first = retry_delay(None, 0).as_secs_f64();
            assert!((0.375..=0.5).contains(&first), "{first}");
            let capped = retry_delay(None, 9).as_secs_f64();
            assert!((6.0..=8.0).contains(&capped), "{capped}");
        }
    }

    #[test]
    fn quota_code_reads_only_a_safe_error_code() {
        let code = |body: &str| quota_code(body.as_bytes());
        assert_eq!(
            code(
                r#"{"error": {"message": "You exceeded your quota", "type": "insufficient_quota", "code": "insufficient_quota"}}"#
            ),
            Some("insufficient_quota".into())
        );
        assert_eq!(
            code(r#"{"error": {"code": "rate_limit_exceeded"}}"#),
            Some("rate_limit_exceeded".into())
        );
        assert_eq!(code(r#"{"error": {"code": null}}"#), None);
        assert_eq!(code(r#"{"error": {"code": 429}}"#), None);
        assert_eq!(code(r#"{"error": {"code": ""}}"#), None);
        assert_eq!(
            code(&format!(r#"{{"error": {{"code": "{}"}}}}"#, "a".repeat(65))),
            None
        );
        assert_eq!(
            code(&format!(r#"{{"error": {{"code": "{}"}}}}"#, "a".repeat(64))),
            Some("a".repeat(64))
        );
        assert_eq!(code(r#"{"error": {"code": "bad\ncode"}}"#), None);
        assert_eq!(code(r#"{"error": {"code": "a b"}}"#), None);
        assert_eq!(code(r#"{"code": "insufficient_quota"}"#), None);
        assert_eq!(code("Too Many Requests"), None);
        assert_eq!(code(""), None);
    }

    #[test]
    fn no_key_means_no_client() {
        let settings = Settings::from_lookup(|_| None);
        assert!(OpenAiClient::from_settings(&settings).is_none());
        let settings = Settings::from_lookup(|name| (name == "OPENAI_API_KEY").then(|| "k".into()));
        assert!(OpenAiClient::from_settings(&settings).is_some());
    }
}
