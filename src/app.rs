//! `app/main.py` 앱 조립 + Starlette 라우터 동작(404·405·끝 `/` 리다이렉트)

use crate::backend::Backend;
use crate::character::{self, Ctx};
use crate::clients::OpenAiClient;
use crate::config::Settings;
use crate::error::{self, ApiError};
use crate::logs;
use crate::response::json_response;
use axum::Router;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<Settings>,
    pub backend: Backend,
    /// OpenAI 키가 없으면 None
    pub client: Option<Arc<OpenAiClient>>,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        let backend = Backend::new(&settings.backend_base_url);
        let client = OpenAiClient::from_settings(&settings).map(Arc::new);
        Self {
            client,
            settings: Arc::new(settings),
            backend,
        }
    }
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .fallback(dispatch)
        .with_state(state)
        .layer(middleware::from_fn(error::render_errors))
        .layer(middleware::from_fn(request_log))
        .layer(DefaultBodyLimit::disable())
}

/// 어떤 경로가 어떤 메서드를 받는지 : 405·리다이렉트 판정용
fn methods_for(path: &str) -> Option<&'static [Method]> {
    if path == "/health" {
        return Some(&[Method::GET, Method::HEAD]);
    }
    character::match_path(path).map(|_| &[Method::POST][..])
}

async fn dispatch(State(state): State<AppState>, request: Request) -> Response {
    let path = error::decoded_path(request.uri().path());
    let method = request.method().clone();

    let Some(methods) = methods_for(&path) else {
        if let Some(location) = redirect_location(&request, &path) {
            return redirect(&location);
        }
        return ApiError::Http {
            status: 404,
            detail: "Not Found".into(),
            allow: None,
        }
        .into_response();
    };
    if !methods.contains(&method) {
        let allow = methods
            .iter()
            .map(Method::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        return ApiError::Http {
            status: 405,
            detail: "Method Not Allowed".into(),
            allow: Some(allow),
        }
        .into_response();
    }
    if path == "/health" {
        return json_response(200, &json!({"status": "ok"}));
    }

    let (parts, body) = request.into_parts();
    let body = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(body) => body,
        Err(_) => {
            return ApiError::Http {
                status: 400,
                detail: "There was an error parsing the body".into(),
                allow: None,
            }
            .into_response();
        }
    };
    let matched = character::match_path(&path).expect("methods_for에서 확인");
    let ctx = Ctx {
        backend: &state.backend,
        client: state.client.as_deref(),
        method: &parts.method,
        headers: &parts.headers,
        body,
    };
    match character::handle(matched, ctx).await {
        Ok(res) => res,
        Err(error) => error.into_response(),
    }
}

/// Starlette redirect_slashes : 끝 `/`를 붙이거나 뗀 경로가 있으면 307
fn redirect_location(request: &Request, path: &str) -> Option<String> {
    if path == "/" {
        return None;
    }
    let other = match path.strip_suffix('/') {
        Some(stripped) => stripped.to_owned(),
        None => format!("{path}/"),
    };
    methods_for(&other)?;
    let headers = request.headers();
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let mut url = format!("{scheme}://{host}{other}");
    if let Some(query) = request.uri().query().filter(|q| !q.is_empty()) {
        url.push('?');
        url.push_str(query);
    }
    Some(url)
}

/// Starlette RedirectResponse : `quote(url, safe=":/%#?=@[]!$&'()*+,;")`
const URL_SAFE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'_')
    .remove(b'.')
    .remove(b'-')
    .remove(b'~')
    .remove(b':')
    .remove(b'/')
    .remove(b'%')
    .remove(b'#')
    .remove(b'?')
    .remove(b'=')
    .remove(b'@')
    .remove(b'[')
    .remove(b']')
    .remove(b'!')
    .remove(b'$')
    .remove(b'&')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')')
    .remove(b'*')
    .remove(b'+')
    .remove(b',')
    .remove(b';');

fn redirect(location: &str) -> Response {
    let location = utf8_percent_encode(location, URL_SAFE).to_string();
    let mut res = StatusCode::TEMPORARY_REDIRECT.into_response();
    if let Ok(value) = HeaderValue::from_str(&location) {
        res.headers_mut().insert(header::LOCATION, value);
    }
    res
}

/// `RequestLogMiddleware` : "METHOD 경로 -> 상태 (ms)" 한 줄, 쿼리·본문·헤더는 남기지 않음
async fn request_log(request: Request<Body>, next: Next) -> Response {
    let started = Instant::now();
    let method = request.method().clone();
    let path = error::decoded_path(request.uri().path());
    let res = next.run(request).await;
    let elapsed = started.elapsed().as_millis();
    logs::info(
        "app.request",
        format!("{method} {path} -> {} ({elapsed}ms)", res.status().as_u16()),
    );
    res
}
