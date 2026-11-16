//! FastAPI가 핸들러 전에 읽는 요청 본문 : Starlette `request.form()`·`request.json()`
//!
//! 폼 파싱 실패는 의존성(`X-Requested-With`·인증)보다 먼저 400 `{"detail": ...}`,
//! JSON 문법 오류도 먼저 400 INVALID_REQUEST다. 값 검증 오류는 의존성 뒤에 난다.

use crate::error::ApiError;
use axum::http::{HeaderMap, header};
use bytes::Bytes;
use serde_json::Value;

/// Starlette 폼 제한 : 파일 아닌 칸 1MB, 칸·파일 각 1000개
const MAX_PART_SIZE: usize = 1024 * 1024;
const MAX_FIELDS: usize = 1000;
const MAX_FILES: usize = 1000;

#[derive(Clone, Debug)]
pub struct UploadFile {
    pub filename: String,
    pub content_type: Option<String>,
    pub data: Bytes,
}

#[derive(Clone, Debug)]
pub enum FormValue {
    Text(String),
    File(UploadFile),
}

#[derive(Clone, Debug, Default)]
pub struct FormData {
    items: Vec<(String, FormValue)>,
}

impl FormData {
    /// Starlette FormData.get : 같은 이름이면 마지막 값.
    /// FastAPI는 빈 문자열 폼 값을 없는 값으로 본다
    pub fn get(&self, name: &str) -> Option<&FormValue> {
        let value = self
            .items
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, v)| v)?;
        match value {
            FormValue::Text(text) if text.is_empty() => None,
            other => Some(other),
        }
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            FormValue::Text(text) => Some(text),
            FormValue::File(_) => None,
        }
    }

    pub fn file(&self, name: &str) -> Option<&UploadFile> {
        match self.get(name)? {
            FormValue::File(file) => Some(file),
            FormValue::Text(_) => None,
        }
    }
}

fn parse_error(detail: &str) -> ApiError {
    ApiError::Http {
        status: 400,
        detail: detail.into(),
        allow: None,
    }
}

/// python-multipart `parse_options_header` : 소문자 형식, 매개변수(이름 소문자, 따옴표 제거)
pub fn parse_options_header(value: Option<&str>) -> (String, Vec<(String, String)>) {
    let Some(value) = value.filter(|v| !v.is_empty()) else {
        return (String::new(), Vec::new());
    };
    let mut parts = split_params(value).into_iter();
    let main = parts.next().unwrap_or_default().trim().to_lowercase();
    let params = parts
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .map(|v| v.replace("\\\"", "\"").replace("\\\\", "\\"))
                .unwrap_or_else(|| value.to_owned());
            Some((key.trim().to_lowercase(), value))
        })
        .collect();
    (main, params)
}

/// `;`로 나누되 따옴표 안의 `;`는 나누지 않는다
fn split_params(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for c in value.chars() {
        match c {
            _ if escaped => {
                escaped = false;
                current.push(c);
            }
            '\\' if quoted => {
                escaped = true;
                current.push(c);
            }
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            ';' if !quoted => parts.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts
}

/// `_user_safe_decode` : 문자셋(기본 UTF-8)으로 못 읽으면 latin-1
fn safe_decode(data: &[u8]) -> String {
    match std::str::from_utf8(data) {
        Ok(text) => text.to_owned(),
        Err(_) => data.iter().map(|&b| b as char).collect(),
    }
}

/// Starlette `request.form()` : multipart·urlencoded만 읽고, 그 밖의 형식은 빈 폼
pub async fn read_form(headers: &HeaderMap, body: Bytes) -> Result<FormData, ApiError> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .map(|v| safe_decode(v.as_bytes()));
    let (main, params) = parse_options_header(content_type.as_deref());
    match main.as_str() {
        "multipart/form-data" => read_multipart(&params, body).await,
        "application/x-www-form-urlencoded" => read_urlencoded(&body),
        _ => Ok(FormData::default()),
    }
}

async fn read_multipart(params: &[(String, String)], body: Bytes) -> Result<FormData, ApiError> {
    let Some((_, boundary)) = params.iter().find(|(key, _)| key == "boundary") else {
        return Err(parse_error("Missing boundary in multipart."));
    };
    let stream = futures_util::stream::once(async move { Ok::<Bytes, std::io::Error>(body) });
    let mut multipart = multer::Multipart::new(stream, boundary.clone());
    let invalid = || parse_error("Invalid multipart data.");
    let (mut fields, mut files) = (0, 0);
    let mut form = FormData::default();
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(_) => return Err(invalid()),
        };
        let Some(name) = field.name().map(str::to_owned) else {
            return Err(parse_error(
                "The Content-Disposition header field \"name\" must be provided.",
            ));
        };
        match field.file_name().map(str::to_owned) {
            Some(filename) => {
                files += 1;
                if files > MAX_FILES {
                    return Err(parse_error(&format!(
                        "Too many files. Maximum number of files is {MAX_FILES}."
                    )));
                }
                let content_type = field.content_type().map(ToString::to_string);
                let data = field.bytes().await.map_err(|_| invalid())?;
                form.items.push((
                    name,
                    FormValue::File(UploadFile {
                        filename,
                        content_type,
                        data,
                    }),
                ));
            }
            None => {
                fields += 1;
                if fields > MAX_FIELDS {
                    return Err(parse_error(&format!(
                        "Too many fields. Maximum number of fields is {MAX_FIELDS}."
                    )));
                }
                let data = field.bytes().await.map_err(|_| invalid())?;
                if data.len() > MAX_PART_SIZE {
                    return Err(parse_error(&format!(
                        "Part exceeded maximum size of {}KB.",
                        MAX_PART_SIZE / 1024
                    )));
                }
                form.items.push((name, FormValue::Text(safe_decode(&data))));
            }
        }
    }
    Ok(form)
}

fn read_urlencoded(body: &[u8]) -> Result<FormData, ApiError> {
    let mut form = FormData::default();
    for (index, (name, value)) in form_urlencoded::parse(body).enumerate() {
        if index + 1 > MAX_FIELDS {
            return Err(parse_error(&format!(
                "Too many fields. Maximum number of fields is {MAX_FIELDS}."
            )));
        }
        if name.len() > MAX_PART_SIZE || value.len() > MAX_PART_SIZE {
            return Err(parse_error(&format!(
                "Field exceeded maximum size of {}KB.",
                MAX_PART_SIZE / 1024
            )));
        }
        form.items
            .push((name.into_owned(), FormValue::Text(value.into_owned())));
    }
    Ok(form)
}

/// FastAPI가 JSON 본문 칸에 넘기는 값
#[derive(Clone, Debug)]
pub enum JsonBody {
    /// 본문 없음
    Undefined,
    /// JSON 형식(`application/json`·`*+json`)으로 읽음
    Json(Value),
    /// 그 밖의 형식 : 바이트 그대로 (모델 검증에서 항상 실패)
    Raw,
}

impl JsonBody {
    pub fn object(&self) -> Option<&serde_json::Map<String, Value>> {
        match self {
            JsonBody::Json(Value::Object(map)) => Some(map),
            _ => None,
        }
    }
}

/// FastAPI `get_request_handler` 본문 읽기 (strict_content_type 기본값 True)
pub fn read_json_body(headers: &HeaderMap, body: &Bytes) -> Result<JsonBody, ApiError> {
    if body.is_empty() {
        return Ok(JsonBody::Undefined);
    }
    let Some(content_type) = headers.get(header::CONTENT_TYPE) else {
        return Ok(JsonBody::Raw);
    };
    if !is_json_content_type(&safe_decode(content_type.as_bytes())) {
        return Ok(JsonBody::Raw);
    }
    // json.loads(bytes) : UTF-8(BOM 허용). 디코딩 실패는 본문 파싱 오류, 문법 오류는 검증 오류
    let data = body.strip_prefix(b"\xef\xbb\xbf").unwrap_or(body);
    let text = std::str::from_utf8(data)
        .map_err(|_| parse_error("There was an error parsing the body"))?;
    serde_json::from_str(text)
        .map(JsonBody::Json)
        .map_err(|_| ApiError::Validation)
}

/// email.message : `/`가 정확히 하나가 아니면 text/plain
fn is_json_content_type(value: &str) -> bool {
    let main = split_params(value).into_iter().next().unwrap_or_default();
    let main = main.trim().to_lowercase();
    if main.matches('/').count() != 1 {
        return false;
    }
    let (maintype, subtype) = main.split_once('/').expect("/ 하나");
    maintype == "application" && (subtype == "json" || subtype.ends_with("+json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(content_type: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
        headers
    }

    #[tokio::test]
    async fn multipart_reads_fields_and_files_last_wins() {
        let body = "--b\r\nContent-Disposition: form-data; name=\"petSn\"\r\n\r\n1\r\n\
                    --b\r\nContent-Disposition: form-data; name=\"petSn\"\r\n\r\n2\r\n\
                    --b\r\nContent-Disposition: form-data; name=\"photo\"; filename=\"a.png\"\r\n\
                    Content-Type: image/png\r\n\r\nPNG\r\n--b--\r\n";
        let form = read_form(
            &headers("multipart/form-data; boundary=b"),
            Bytes::from(body),
        )
        .await
        .unwrap();
        assert_eq!(form.text("petSn"), Some("2"));
        let file = form.file("photo").unwrap();
        assert_eq!(file.filename, "a.png");
        assert_eq!(&file.data[..], b"PNG");
    }

    #[tokio::test]
    async fn multipart_without_boundary_is_parse_error() {
        let err = read_form(&headers("multipart/form-data"), Bytes::new())
            .await
            .unwrap_err();
        assert!(
            matches!(err, ApiError::Http { status: 400, ref detail, .. } if detail == "Missing boundary in multipart.")
        );
    }

    #[tokio::test]
    async fn other_content_type_is_empty_form_and_blank_is_missing() {
        let form = read_form(&headers("text/plain"), Bytes::from("x"))
            .await
            .unwrap();
        assert!(form.get("x").is_none());
        let form = read_form(
            &headers("application/x-www-form-urlencoded"),
            Bytes::from("a=&b=1"),
        )
        .await
        .unwrap();
        assert!(form.get("a").is_none());
        assert_eq!(form.text("b"), Some("1"));
    }

    #[test]
    fn json_body_rules() {
        let json = headers("Application/JSON; charset=utf-8");
        assert!(matches!(
            read_json_body(&json, &Bytes::new()).unwrap(),
            JsonBody::Undefined
        ));
        assert!(matches!(
            read_json_body(&json, &Bytes::from("{")),
            Err(ApiError::Validation)
        ));
        assert!(matches!(
            read_json_body(&json, &Bytes::from(&b"\xff"[..])),
            Err(ApiError::Http { status: 400, .. })
        ));
        assert!(
            read_json_body(&json, &Bytes::from("{\"a\":1}"))
                .unwrap()
                .object()
                .is_some()
        );
        let problem = headers("application/problem+json");
        assert!(matches!(
            read_json_body(&problem, &Bytes::from("[]")).unwrap(),
            JsonBody::Json(_)
        ));
        assert!(matches!(
            read_json_body(&headers("text/plain"), &Bytes::from("{")).unwrap(),
            JsonBody::Raw
        ));
        assert!(matches!(
            read_json_body(&HeaderMap::new(), &Bytes::from("{")).unwrap(),
            JsonBody::Raw
        ));
    }
}
