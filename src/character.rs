//! `app/character/router.py` 경로 8개 : 본문 읽기 → X-Requested-With → 인증 → 값 검증 → 핸들러 순서가 Python과 같다
//! - 차이: Starlette `BackgroundTasks`는 응답 본문을 다 보낸 뒤 시작한다. 여기는 응답을 돌려주기 직전에
//!   `tokio::spawn`으로 띄워 Java 호출이 응답보다 먼저 갈 수 있다(응답·Java가 받는 요청은 같음)

use crate::auth;
use crate::backend::Backend;
use crate::clients::OpenAiClient;
use crate::error::ApiError;
use crate::form::{self, FormData, JsonBody};
use crate::image::MAX_IMAGE_BYTES;
use crate::pyval;
use crate::queue::BakeQueue;
use crate::repository::JavaRepository;
use crate::response::{self, AppError};
use crate::service;
use axum::body::Body as HttpBody;
use axum::http::{HeaderMap, Method, header};
use axum::response::Response;
use bytes::Bytes;
use serde_json::{Map, Value, json};
use std::sync::Arc;

/// FN-ADM-070-02 : 관리자 아이템 등록 보조
pub const ITEM_NM_MAX: usize = 50;
pub const BAKE_HINT_INPUT_MAX: usize = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    CreateCharacter,
    Preview,
    EquippedImage,
    BakeHints,
    PixelImage,
    TrialPreview,
    BakeItem,
    Regenerate,
}

/// 프로세스에 하나인 굽기 큐
pub type AppQueue = BakeQueue<OpenAiClient, JavaRepository>;

const ITEM_BAKING_MESSAGE: &str = "아이템을 준비하고 있습니다. 잠시 후 다시 시도해 주세요.";

/// 경로 일치 결과 : 경로 매개변수(디코딩된 문자열)
pub struct Matched {
    pub route: Route,
    pub param: Option<String>,
}

/// Starlette 경로 정규식 `{name}` = `[^/]+`, 디코딩된 경로로 맞춘다
pub fn match_path(path: &str) -> Option<Matched> {
    let segments: Vec<&str> = path.strip_prefix('/')?.split('/').collect();
    let param = |s: &str| (!s.is_empty()).then(|| s.to_owned());
    let matched = match segments.as_slice() {
        ["ai", "characters"] => Matched {
            route: Route::CreateCharacter,
            param: None,
        },
        ["ai", "characters", sn, "preview"] => Matched {
            route: Route::Preview,
            param: Some(param(sn)?),
        },
        ["ai", "characters", sn, "equipped-image"] => Matched {
            route: Route::EquippedImage,
            param: Some(param(sn)?),
        },
        ["ai", "items", "bake-hints"] => Matched {
            route: Route::BakeHints,
            param: None,
        },
        ["ai", "items", "pixel-image"] => Matched {
            route: Route::PixelImage,
            param: None,
        },
        ["ai", "items", "trial-preview"] => Matched {
            route: Route::TrialPreview,
            param: None,
        },
        ["ai", "items", sn, "bake"] => Matched {
            route: Route::BakeItem,
            param: Some(param(sn)?),
        },
        ["ai", "admin", "characters", "jobs", sn, "regenerate"] => Matched {
            route: Route::Regenerate,
            param: Some(param(sn)?),
        },
        _ => return None,
    };
    Some(matched)
}

pub struct Ctx<'a> {
    pub backend: &'a Backend,
    /// 키가 없으면 None : 업로드 검증 뒤 502
    pub client: Option<Arc<OpenAiClient>>,
    pub queue: &'a Arc<AppQueue>,
    pub method: &'a Method,
    pub headers: &'a HeaderMap,
    pub body: Bytes,
}

enum Body {
    None,
    Form(FormData),
    Json(JsonBody),
}

pub async fn handle(matched: Matched, ctx: Ctx<'_>) -> Result<Response, ApiError> {
    use Route::*;
    // 1. 본문 읽기 (의존성보다 먼저)
    let body = match matched.route {
        CreateCharacter | BakeHints | PixelImage => {
            Body::Form(form::read_form(ctx.headers, ctx.body).await?)
        }
        Preview | EquippedImage | TrialPreview => {
            Body::Json(form::read_json_body(ctx.headers, &ctx.body)?)
        }
        BakeItem | Regenerate => Body::None,
    };
    // 2. 라우터 의존성 : X-Requested-With
    auth::require_requested_with(ctx.method, ctx.headers)?;
    // 3. 경로 의존성 : 회원·관리자
    match matched.route {
        CreateCharacter | Preview | EquippedImage => {
            auth::require_member(ctx.backend, ctx.headers).await?
        }
        BakeHints | PixelImage | TrialPreview | BakeItem | Regenerate => {
            auth::require_admin(ctx.backend, ctx.headers).await?
        }
    };
    // 4. 경로·본문 값 검증
    let param = matched.param.as_deref();
    match (matched.route, &body) {
        (CreateCharacter, Body::Form(form)) => {
            let photo = form.file("photo");
            let pet_sn = form.text("petSn").and_then(pyval::pydantic_int_str);
            photo.zip(pet_sn).ok_or(ApiError::Validation)?;
        }
        (Preview | EquippedImage, Body::Json(json)) => {
            path_int(param)?;
            preview_request(json).ok_or(ApiError::Validation)?;
        }
        (BakeHints | PixelImage, Body::Form(form)) => {
            let image = form.file("image");
            let regn_cd = form.text("regnCd");
            let item_nm = form
                .text("itemNm")
                .filter(|n| (1..=ITEM_NM_MAX).contains(&pyval::py_len(n)));
            (image.is_some() && regn_cd.is_some() && item_nm.is_some())
                .then_some(())
                .ok_or(ApiError::Validation)?;
        }
        (TrialPreview, Body::Json(json)) => {
            trial_request(json).ok_or(ApiError::Validation)?;
        }
        (BakeItem | Regenerate, Body::None) => {
            path_int(param)?;
        }
        _ => unreachable!("경로마다 본문 종류가 정해져 있음"),
    }
    // 5. 핸들러
    let repo = || {
        Arc::new(JavaRepository::new(
            ctx.backend.clone(),
            ctx.headers.get(header::COOKIE).cloned(),
        ))
    };
    let client = ctx.client.as_deref();
    match (matched.route, &body) {
        (CreateCharacter, Body::Form(form)) => {
            let photo = form.file("photo").expect("값 검증을 통과함");
            let pet_sn = form
                .text("petSn")
                .and_then(pyval::pydantic_int_str)
                .expect("값 검증을 통과함");
            // 10MB + 1바이트까지만 읽기 : 초과 판정용
            let image = upload_bytes(photo);
            let repo = repo();
            let job = service::create_job(client, &*repo, image, &photo.filename, pet_sn).await?;
            let char_crt_job_sn = job.char_crt_job_sn;
            // FN-MYP-010-09 : 응답 뒤 생성 (상태 조회는 React가 Java에서)
            let generator = ctx.client.clone().expect("create_job이 생성기 확인");
            tokio::spawn(service::generate_and_report(
                generator,
                repo,
                ctx.queue.clone(),
                job,
                image.to_vec(),
                Some(photo.filename.clone()),
            ));
            Ok(response::success(
                Some(json!({ "charCrtJobSn": char_crt_job_sn })),
                202,
            ))
        }
        (Regenerate, Body::None) => {
            // FN-MYP-010-23 : Java가 만든 REGEN 작업 번호만 받고 입력은 Java에서 받음
            let char_crt_job_sn = path_int(param)?;
            service::regenerate(ctx.client.clone(), repo(), ctx.queue, char_crt_job_sn)?;
            Ok(response::success(None, 202))
        }
        (Preview, Body::Json(json)) => {
            let pixel_char_sn = path_int(param)?;
            let item_sns = preview_request(json).expect("값 검증을 통과함");
            let image = service::preview(
                ctx.client.clone(),
                repo(),
                ctx.queue,
                pixel_char_sn,
                &item_sns,
            )
            .await?;
            match image {
                Some(png) => Ok(png_response(png)),
                None => Err(item_baking(ctx.queue)),
            }
        }
        (EquippedImage, Body::Json(json)) => {
            let pixel_char_sn = path_int(param)?;
            let item_sns = preview_request(json).expect("값 검증을 통과함");
            let saved = service::save_equipped_image(
                ctx.client.clone(),
                repo(),
                ctx.queue,
                pixel_char_sn,
                &item_sns,
            )
            .await?;
            if saved {
                return Ok(response::success(None, 200));
            }
            Err(item_baking(ctx.queue))
        }
        (BakeItem, Body::None) => {
            // 한 묶음을 다 굽고 답함 : 관리자 프론트가 lastYn까지 다시 부름 (쿠키 30분 만료 대책)
            let item_sn = path_int(param)?;
            let result =
                service::bake_item_batch(ctx.client.clone(), repo(), ctx.queue, item_sn).await?;
            Ok(response::success(
                Some(json!({
                    "itemSn": item_sn,
                    "bakedCnt": result.baked_cnt,
                    "failedCnt": result.failed_cnt,
                    "lastYn": result.last,
                })),
                200,
            ))
        }
        (BakeHints, Body::Form(form)) => {
            let (image, regn_cd, item_nm) = admin_item_form(form);
            let hint = service::describe_item(
                client,
                upload_bytes(image),
                &image.filename,
                regn_cd,
                item_nm,
            )
            .await?;
            Ok(response::success(Some(json!({ "bakeHint": hint })), 200))
        }
        (PixelImage, Body::Form(form)) => {
            let (image, regn_cd, item_nm) = admin_item_form(form);
            let png = service::draw_item(
                client,
                upload_bytes(image),
                &image.filename,
                regn_cd,
                item_nm,
            )
            .await?;
            Ok(png_response(png))
        }
        (TrialPreview, Body::Json(json)) => {
            let (pixel_char_sn, regn_cd, bake_hint) =
                trial_request(json).expect("값 검증을 통과함");
            let repo = repo();
            let png =
                service::trial_preview(client, &*repo, pixel_char_sn, &regn_cd, &bake_hint).await?;
            Ok(png_response(png))
        }
        _ => unreachable!("경로마다 본문 종류가 정해져 있음"),
    }
}

/// 아직 안 구운 아이템 : 응답 뒤 굽고 409 `ITEM_BAKING`
/// (Python은 예외로 끝내면 백그라운드 작업이 버려져 `response.error`를 직접 돌려준다. 응답 모양은 같다)
fn item_baking(queue: &Arc<AppQueue>) -> ApiError {
    let queue = queue.clone();
    tokio::spawn(async move { queue.drain().await });
    AppError::new("ITEM_BAKING", 409, ITEM_BAKING_MESSAGE).into()
}

/// 업로드는 10MB + 1바이트까지만 읽음 : 초과 판정용
fn upload_bytes(file: &form::UploadFile) -> &[u8] {
    &file.data[..file.data.len().min(MAX_IMAGE_BYTES + 1)]
}

/// 값 검증을 통과한 관리자 아이템 폼 : image, regnCd, itemNm
fn admin_item_form(form: &FormData) -> (&form::UploadFile, &str, &str) {
    (
        form.file("image").expect("값 검증을 통과함"),
        form.text("regnCd").expect("값 검증을 통과함"),
        form.text("itemNm").expect("값 검증을 통과함"),
    )
}

/// `Response(png, media_type="image/png")`
fn png_response(png: Vec<u8>) -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "image/png")
        .body(HttpBody::from(png))
        .expect("PNG 응답")
}

fn path_int(param: Option<&str>) -> Result<i64, ApiError> {
    param
        .and_then(pyval::pydantic_int_str)
        .ok_or(ApiError::Validation)
}

/// alias(camelCase)를 먼저, 없으면 필드 이름(populate_by_name)
fn field<'a>(map: &'a Map<String, Value>, alias: &str, name: &str) -> Option<&'a Value> {
    map.get(alias).or_else(|| map.get(name))
}

/// CharacterPreviewRequest : item_sns: list[int]
pub fn preview_request(body: &JsonBody) -> Option<Vec<i64>> {
    let map = body.object()?;
    let Value::Array(items) = field(map, "itemSns", "item_sns")? else {
        return None;
    };
    items.iter().map(pyval::pydantic_int).collect()
}

/// ItemTrialRequest : pixel_char_sn: int, regn_cd: str, bake_hint: str(1..=300)
pub fn trial_request(body: &JsonBody) -> Option<(i64, String, String)> {
    let map = body.object()?;
    let pixel_char_sn = pyval::pydantic_int(field(map, "pixelCharSn", "pixel_char_sn")?)?;
    let regn_cd = pyval::pydantic_str(field(map, "regnCd", "regn_cd")?)?;
    let bake_hint = pyval::pydantic_str(field(map, "bakeHint", "bake_hint")?)?;
    if !(1..=BAKE_HINT_INPUT_MAX).contains(&pyval::py_len(bake_hint)) {
        return None;
    }
    Some((pixel_char_sn, regn_cd.to_owned(), bake_hint.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn match_path_routes() {
        assert_eq!(
            match_path("/ai/characters").unwrap().route,
            Route::CreateCharacter
        );
        let m = match_path("/ai/characters/12/preview").unwrap();
        assert_eq!((m.route, m.param.as_deref()), (Route::Preview, Some("12")));
        assert_eq!(
            match_path("/ai/items/bake-hints").unwrap().route,
            Route::BakeHints
        );
        assert_eq!(
            match_path("/ai/items/x/bake").unwrap().route,
            Route::BakeItem
        );
        assert!(match_path("/ai/characters/").is_none());
        assert!(match_path("/ai/characters//preview").is_none());
        assert!(match_path("/ai/items").is_none());
    }

    #[test]
    fn preview_request_rules() {
        let body = |v: Value| JsonBody::Json(v);
        assert_eq!(
            preview_request(&body(json!({"itemSns": [1, "2", 3.0, true]}))),
            Some(vec![1, 2, 3, 1])
        );
        assert_eq!(
            preview_request(&body(json!({"item_sns": []}))),
            Some(vec![])
        );
        assert_eq!(
            preview_request(&body(json!({"itemSns": [1], "item_sns": [2]}))),
            Some(vec![1])
        );
        assert_eq!(preview_request(&body(json!({"itemSns": "1"}))), None);
        assert_eq!(preview_request(&body(json!([1]))), None);
        assert_eq!(preview_request(&JsonBody::Raw), None);
        assert_eq!(preview_request(&JsonBody::Undefined), None);
    }

    #[test]
    fn trial_request_rules() {
        let ok = json!({"pixelCharSn": 1, "regnCd": "H", "bakeHint": "가".repeat(300)});
        assert!(trial_request(&JsonBody::Json(ok)).is_some());
        let long = json!({"pixelCharSn": 1, "regnCd": "H", "bakeHint": "가".repeat(301)});
        assert!(trial_request(&JsonBody::Json(long)).is_none());
        let number = json!({"pixelCharSn": 1, "regnCd": 1, "bakeHint": "a"});
        assert!(trial_request(&JsonBody::Json(number)).is_none());
        let empty = json!({"pixelCharSn": 1, "regnCd": "H", "bakeHint": ""});
        assert!(trial_request(&JsonBody::Json(empty)).is_none());
    }
}
