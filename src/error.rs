//! `app/core/errors.py` + FastAPI·Starlette 기본 오류 응답
//!
//! 핸들러는 경로를 모른 채 `ApiError`를 돌려주고, `render_errors` 미들웨어가
//! 요청 경로(`request.url.path`, 퍼센트 디코딩)를 넣어 최종 응답을 만듦

use crate::backend::BackendError;
use crate::pyjson;
use crate::response::{self, AppError};
use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

pub const VALIDATION_MESSAGE: &str = "요청 값이 올바르지 않습니다.";

#[derive(Clone, Debug)]
pub enum ApiError {
    /// `AppError` 처리기 : ApiResponse 오류 모양
    App(Box<AppError>),
    /// `BackendError` 처리기 : Java 4xx 본문·`Set-Cookie` 그대로 (본문 path만 바꿈)
    Backend(Box<BackendError>),
    /// `RequestValidationError` 처리기 : 400 INVALID_REQUEST (필드 상세 미반환)
    Validation,
    /// Starlette `HTTPException` → FastAPI 기본 처리기 `{"detail": ...}`
    Http {
        status: u16,
        detail: String,
        allow: Option<String>,
    },
    /// 처리하지 못한 예외 → Starlette 500 `Internal Server Error`
    Internal,
}

impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        Self::App(Box::new(error))
    }
}

impl From<BackendError> for ApiError {
    fn from(error: BackendError) -> Self {
        Self::Backend(Box::new(error))
    }
}

#[derive(Clone)]
struct Pending(ApiError);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut res = Response::new(Body::empty());
        res.extensions_mut().insert(Pending(self));
        res
    }
}

/// 미들웨어 : 핸들러가 남긴 `ApiError`를 요청 경로와 함께 응답으로 바꿈
pub async fn render_errors(request: Request, next: Next) -> Response {
    let path = decoded_path(request.uri().path());
    let res = next.run(request).await;
    match res.extensions().get::<Pending>() {
        Some(Pending(error)) => render(error.clone(), &path),
        None => res,
    }
}

/// uvicorn : `unquote(raw_path)` (UTF-8, 깨진 바이트는 대체 문자)
pub fn decoded_path(raw: &str) -> String {
    percent_encoding::percent_decode_str(raw)
        .decode_utf8_lossy()
        .into_owned()
}

pub fn render(error: ApiError, path: &str) -> Response {
    match error {
        ApiError::App(e) => response::error(&e.code, e.http_code, &e.message, path, e.data),
        ApiError::Backend(e) => backend_response(*e, path),
        ApiError::Validation => {
            response::error("INVALID_REQUEST", 400, VALIDATION_MESSAGE, path, None)
        }
        ApiError::Http {
            status,
            detail,
            allow,
        } => {
            let mut res = response::json_response(status, &json!({ "detail": detail }));
            if let Some(allow) = allow {
                res.headers_mut()
                    .insert(header::ALLOW, HeaderValue::from_str(&allow).expect("Allow"));
            }
            res
        }
        ApiError::Internal => {
            let mut res =
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response();
            res.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            );
            res
        }
    }
}

fn backend_response(error: BackendError, path: &str) -> Response {
    let body = hide_internal_path(&error.body, path);
    let status = StatusCode::from_u16(error.status_code).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut res = (status, body).into_response();
    let headers = res.headers_mut();
    headers.remove(header::CONTENT_TYPE);
    // Starlette Response(media_type=...) : text/*에 charset이 없으면 `; charset=utf-8`을 붙임
    if let Some(content_type) = error.content_type.as_deref() {
        let mut content_type = content_type.to_owned();
        if content_type.starts_with("text/") && !content_type.to_lowercase().contains("charset=") {
            content_type.push_str("; charset=utf-8");
        }
        if let Ok(value) = HeaderValue::from_str(&content_type) {
            headers.insert(header::CONTENT_TYPE, value);
        }
    }
    for cookie in &error.set_cookies {
        headers.append(header::SET_COOKIE, cookie.clone());
    }
    res
}

/// Java 오류 본문의 path(/internal/...)를 React가 부른 경로로 바꿈. path가 없으면 그대로
pub fn hide_internal_path(body: &[u8], path: &str) -> Vec<u8> {
    let Some(Value::Object(mut data)) = pyjson::loads(body) else {
        return body.to_vec();
    };
    if !data.contains_key("path") {
        return body.to_vec();
    }
    data.insert("path".into(), Value::String(path.into()));
    pyjson::dumps(&Value::Object(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hide_internal_path_rewrites_like_python() {
        let body = br#"{"timestamp":"t","status":"error","httpCode":401,"message":"m","path":"/internal/auth/check","code":"UNAUTHORIZED"}"#;
        assert_eq!(
            String::from_utf8(hide_internal_path(body, "/ai/characters")).unwrap(),
            r#"{"timestamp": "t", "status": "error", "httpCode": 401, "message": "m", "path": "/ai/characters", "code": "UNAUTHORIZED"}"#
        );
    }

    #[test]
    fn hide_internal_path_keeps_body_without_path_or_json() {
        assert_eq!(hide_internal_path(br#"{"a":1}"#, "/x"), br#"{"a":1}"#);
        assert_eq!(hide_internal_path(b"[1]", "/x"), b"[1]");
        assert_eq!(hide_internal_path(b"oops", "/x"), b"oops");
    }

    #[test]
    fn decoded_path_unquotes() {
        assert_eq!(
            decoded_path("/ai/items/bake%2Dhints"),
            "/ai/items/bake-hints"
        );
        assert_eq!(decoded_path("/a/%ED%95%9C"), "/a/한");
    }
}
