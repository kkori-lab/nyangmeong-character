//! `app/core/backend.py` : Java 내부 API 호출
//! 4xx는 본문·`Set-Cookie` 그대로 전달(`BackendError`), 연결 실패·5xx·모양 다름은 502

use crate::error::ApiError;
use crate::logs;
use crate::pyjson;
use crate::response::AppError;
use axum::http::{HeaderMap, HeaderValue, Method, header};
use bytes::Bytes;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Java 내부 API 제한 시간(초)
pub const TIMEOUT_SEC: f64 = 3.0;
const LOGGER: &str = "app.core.backend";

/// Java 4xx 오류 : 본문·Set-Cookie 그대로 전달
#[derive(Clone, Debug)]
pub struct BackendError {
    pub status_code: u16,
    pub body: Bytes,
    pub content_type: Option<String>,
    pub set_cookies: Vec<HeaderValue>,
}

impl BackendError {
    pub fn of(res: &BackendResponse) -> Self {
        Self {
            status_code: res.status,
            body: res.body.clone(),
            content_type: res
                .headers
                .get(header::CONTENT_TYPE)
                .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned()),
            set_cookies: res
                .headers
                .get_all(header::SET_COOKIE)
                .iter()
                .cloned()
                .collect(),
        }
    }
}

/// 다 읽은 Java 응답 (httpx Response처럼 본문까지 받아 둠)
#[derive(Clone, Debug)]
pub struct BackendResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Bytes,
}

/// 요청끼리 공유. 쿠키 저장소를 두지 않음 : 다른 회원 요청에 섞임 방지
/// httpx 기본처럼 리다이렉트를 따라가지 않음. 제한 시간은 연결·읽기 각각에 적용
#[derive(Clone)]
pub struct Backend {
    base_url: String,
    clients: Arc<Mutex<HashMap<u64, reqwest::Client>>>,
}

impl Backend {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_owned(),
            clients: Arc::default(),
        }
    }

    fn client(&self, timeout_sec: f64) -> reqwest::Client {
        let mut clients = self.clients.lock().expect("clients 잠금");
        clients
            .entry(timeout_sec.to_bits())
            .or_insert_with(|| {
                let timeout = Duration::from_secs_f64(timeout_sec);
                reqwest::Client::builder()
                    .connect_timeout(timeout)
                    .read_timeout(timeout)
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .expect("HTTP 클라이언트 생성")
            })
            .clone()
    }

    /// 주소가 비었거나 꼴이 틀리거나 연결·읽기에 실패하면 이유 한 줄(로그용)
    pub async fn request(
        &self,
        method: Method,
        path: &str,
        timeout_sec: f64,
        build: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<BackendResponse, String> {
        let base_url = self.base_url.trim_end_matches('/');
        if base_url.is_empty() {
            return Err("주소 비어 있음".into());
        }
        let builder = self
            .client(timeout_sec)
            .request(method, format!("{base_url}{path}"));
        let res = build(builder).send().await.map_err(|e| reason(&e))?;
        let status = res.status().as_u16();
        let headers = res.headers().clone();
        let body = res.bytes().await.map_err(|e| reason(&e))?;
        Ok(BackendResponse {
            status,
            headers,
            body,
        })
    }

    /// `_send` : 4xx는 그대로 전달, 2xx가 아니면 502
    pub async fn send(
        &self,
        method: Method,
        path: &str,
        timeout_sec: f64,
        build: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<BackendResponse, ApiError> {
        let res = match self.request(method.clone(), path, timeout_sec, build).await {
            Ok(res) => res,
            Err(cause) => return Err(fail(&method, path, &cause).into()),
        };
        if (400..500).contains(&res.status) {
            return Err(BackendError::of(&res).into());
        }
        if !(200..300).contains(&res.status) {
            return Err(fail(&method, path, &format!("Java {}", res.status)).into());
        }
        Ok(res)
    }

    /// `call_backend` : ApiResponse의 data (본문이 비면 Null)
    pub async fn call(
        &self,
        method: Method,
        path: &str,
        timeout_sec: f64,
        build: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<Value, ApiError> {
        let res = self.send(method.clone(), path, timeout_sec, build).await?;
        if res.body.is_empty() {
            return Ok(Value::Null);
        }
        match pyjson::loads(&res.body) {
            Some(Value::Object(mut map)) => Ok(map.remove("data").unwrap_or(Value::Null)),
            _ => Err(fail(&method, path, "응답 JSON 아님").into()),
        }
    }

    /// `fetch_backend_bytes` : 파일 내려받기 (4xx 그대로 전달, 그 외 실패·빈 본문 502)
    pub async fn fetch_bytes(
        &self,
        path: &str,
        timeout_sec: f64,
        build: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<Bytes, ApiError> {
        let res = self.send(Method::GET, path, timeout_sec, build).await?;
        if res.body.is_empty() {
            return Err(fail(&Method::GET, path, "빈 본문").into());
        }
        Ok(res.body)
    }
}

/// `cause_reason` : 제한 시간·연결·주소 꼴·그 밖의 통신 실패 구분
pub fn reason(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        let kind = if error.is_connect() {
            "ConnectTimeout"
        } else {
            "ReadTimeout"
        };
        format!("제한 시간 초과 {kind}")
    } else if error.is_connect() {
        "연결 실패 ConnectError".into()
    } else if error.is_builder() {
        "주소 꼴 틀림 InvalidURL".into()
    } else {
        "통신 실패 RemoteProtocolError".into()
    }
}

/// 502로 바꾸며 이유 한 줄을 남김 : 주소 값·쿠키·본문·query·경로 숫자는 남기지 않음
fn fail(method: &Method, path: &str, reason: &str) -> AppError {
    logs::warning(
        LOGGER,
        format!("Java 호출 실패({reason}), 502 {method} {}", masked(path)),
    );
    external_error()
}

/// 경로의 숫자 구간(회원·반려동물 번호 등) -> {n}, query는 제외
pub fn masked(path: &str) -> String {
    let path = path.split('?').next().unwrap_or("");
    path.split('/')
        .map(|segment| {
            if !segment.is_empty() && segment.chars().all(|c| c.is_ascii_digit()) {
                "{n}"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub fn external_error() -> AppError {
    AppError::new(
        "EXTERNAL_API_ERROR",
        502,
        "외부 서비스 호출에 실패했습니다. 잠시 후 다시 시도해 주세요.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masked_hides_numbers_and_query() {
        assert_eq!(
            masked("/internal/ai/characters/12/item-sheets/3?x=1"),
            "/internal/ai/characters/{n}/item-sheets/{n}"
        );
        assert_eq!(
            masked("/internal/ai/items/a1/bake"),
            "/internal/ai/items/a1/bake"
        );
    }
}
