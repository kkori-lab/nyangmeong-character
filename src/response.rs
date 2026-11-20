//! `app/core/response.py` : 백엔드 ApiResponse와 같은 응답 껍데기
//! `{timestamp, status, httpCode, message, path?, code?, data?}` 순서, None인 칸은 제외

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{Duration, Timelike, Utc};
use serde_json::{Map, Value};

/// 업무 오류. code = 백엔드 ErrorCode 상수 이름
#[derive(Clone, Debug)]
pub struct AppError {
    pub code: String,
    pub http_code: u16,
    pub message: String,
    /// 백엔드 ApiResponse.errorWithData와 동일 : 오류에도 data 동봉 (없으면 필드 제외)
    pub data: Option<Value>,
}

impl AppError {
    pub fn new(code: &str, http_code: u16, message: &str) -> Self {
        Self {
            code: code.into(),
            http_code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

pub fn success(data: Option<Value>, http_code: u16) -> Response {
    // 성공 응답은 path 없음
    build("success", http_code, "ok", None, None, data)
}

pub fn error(
    code: &str,
    http_code: u16,
    message: &str,
    path: &str,
    data: Option<Value>,
) -> Response {
    build("error", http_code, message, Some(path), Some(code), data)
}

fn build(
    status: &str,
    http_code: u16,
    message: &str,
    path: Option<&str>,
    code: Option<&str>,
    data: Option<Value>,
) -> Response {
    let mut body = Map::new();
    body.insert("timestamp".into(), Value::String(timestamp()));
    body.insert("status".into(), Value::String(status.into()));
    body.insert("httpCode".into(), Value::from(http_code));
    body.insert("message".into(), Value::String(message.into()));
    if let Some(path) = path {
        body.insert("path".into(), Value::String(path.into()));
    }
    if let Some(code) = code {
        body.insert("code".into(), Value::String(code.into()));
    }
    // Python은 None만 제외 : data가 JSON null이어도 제외
    if let Some(data) = data.filter(|d| !d.is_null()) {
        body.insert("data".into(), data);
    }
    json_response(http_code, &Value::Object(body))
}

/// FastAPI JSONResponse : 공백 없는 JSON, `application/json`
pub fn json_response(http_code: u16, body: &Value) -> Response {
    let bytes = serde_json::to_vec(body).expect("Value 직렬화는 실패하지 않음");
    let mut res = (StatusCode::from_u16(http_code).expect("상태 코드"), bytes).into_response();
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    res
}

/// Python `datetime.now().isoformat()` : 1번 서버 시간대(KST) 기준, 마이크로초가 0이면 소수부 없음
/// 2번 서버 시간대가 UTC여도 같게 나오도록 +9시간 고정(한국은 일광 절약 시간 없음)
pub fn timestamp() -> String {
    let now = (Utc::now() + Duration::hours(9)).naive_utc();
    let micros = now.nanosecond() / 1_000 % 1_000_000;
    let base = now.format("%Y-%m-%dT%H:%M:%S");
    if micros == 0 {
        base.to_string()
    } else {
        format!("{base}.{micros:06}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    async fn body_json(res: Response) -> Value {
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn error_keeps_order_and_skips_none() {
        let res = error("FORBIDDEN", 403, "접근 권한이 없습니다.", "/ai/x", None);
        assert_eq!(res.status(), 403);
        assert_eq!(res.headers()[header::CONTENT_TYPE], "application/json");
        let body = body_json(res).await;
        let keys: Vec<_> = body.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            ["timestamp", "status", "httpCode", "message", "path", "code"]
        );
        assert_eq!(body["status"], "error");
    }

    #[tokio::test]
    async fn success_has_no_path_and_drops_null_data() {
        let body = body_json(success(None, 200)).await;
        let keys: Vec<_> = body.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["timestamp", "status", "httpCode", "message"]);
        let body = body_json(success(Some(serde_json::json!({"a": null})), 202)).await;
        assert_eq!(body["data"], serde_json::json!({"a": null}));
        assert_eq!(body["httpCode"], 202);
    }

    #[test]
    fn timestamp_is_naive_iso() {
        let ts = timestamp();
        assert_eq!(&ts[10..11], "T");
        assert!(ts.len() == 19 || ts.len() == 26, "{ts}");
    }
}
