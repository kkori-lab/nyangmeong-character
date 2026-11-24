//! `app/core/auth.py` : `X-Requested-With` 검사, Java `/internal/auth/check`로 회원·관리자 판정

use crate::backend::{Backend, BackendError, TIMEOUT_SEC, external_error};
use crate::error::ApiError;
use crate::logs;
use crate::pyjson;
use crate::pyval;
use crate::response::AppError;
use axum::http::{HeaderMap, Method, header};
use serde_json::Value;

pub const AUTH_CHECK_PATH: &str = "/internal/auth/check";
pub const REQUESTED_WITH: &str = "XMLHttpRequest";
/// FN-ADM-001-01 : 관리자 권한 코드
pub const ADMIN_AUTHORITY: &str = "USRC0005";
const LOGGER: &str = "app.core.auth";

fn forbidden() -> ApiError {
    AppError::new("FORBIDDEN", 403, "접근 권한이 없습니다.").into()
}

/// 상태 변경 요청에 X-Requested-With 필수 (CSRF 토큰 대체, 백엔드 RequestedWithFilter와 같음)
pub fn require_requested_with(method: &Method, headers: &HeaderMap) -> Result<(), ApiError> {
    let safe = matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
    let value = headers.get("x-requested-with").map(|v| v.as_bytes());
    if !safe && value != Some(REQUESTED_WITH.as_bytes()) {
        return Err(forbidden());
    }
    Ok(())
}

/// 브라우저 Cookie로 Java /internal/auth/check 호출 -> 회원 번호 (캐시 없음)
pub async fn require_member(backend: &Backend, headers: &HeaderMap) -> Result<i64, ApiError> {
    let (mbr_sn, _) = check(backend, headers).await?;
    Ok(mbr_sn)
}

/// 관리자만 통과 -> 회원 번호 (권한 판정 값은 Java)
pub async fn require_admin(backend: &Backend, headers: &HeaderMap) -> Result<i64, ApiError> {
    let (mbr_sn, authority) = check(backend, headers).await?;
    if authority.as_deref() != Some(ADMIN_AUTHORITY) {
        return Err(forbidden());
    }
    Ok(mbr_sn)
}

async fn check(backend: &Backend, headers: &HeaderMap) -> Result<(i64, Option<String>), ApiError> {
    // Starlette headers.get : 같은 이름이 여럿이면 첫 값. 빈 값이면 보내지 않음
    let cookie = headers
        .get(header::COOKIE)
        .filter(|v| !v.is_empty())
        .cloned();
    let res = backend
        .request(
            Method::GET,
            AUTH_CHECK_PATH,
            TIMEOUT_SEC,
            |builder| match cookie {
                Some(cookie) => builder.header(header::COOKIE, cookie),
                None => builder,
            },
        )
        .await
        .map_err(|reason| fail(&reason))?;

    if res.status == 401 || res.status == 403 {
        return Err(BackendError::of(&res).into());
    }
    if res.status != 200 {
        return Err(fail(&format!("Java {}", res.status)));
    }
    parse(&res.body).ok_or_else(|| fail("응답 형식 틀림"))
}

/// `res.json()["data"]`, `int(data["mbrSn"])`, 권한 코드는 문자열일 때만
fn parse(body: &[u8]) -> Option<(i64, Option<String>)> {
    let Value::Object(root) = pyjson::loads(body)? else {
        return None;
    };
    let Value::Object(data) = root.get("data")? else {
        return None;
    };
    let mbr_sn = pyval::builtin_int(data.get("mbrSn")?)?;
    let authority = data
        .get("mbrAuthrtCd")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some((mbr_sn, authority))
}

/// 502 이유 한 줄 : 주소 값·쿠키·응답 본문은 남기지 않음
fn fail(reason: &str) -> ApiError {
    logs::warning(LOGGER, format!("인증 확인 실패({reason}), 502"));
    external_error().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_with_only_for_unsafe_methods() {
        let mut headers = HeaderMap::new();
        assert!(require_requested_with(&Method::GET, &headers).is_ok());
        assert!(require_requested_with(&Method::POST, &headers).is_err());
        headers.insert("X-Requested-With", "xmlhttprequest".parse().unwrap());
        assert!(require_requested_with(&Method::POST, &headers).is_err());
        headers.insert("X-Requested-With", "XMLHttpRequest".parse().unwrap());
        assert!(require_requested_with(&Method::POST, &headers).is_ok());
    }

    #[test]
    fn parse_matches_python() {
        assert_eq!(
            parse(br#"{"data":{"mbrSn":"7","mbrAuthrtCd":"USRC0005"}}"#),
            Some((7, Some("USRC0005".into())))
        );
        assert_eq!(
            parse(br#"{"data":{"mbrSn":7.9,"mbrAuthrtCd":3}}"#),
            Some((7, None))
        );
        assert_eq!(parse(br#"{"data":{"mbrAuthrtCd":"X"}}"#), None);
        assert_eq!(parse(br#"{"data":[1]}"#), None);
        assert_eq!(parse(br#"[{"data":{"mbrSn":1}}]"#), None);
    }
}
