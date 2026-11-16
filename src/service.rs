//! `app/character/service.py` : 캐릭터·아이템 업무 흐름
//!
//! 지금(M4)은 관리자 경로 3개가 쓰는 것만 있다: `describe_item`·`draw_item`·`trial_preview`와
//! 그 아래 `bake_once`·`render`. 굽기 큐와 회원 경로(`create_job`·`preview` 등)는 M5에서 옮긴다.

use std::collections::HashMap;

use bytes::Bytes;

use crate::backend::external_error;
use crate::bake;
use crate::canvas::Layout;
use crate::clients::{CharacterApiError, CharacterClient, heavy};
use crate::compose;
use crate::engine::{self, BakeItem, EngineError};
use crate::error::ApiError;
use crate::image::validate_upload;
use crate::logs;
use crate::repository::CharacterJobRepository;
use crate::response::AppError;

const LOGGER: &str = "app.character.service";

pub fn check_region(regn_cd: &str) -> Result<(), ApiError> {
    if engine::is_bake_region(regn_cd) {
        return Ok(());
    }
    Err(AppError::new("INVALID_REQUEST", 400, "구울 수 없는 부위입니다.").into())
}

fn item_image_error() -> ApiError {
    AppError::new(
        "CHARACTER_API_ERROR",
        502,
        "픽셀 아이템 그림을 만들지 못했습니다.",
    )
    .into()
}

fn preview_error() -> ApiError {
    AppError::new(
        "CHARACTER_API_ERROR",
        502,
        "캐릭터 미리보기를 만들지 못했습니다.",
    )
    .into()
}

/// FN-ADM-070-02 : 관리자 아이템 그림 -> 굽기 지시 초안 (저장 안 함)
pub async fn describe_item<C: CharacterClient>(
    client: Option<&C>,
    image: &[u8],
    file_name: &str,
    regn_cd: &str,
    item_nm: &str,
) -> Result<String, ApiError> {
    check_region(regn_cd)?;
    validate_upload(image, Some(file_name))?;
    let Some(client) = client else {
        logs::error(
            LOGGER,
            "굽기 지시 초안 불가: 생성기가 아직 연결되지 않았습니다",
        );
        return Err(external_error().into());
    };
    client
        .describe_item(image, regn_cd, item_nm)
        .await
        .map_err(|CharacterApiError| external_error().into())
}

/// FN-ADM-070-02 : 실사 아이템 사진 -> 투명 배경 픽셀 아이템 그림 (상점 이미지, 저장 안 함)
pub async fn draw_item<C: CharacterClient>(
    client: Option<&C>,
    image: &[u8],
    file_name: &str,
    regn_cd: &str,
    item_nm: &str,
) -> Result<Vec<u8>, ApiError> {
    check_region(regn_cd)?;
    validate_upload(image, Some(file_name))?;
    let Some(client) = client else {
        logs::error(
            LOGGER,
            "픽셀 아이템 그림 불가: 생성기가 아직 연결되지 않았습니다",
        );
        return Err(external_error().into());
    };
    let drawn = client
        .draw_item(image, regn_cd, item_nm)
        .await
        .map_err(|CharacterApiError| item_image_error())?;
    // ItemImageError·OSError(PIL이 그림으로 못 읽음)·ValueError는 502, 그 밖의 예외는 그대로 올라간다
    let cut = heavy(move || engine::cut_item(&drawn)).await;
    match cut {
        Ok(Ok(png)) => Ok(png),
        Ok(Err(error)) => {
            logs::error(
                LOGGER,
                format!(
                    "픽셀 아이템 그림 후처리 실패: {}",
                    engine::error_name(&error)
                ),
            );
            Err(item_image_error())
        }
        Err(_) => Err(ApiError::Internal),
    }
}

/// `_bake_once`가 실패한 이유 : 큐와 시험 굽기가 모두 "실패"로만 본다
#[derive(Debug)]
pub enum BakeError {
    /// 굽기 입력 만들기·검증(이미지 계산)
    Engine(EngineError),
    /// OpenAI
    Api(CharacterApiError),
    /// 그림 참고 굽기의 아이템 그림 받기(Java)
    Backend(ApiError),
    /// 계산 스레드가 죽음
    Panic,
}

impl BakeError {
    /// 예외 이름(로그용)
    pub fn name(&self) -> &'static str {
        match self {
            Self::Engine(error) => engine::error_name(error),
            Self::Api(_) => "CharacterApiError",
            Self::Backend(ApiError::Backend(_)) => "BackendError",
            Self::Backend(_) => "AppError",
            Self::Panic => "RuntimeError",
        }
    }
}

/// 맨몸 시트에 아이템을 굽고 합성에 쓸 수 있는지 판정한 구운 시트
pub async fn bake_once<C: CharacterClient, R: CharacterJobRepository>(
    client: &C,
    repo: &R,
    sheet: &Bytes,
    pet_se_cd: &str,
    item: &BakeItem,
) -> Result<Vec<u8>, BakeError> {
    let (bare, regn_cd) = (sheet.clone(), item.regn_cd.clone());
    let source = heavy(move || bake::bake_source(&bare, &regn_cd))
        .await
        .map_err(|_| BakeError::Panic)?
        .map_err(BakeError::Engine)?;
    let worn = if item.by_image {
        // 그림 참고 굽기 : 받기 실패도 굽기 실패와 같이 다시 시도
        let item_image = repo
            .fetch_item_image(item.item_sn)
            .await
            .map_err(BakeError::Backend)?;
        client
            .bake(&source, pet_se_cd, item, Some(&item_image))
            .await
    } else {
        client.bake(&source, pet_se_cd, item, None).await
    }
    .map_err(BakeError::Api)?;
    let (bare, worn_sheet, regn_cd) = (sheet.clone(), worn.clone(), item.regn_cd.clone());
    heavy(move || bake::validate(&bare, &worn_sheet, &regn_cd))
        .await
        .map_err(|_| BakeError::Panic)?
        .map_err(BakeError::Engine)?;
    Ok(worn)
}

/// 부위 코드별 착용 시트를 입혀 합성 PNG를 만든다 (실패하면 502)
pub async fn render(
    bare: &Bytes,
    layout: Layout,
    worn: HashMap<String, Vec<u8>>,
) -> Result<Vec<u8>, ApiError> {
    let bare = bare.clone();
    let rendered = heavy(move || compose::render(&bare, &layout, &worn)).await;
    match rendered {
        Ok(Ok(png)) => Ok(png),
        Ok(Err(error)) => {
            logs::error(
                LOGGER,
                format!("캐릭터 합성 실패: {}", engine::error_name(&error)),
            );
            Err(preview_error())
        }
        Err(_) => {
            logs::error(LOGGER, "캐릭터 합성 실패: RuntimeError");
            Err(preview_error())
        }
    }
}

/// FN-ADM-070-02 : 굽기 지시 1회 시험 굽기·합성 (저장 안 함, 굽기 설정과 무관)
pub async fn trial_preview<C: CharacterClient, R: CharacterJobRepository>(
    client: Option<&C>,
    repo: &R,
    pixel_char_sn: i64,
    regn_cd: &str,
    bake_hint: &str,
) -> Result<Vec<u8>, ApiError> {
    check_region(regn_cd)?;
    let Some(client) = client else {
        logs::error(LOGGER, "시험 굽기 불가: 생성기가 아직 연결되지 않았습니다");
        return Err(external_error().into());
    };
    let found = repo.find_sheets(pixel_char_sn, &[]).await?;
    let item = BakeItem {
        item_sn: 0,
        regn_cd: regn_cd.to_owned(),
        bake_hint: Some(bake_hint.to_owned()),
        by_image: false,
    };
    let worn = match bake_once(client, repo, &found.sheet, &found.pet_se_cd, &item).await {
        Ok(worn) => worn,
        Err(error) => {
            logs::error(LOGGER, format!("시험 굽기 실패: {}", error.name()));
            return Err(preview_error());
        }
    };
    render(
        &found.sheet,
        found.layout,
        HashMap::from([(item.regn_cd, worn)]),
    )
    .await
}
