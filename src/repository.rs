//! `app/character/repository.py` : Java 캐릭터 내부 API 호출 (`docs/api/internal.md` "Python → Java")
//!
//! 회원 판정은 Java(브라우저 Cookie 그대로 전달) : 4xx는 그대로 전달, 연결 실패·5xx·응답 모양 불일치는 502.
//! 응답 뒤 호출도 요청에서 복사해 둔 쿠키를 쓴다 : 쿠키가 만료되면 Java가 401(보고 실패로 남음)
//! - 차이: `BakeTargetPage`·`CharacterSheets`의 번호 목록·표는 Python `dict`·`set` 대신 `Vec`·`HashMap`·`HashSet`.
//!   `list_bake_items`는 Python `dict`처럼 처음 나온 자리에 마지막 값을 둔 `Vec`로 돌려준다

use std::collections::{BTreeSet, HashMap, HashSet};
use std::future::Future;

use axum::http::{HeaderValue, Method, header};
use bytes::Bytes;
use reqwest::multipart::{Form, Part};
use serde_json::{Map, Value};

use crate::auth::REQUESTED_WITH;
use crate::backend::{Backend, external_error};
use crate::canvas::Layout;
use crate::engine::BakeItem;
use crate::error::ApiError;
use crate::logs;
use crate::pyjson;
use crate::pyval;

const LOGGER: &str = "app.character.repository";
/// Java 캐릭터 내부 API
pub const CHARACTERS_PATH: &str = "/internal/ai/characters";
pub const JOBS_PATH: &str = "/internal/ai/characters/jobs";
pub const ITEMS_PATH: &str = "/internal/ai/items";
const REQUESTED_WITH_HEADER: &str = "X-Requested-With";
/// 시트는 1024 PNG : 기본 3초로는 전송이 끊길 수 있음
pub const SHEET_TIMEOUT_SEC: f64 = 15.0;
const DEFAULT_TIMEOUT_SEC: f64 = crate::backend::TIMEOUT_SEC;

/// 실패 사유 : Java CHAR_CRT_JOB 실패 사유로 저장
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharacterFailure {
    /// 생성기 오류
    ApiError,
    /// 512×512 규격 밖
    SpecMismatch,
}

impl CharacterFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApiError => "CHARACTER_API_ERROR",
            Self::SpecMismatch => "CHARACTER_SPEC_MISMATCH",
        }
    }
}

/// Java가 만든 생성 작업과 반려동물 등록 정보(FN-MYP-010-08 읽기 전용 값)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharacterJob {
    pub char_crt_job_sn: i64,
    pub pet_se_cd: String,
    pub vrty_nm: Option<String>,
    pub colr_nm: Option<String>,
    pub chrc_cn: Option<String>,
}

/// 합성용 시트 : `sheets`는 구운 아이템, `failed`는 굽기 실패 아이템
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterSheets {
    pub pet_se_cd: String,
    pub sheet: Bytes,
    pub layout: Layout,
    pub sheets: HashMap<i64, Bytes>,
    pub failed: HashSet<i64>,
}

/// FN-ADM-070-02 : 안 구운 캐릭터 첫 페이지 (Java 100개씩)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BakeTargetPage {
    pub pixel_char_sns: Vec<i64>,
    pub last: bool,
}

pub trait CharacterJobRepository: Send + Sync {
    fn create_job(
        &self,
        pet_sn: i64,
    ) -> impl Future<Output = Result<CharacterJob, ApiError>> + Send;

    fn report_success(
        &self,
        char_crt_job_sn: i64,
        image: &[u8],
        sheet: &[u8],
        layout: Option<Layout>,
    ) -> impl Future<Output = Result<i64, ApiError>> + Send;

    fn report_failure(
        &self,
        char_crt_job_sn: i64,
        reason: CharacterFailure,
    ) -> impl Future<Output = Result<(), ApiError>> + Send;

    fn list_bake_items(&self) -> impl Future<Output = Result<Vec<BakeItem>, ApiError>> + Send;

    fn fetch_item_image(
        &self,
        item_sn: i64,
    ) -> impl Future<Output = Result<Bytes, ApiError>> + Send;

    fn list_bake_targets(
        &self,
        item_sn: i64,
    ) -> impl Future<Output = Result<BakeTargetPage, ApiError>> + Send;

    fn save_item_sheet(
        &self,
        pixel_char_sn: i64,
        item_sn: i64,
        sheet: &[u8],
    ) -> impl Future<Output = Result<(), ApiError>> + Send;

    fn report_bake_failure(
        &self,
        pixel_char_sn: i64,
        item_sn: i64,
    ) -> impl Future<Output = Result<(), ApiError>> + Send;

    fn find_sheets(
        &self,
        pixel_char_sn: i64,
        item_sns: &[i64],
    ) -> impl Future<Output = Result<CharacterSheets, ApiError>> + Send;

    fn save_equipped_image(
        &self,
        pixel_char_sn: i64,
        item_sns: &[i64],
        image: &[u8],
    ) -> impl Future<Output = Result<(), ApiError>> + Send;
}

/// 응답 모양이 틀림 : `model_validate`의 ValidationError → 502
fn shape<T>(value: Option<T>) -> Result<T, ApiError> {
    value.ok_or_else(|| external_error().into())
}

/// 키가 없거나 null이면 None, 문자열이면 Some, 그 밖은 모양 불일치(`str | None`)
fn optional_str(map: &Map<String, Value>, key: &str) -> Option<Option<String>> {
    match map.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(Value::String(text)) => Some(Some(text.clone())),
        Some(_) => None,
    }
}

fn pet_se_cd(map: &Map<String, Value>) -> Option<String> {
    match map.get("petSeCd") {
        Some(Value::String(code)) if code == "PETC0001" || code == "PETC0002" => Some(code.clone()),
        _ => None,
    }
}

fn required_int(map: &Map<String, Value>, key: &str) -> Option<i64> {
    pyval::pydantic_int(map.get(key)?)
}

fn parse_job(data: &Value) -> Option<CharacterJob> {
    let map = data.as_object()?;
    Some(CharacterJob {
        char_crt_job_sn: required_int(map, "charCrtJobSn")?,
        pet_se_cd: pet_se_cd(map)?,
        vrty_nm: optional_str(map, "vrtyNm")?,
        colr_nm: optional_str(map, "colrNm")?,
        chrc_cn: optional_str(map, "chrcCn")?,
    })
}

fn parse_bake_item(data: &Value) -> Option<BakeItem> {
    let map = data.as_object()?;
    let bake_hint = optional_str(map, "bakeHint")?;
    if bake_hint.as_deref().is_some_and(str::is_empty) {
        return None;
    }
    Some(BakeItem {
        item_sn: required_int(map, "itemSn")?,
        regn_cd: map.get("regnCd")?.as_str()?.to_owned(),
        bake_hint,
        by_image: pyval::pydantic_bool(map.get("bakeImgRefYn")?)?,
    })
}

/// `[_BakeItem.model_validate(item) for item in data]` : 목록이 아니어도 Python이 돌 수 있는 값
/// (빈 객체·빈 문자열은 빈 목록, 그 밖은 오류)
fn parse_bake_items(data: &Value) -> Option<Vec<BakeItem>> {
    match data {
        Value::Array(items) => items.iter().map(parse_bake_item).collect(),
        Value::Object(map) if map.is_empty() => Some(Vec::new()),
        Value::String(text) if text.is_empty() => Some(Vec::new()),
        _ => None,
    }
}

fn parse_target_page(data: &Value) -> Option<BakeTargetPage> {
    let map = data.as_object()?;
    let Value::Array(sns) = map.get("pixelCharSns")? else {
        return None;
    };
    Some(BakeTargetPage {
        pixel_char_sns: sns
            .iter()
            .map(pyval::pydantic_int)
            .collect::<Option<Vec<_>>>()?,
        last: pyval::pydantic_bool(map.get("last")?)?,
    })
}

struct SheetItem {
    item_sn: i64,
    atch_file_sn: Option<i64>,
    bake_failed: bool,
}

struct SheetsResponse {
    pet_se_cd: String,
    sheet_file_sn: i64,
    layout: Layout,
    items: Vec<SheetItem>,
}

fn parse_sheets(data: &Value) -> Option<SheetsResponse> {
    let map = data.as_object()?;
    let items = match map.get("items") {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                let item = item.as_object()?;
                Some(SheetItem {
                    item_sn: required_int(item, "itemSn")?,
                    atch_file_sn: match item.get("atchFileSn") {
                        None | Some(Value::Null) => None,
                        Some(value) => Some(pyval::pydantic_int(value)?),
                    },
                    bake_failed: match item.get("bakeFailed") {
                        None => false,
                        Some(value) => pyval::pydantic_bool(value)?,
                    },
                })
            })
            .collect::<Option<Vec<_>>>()?,
        Some(_) => return None,
    };
    Some(SheetsResponse {
        pet_se_cd: pet_se_cd(map)?,
        sheet_file_sn: required_int(map, "sheetFileSn")?,
        layout: Layout {
            scale: pyval::pydantic_float(map.get("scale")?)?,
            origin_x: pyval::pydantic_float(map.get("originX")?)?,
            origin_y: pyval::pydantic_float(map.get("originY")?)?,
        },
        items,
    })
}

/// Python `str(float)` : `place`의 `min(.., TOP_MAX)`가 100(정수)을 돌려주면 `"100"`
/// 러스트 `Layout`은 f64라 정확히 100.0이면 정수 100으로 본다(실수가 우연히 100.0일 확률은 0에 가깝다)
fn placement_text(value: f64) -> String {
    if value == crate::canvas::TOP_MAX {
        return "100".into();
    }
    pyjson::float_repr(value)
}

fn png_part(content: &[u8]) -> Part {
    Part::bytes(content.to_vec())
        .mime_str("image/png")
        .expect("image/png")
}

fn png_file(name: &'static str, content: &[u8]) -> Part {
    png_part(content).file_name(name)
}

#[derive(Clone)]
pub struct JavaRepository {
    backend: Backend,
    cookie: Option<HeaderValue>,
}

impl JavaRepository {
    /// `cookie`: 요청의 Cookie 헤더 그대로 (비어 있으면 보내지 않음)
    pub fn new(backend: Backend, cookie: Option<HeaderValue>) -> Self {
        Self {
            backend,
            cookie: cookie.filter(|value| !value.is_empty()),
        }
    }

    /// Java RequestedWithFilter가 /internal/**의 POST에도 적용
    fn with_headers(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let builder = builder.header(REQUESTED_WITH_HEADER, REQUESTED_WITH);
        match &self.cookie {
            Some(cookie) => builder.header(header::COOKIE, cookie.clone()),
            None => builder,
        }
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        timeout_sec: f64,
        build: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<Value, ApiError> {
        self.backend
            .call(method, path, timeout_sec, |b| build(self.with_headers(b)))
            .await
    }

    async fn fetch(&self, path: &str) -> Result<Bytes, ApiError> {
        self.backend
            .fetch_bytes(path, SHEET_TIMEOUT_SEC, |b| self.with_headers(b))
            .await
    }

    async fn fetch_sheet(&self, pixel_char_sn: i64, atch_file_sn: i64) -> Result<Bytes, ApiError> {
        self.fetch(&format!(
            "{CHARACTERS_PATH}/{pixel_char_sn}/sheet-files/{atch_file_sn}"
        ))
        .await
    }
}

impl CharacterJobRepository for JavaRepository {
    async fn create_job(&self, pet_sn: i64) -> Result<CharacterJob, ApiError> {
        // FN-MYP-010-08 : 주양육자·이미 캐릭터 있음·생성 중 판정은 Java
        let data = self
            .call(Method::POST, JOBS_PATH, DEFAULT_TIMEOUT_SEC, |b| {
                b.json(&serde_json::json!({ "petSn": pet_sn }))
            })
            .await?;
        shape(parse_job(&data))
    }

    async fn report_success(
        &self,
        char_crt_job_sn: i64,
        image: &[u8],
        sheet: &[u8],
        layout: Option<Layout>,
    ) -> Result<i64, ApiError> {
        // FN-MYP-010-09·17 : 결과와 합성에 쓸 맨몸 시트·배치값을 함께 저장 요청
        let mut form = Form::new();
        if let Some(layout) = layout {
            form = form
                .text("scale", pyjson::float_repr(layout.scale))
                .text("originX", pyjson::float_repr(layout.origin_x))
                .text("originY", placement_text(layout.origin_y));
        }
        let form = form
            .part("image", png_file("image.png", image))
            .part("sheet", png_file("sheet.png", sheet));
        let data = self
            .call(
                Method::POST,
                &format!("{JOBS_PATH}/{char_crt_job_sn}/success"),
                SHEET_TIMEOUT_SEC,
                |b| b.multipart(form),
            )
            .await?;
        shape(
            data.as_object()
                .and_then(|map| required_int(map, "pixelCharSn")),
        )
    }

    async fn report_failure(
        &self,
        char_crt_job_sn: i64,
        reason: CharacterFailure,
    ) -> Result<(), ApiError> {
        self.call(
            Method::POST,
            &format!("{JOBS_PATH}/{char_crt_job_sn}/failure"),
            DEFAULT_TIMEOUT_SEC,
            |b| b.json(&serde_json::json!({ "failRsn": reason.as_str() })),
        )
        .await
        .map(drop)
    }

    async fn list_bake_items(&self) -> Result<Vec<BakeItem>, ApiError> {
        let data = self
            .call(
                Method::GET,
                &format!("{CHARACTERS_PATH}/bake-items"),
                DEFAULT_TIMEOUT_SEC,
                |b| b,
            )
            .await?;
        let items = shape(parse_bake_items(&data))?;
        // dict : 같은 번호는 처음 나온 자리에 마지막 값
        let mut found: Vec<BakeItem> = Vec::new();
        for item in items {
            if item.bake_hint.is_none() && !item.by_image {
                // 굽기 지시도 그림도 없는 행 : 구울 방법이 없어 제외
                logs::warning(
                    LOGGER,
                    format!(
                        "굽기 지시·그림이 모두 없는 아이템 제외: itemSn={}",
                        item.item_sn
                    ),
                );
                continue;
            }
            match found.iter_mut().find(|known| known.item_sn == item.item_sn) {
                Some(known) => *known = item,
                None => found.push(item),
            }
        }
        Ok(found)
    }

    async fn fetch_item_image(&self, item_sn: i64) -> Result<Bytes, ApiError> {
        self.fetch(&format!("{ITEMS_PATH}/{item_sn}/bake-image"))
            .await
    }

    async fn list_bake_targets(&self, item_sn: i64) -> Result<BakeTargetPage, ApiError> {
        // FN-ADM-070-02 : 아직 안 구운 캐릭터 첫 페이지 (관리자 판정은 Java)
        // 구운·실패 표시한 캐릭터는 빠지므로 다음 묶음도 첫 페이지
        let data = self
            .call(
                Method::GET,
                &format!("{ITEMS_PATH}/{item_sn}/bake-targets"),
                DEFAULT_TIMEOUT_SEC,
                |b| b.query(&[("page", 0)]),
            )
            .await?;
        shape(parse_target_page(&data))
    }

    async fn save_item_sheet(
        &self,
        pixel_char_sn: i64,
        item_sn: i64,
        sheet: &[u8],
    ) -> Result<(), ApiError> {
        self.call(
            Method::PUT,
            &format!("{CHARACTERS_PATH}/{pixel_char_sn}/item-sheets/{item_sn}"),
            SHEET_TIMEOUT_SEC,
            |b| b.multipart(Form::new().part("sheet", png_file("sheet.png", sheet))),
        )
        .await
        .map(drop)
    }

    async fn report_bake_failure(&self, pixel_char_sn: i64, item_sn: i64) -> Result<(), ApiError> {
        self.call(
            Method::POST,
            &format!("{CHARACTERS_PATH}/{pixel_char_sn}/item-sheets/{item_sn}/failure"),
            DEFAULT_TIMEOUT_SEC,
            |b| b,
        )
        .await
        .map(drop)
    }

    async fn find_sheets(
        &self,
        pixel_char_sn: i64,
        item_sns: &[i64],
    ) -> Result<CharacterSheets, ApiError> {
        // FN-MYP-011-06 : 주양육자·소유 판정은 Java, 시트는 커서 파일 번호로 따로 받음
        let wanted: BTreeSet<i64> = item_sns.iter().copied().collect();
        let query: Vec<(&str, String)> = wanted
            .iter()
            .map(|sn| ("itemSns", sn.to_string()))
            .collect();
        let data = self
            .call(
                Method::GET,
                &format!("{CHARACTERS_PATH}/{pixel_char_sn}/sheets"),
                DEFAULT_TIMEOUT_SEC,
                |b| if query.is_empty() { b } else { b.query(&query) },
            )
            .await?;
        let found = shape(parse_sheets(&data))?;
        let items: Vec<&SheetItem> = found
            .items
            .iter()
            .filter(|item| wanted.contains(&item.item_sn))
            .collect();
        let baked: Vec<(i64, i64)> = items
            .iter()
            .filter_map(|item| Some((item.item_sn, item.atch_file_sn?)))
            .collect();
        let fetches = std::iter::once(self.fetch_sheet(pixel_char_sn, found.sheet_file_sn)).chain(
            baked
                .iter()
                .map(|&(_, atch_file_sn)| self.fetch_sheet(pixel_char_sn, atch_file_sn)),
        );
        let mut files = futures_util::future::try_join_all(fetches).await?;
        let sheet = files.remove(0);
        Ok(CharacterSheets {
            pet_se_cd: found.pet_se_cd,
            sheet,
            layout: found.layout,
            // dict(zip) : 같은 번호는 나중 값
            sheets: baked.iter().map(|&(sn, _)| sn).zip(files).collect(),
            failed: items
                .iter()
                .filter(|item| item.bake_failed)
                .map(|item| item.item_sn)
                .collect(),
        })
    }

    async fn save_equipped_image(
        &self,
        pixel_char_sn: i64,
        item_sns: &[i64],
        image: &[u8],
    ) -> Result<(), ApiError> {
        // FN-MYP-010-16 : Java 대표 이미지로 저장
        // 지금 확정 착용 목록과 다르면 409 EQUIPPED_ITEMS_CHANGED (옛 합성 덮어쓰기 방지)
        let mut form = Form::new();
        for sn in item_sns {
            form = form.text("itemSns", sn.to_string());
        }
        let form = form.part("image", png_file("image.png", image));
        self.call(
            Method::PUT,
            &format!("{CHARACTERS_PATH}/{pixel_char_sn}/equipped-image"),
            SHEET_TIMEOUT_SEC,
            |b| b.multipart(form),
        )
        .await
        .map(drop)
    }
}
