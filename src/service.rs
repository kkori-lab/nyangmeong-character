//! `app/character/service.py` : 캐릭터·아이템 업무 흐름
//!
//! 굽기 큐는 `queue.rs`. 응답 뒤에 할 일(`generate_and_report`·`regenerate_and_report`)은 경로가 `tokio::spawn`으로 띄움

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;

use crate::backend::external_error;
use crate::bake;
use crate::canvas::CANVAS_SIZE;
use crate::canvas::Layout;
use crate::clients::{CharacterApiError, CharacterClient, CharacterRequest, heavy};
use crate::compose;
use crate::engine::{self, BakeItem, EngineError};
use crate::error::ApiError;
use crate::image::validate_upload;
use crate::logs;
use crate::queue::{BAKE_BATCH, BakeOutcome, BakeQueue, BakeTask};
use crate::repository::{CharacterFailure, CharacterJob, CharacterJobRepository, CharacterSheets};
use crate::response::AppError;

const LOGGER: &str = "app.character.service";

/// 로그용 실패 이유 한 줄 : Java 4xx는 상태 숫자, AppError는 오류 코드(본문·쿠키·경로 숫자는 남기지 않음)
/// 원본은 traceback 마지막 줄에 예외 이름만 남김(`migration.md` "알려진 차이")
pub fn cause(error: &ApiError) -> String {
    match error {
        ApiError::App(e) => e.code.clone(),
        ApiError::Backend(e) => format!("Java {}", e.status_code),
        ApiError::Validation => "Validation".into(),
        ApiError::Http { status, .. } => format!("HTTP {status}"),
        ApiError::Internal => "Internal".into(),
    }
}

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
    // ItemImageError·OSError(PIL이 그림으로 못 읽음)·ValueError는 502, 그 밖의 예외는 그대로 올라감
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

/// `_bake_once`가 실패한 이유 : 큐와 시험 굽기가 모두 "실패"로만 취급
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

/// 부위 코드별 착용 시트를 입혀 합성 PNG 생성 (실패하면 502)
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

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// `rules.py:png_size` : PNG IHDR(첫 청크)의 폭·높이 (PNG가 아니면 None)
pub fn png_size(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 24 || !data.starts_with(PNG_SIGNATURE) || &data[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(data[16..20].try_into().expect("4바이트"));
    let height = u32::from_be_bytes(data[20..24].try_into().expect("4바이트"));
    Some((width, height))
}

/// `rules.py:matches_spec` : FN-MYP-010-18 512×512 PNG 여부 (백엔드 CharacterSpec.matches와 같은 기준)
pub fn matches_spec(data: &[u8]) -> bool {
    png_size(data) == Some((CANVAS_SIZE as u32, CANVAS_SIZE as u32))
}

/// FN-MYP-010-08 : 검증 실패·생성기 없음이면 Java 작업 미생성
pub async fn create_job<C: CharacterClient, R: CharacterJobRepository>(
    client: Option<&C>,
    repo: &R,
    photo: &[u8],
    file_name: &str,
    pet_sn: i64,
) -> Result<CharacterJob, ApiError> {
    validate_upload(photo, Some(file_name))?;
    if client.is_none() {
        logs::error(
            LOGGER,
            "캐릭터 생성 불가: 생성기가 아직 연결되지 않았습니다",
        );
        return Err(external_error().into());
    }
    // 주양육자 여부·이미 캐릭터 있음·생성 중 판정은 Java (4xx 그대로 전달)
    repo.create_job(pet_sn).await
}

/// FN-MYP-010-23 : 관리자 무료 재생성 입구(`router.py:regenerate_character`).
/// 생성기가 없으면 Java를 부르지 않고 502, 있으면 응답 뒤 `regenerate_and_report`를 띄움
pub fn regenerate<C, R>(
    client: Option<Arc<C>>,
    repo: Arc<R>,
    queue: &Arc<BakeQueue<C, R>>,
    char_crt_job_sn: i64,
) -> Result<(), ApiError>
where
    C: CharacterClient + 'static,
    R: CharacterJobRepository + 'static,
{
    let Some(client) = client else {
        logs::error(
            "app.character.router",
            "캐릭터 재생성 불가: 생성기가 아직 연결되지 않았습니다",
        );
        return Err(external_error().into());
    };
    tokio::spawn(regenerate_and_report(
        client,
        repo,
        queue.clone(),
        char_crt_job_sn,
    ));
    Ok(())
}

/// FN-MYP-010-23 : 관리자 무료 재생성. 응답 뒤 입력을 받아 생성·보고·굽기
pub async fn regenerate_and_report<C, R>(
    client: Arc<C>,
    repo: Arc<R>,
    queue: Arc<BakeQueue<C, R>>,
    char_crt_job_sn: i64,
) where
    C: CharacterClient + 'static,
    R: CharacterJobRepository + 'static,
{
    let job = match repo.regen_start(char_crt_job_sn).await {
        Ok(job) => job,
        Err(error) => {
            // 이미 처리 중(409)·REGEN 작업 아님(404) 등 : 보고할 곳이 없어 Java 시간 초과 배치가 정리
            logs::error(
                LOGGER,
                format!(
                    "재생성 시작 실패: charCrtJobSn={char_crt_job_sn}: {}",
                    cause(&error)
                ),
            );
            return;
        }
    };
    let photo = match repo.fetch_original_photo(char_crt_job_sn).await {
        Ok(photo) => photo,
        Err(error) => {
            logs::error(
                LOGGER,
                format!(
                    "재생성 원본 사진 조회 실패: charCrtJobSn={char_crt_job_sn}: {}",
                    cause(&error)
                ),
            );
            report_failure(&*repo, &job, CharacterFailure::InputError).await;
            return;
        }
    };
    // 업로드 때 검증된 원본이라 validate_upload는 다시 돌리지 않음(깨졌으면 생성 오류로 실패 보고)
    generate_and_report(client, repo, queue, job, photo.to_vec(), None).await;
}

/// FN-MYP-010-09 : 응답 뒤 백그라운드 생성, 결과·상태 Java 보고, 이어서 굽기
/// `file_name`이 있으면(첫 생성) 원본 사진도 success에 실어 무료 재생성 입력으로 남김
pub async fn generate_and_report<C, R>(
    client: Arc<C>,
    repo: Arc<R>,
    queue: Arc<BakeQueue<C, R>>,
    job: CharacterJob,
    photo: Vec<u8>,
    file_name: Option<String>,
) where
    C: CharacterClient + 'static,
    R: CharacterJobRepository + 'static,
{
    let request = CharacterRequest {
        photo: photo.clone(),
        pet_se_cd: job.pet_se_cd.clone(),
        vrty_nm: job.vrty_nm.clone(),
        colr_nm: job.colr_nm.clone(),
        chrc_cn: job.chrc_cn.clone(),
    };
    let character = match client.generate(request).await {
        Ok(character) => character,
        Err(CharacterApiError) => {
            report_failure(&*repo, &job, CharacterFailure::ApiError).await;
            return;
        }
    };
    // FN-MYP-010-17·18 : 규격 밖 결과 저장 제외
    if !matches_spec(&character.image) {
        report_failure(&*repo, &job, CharacterFailure::SpecMismatch).await;
        return;
    }
    let original = file_name
        .as_deref()
        .filter(|name| !name.is_empty())
        .map(|name| (name, photo.as_slice()));
    let reported = repo
        .report_success(
            job.char_crt_job_sn,
            &character.image,
            &character.sheet,
            character.layout,
            original,
        )
        .await;
    let pixel_char_sn = match reported {
        Ok(sn) => sn,
        Err(error) => {
            // 보고 실패로 멈춘 작업은 Java가 타임아웃으로 정리
            logs::error(
                LOGGER,
                format!(
                    "캐릭터 성공 보고 실패: charCrtJobSn={}: {}",
                    job.char_crt_job_sn,
                    cause(&error)
                ),
            );
            return;
        }
    };
    // FN-MYP-011-01 : 사자마자 입도록 전 아이템 미리 굽기
    let targets = match repo.list_bake_items().await {
        Ok(targets) => targets,
        Err(error) => {
            logs::error(
                LOGGER,
                format!(
                    "굽기 대상 조회 실패: pixelCharSn={pixel_char_sn}: {}",
                    cause(&error)
                ),
            );
            return;
        }
    };
    let sheet = Bytes::from(character.sheet);
    for item in targets {
        queue.add(BakeTask {
            client: client.clone(),
            repo: repo.clone(),
            pixel_char_sn,
            item_sn: item.item_sn,
            item,
            sheet: Some(sheet.clone()),
            pet_se_cd: Some(job.pet_se_cd.clone()),
        });
    }
    let started = Instant::now();
    queue.drain().await;
    logs::info(
        LOGGER,
        format!(
            "전체 굽기 시간: pixelCharSn={pixel_char_sn}, {:.1}초",
            started.elapsed().as_secs_f64()
        ),
    );
}

/// FN-MYP-011-01·FN-MYP-010-13 : 합성 PNG, 아직 안 구운 아이템이 있으면 None
pub async fn preview<C, R>(
    client: Option<Arc<C>>,
    repo: Arc<R>,
    queue: &BakeQueue<C, R>,
    pixel_char_sn: i64,
    item_sns: &[i64],
) -> Result<Option<Vec<u8>>, ApiError>
where
    C: CharacterClient + 'static,
    R: CharacterJobRepository + 'static,
{
    let targets = repo.list_bake_items().await?;
    let chosen = chosen_items(item_sns, &targets)?;
    let found = repo.find_sheets(pixel_char_sn, item_sns).await?;
    if !found.failed.is_empty() {
        logs::warning(
            LOGGER,
            format!("굽기 실패 아이템 미리보기: pixelCharSn={pixel_char_sn}"),
        );
        return Err(preview_error());
    }
    let missing: Vec<i64> = item_sns
        .iter()
        .copied()
        .filter(|sn| !found.sheets.contains_key(sn))
        .collect();
    if !missing.is_empty() {
        let Some(client) = client else {
            logs::error(LOGGER, "미리보기 불가: 생성기가 아직 연결되지 않았습니다");
            return Err(external_error().into());
        };
        for sn in missing.into_iter().rev() {
            let item = chosen
                .iter()
                .find(|(chosen_sn, _)| *chosen_sn == sn)
                .map(|(_, item)| item.clone())
                .expect("고른 아이템");
            queue.put_first(BakeTask {
                client: client.clone(),
                repo: repo.clone(),
                pixel_char_sn,
                item_sn: sn,
                item,
                sheet: Some(found.sheet.clone()),
                pet_se_cd: Some(found.pet_se_cd.clone()),
            });
        }
        return Ok(None);
    }
    render_chosen(&found, &chosen).await.map(Some)
}

/// FN-MYP-010-16·13 : [장착] 확정 뒤 착용 모습을 Java에 저장, 안 구운 아이템이 있으면 false
pub async fn save_equipped_image<C, R>(
    client: Option<Arc<C>>,
    repo: Arc<R>,
    queue: &BakeQueue<C, R>,
    pixel_char_sn: i64,
    item_sns: &[i64],
) -> Result<bool, ApiError>
where
    C: CharacterClient + 'static,
    R: CharacterJobRepository + 'static,
{
    let Some(image) = preview(client, repo.clone(), queue, pixel_char_sn, item_sns).await? else {
        return Ok(false);
    };
    repo.save_equipped_image(pixel_char_sn, item_sns, &image)
        .await?;
    Ok(true)
}

/// 입혀 볼 아이템 : `dict`처럼 요청 순서, 같은 번호는 처음 자리에 마지막 값
fn chosen_items(item_sns: &[i64], targets: &[BakeItem]) -> Result<Vec<(i64, BakeItem)>, ApiError> {
    let invalid = || -> ApiError {
        AppError::new("INVALID_REQUEST", 400, "입혀 볼 수 없는 아이템입니다.").into()
    };
    let mut chosen: Vec<(i64, BakeItem)> = Vec::new();
    for &item_sn in item_sns {
        let item = targets
            .iter()
            .find(|item| item.item_sn == item_sn)
            .ok_or_else(invalid)?;
        match chosen.iter_mut().find(|(sn, _)| *sn == item_sn) {
            Some(known) => known.1 = item.clone(),
            None => chosen.push((item_sn, item.clone())),
        }
    }
    // FN-MYP-010-13 : 한 부위에 하나
    let regions: HashSet<&str> = chosen
        .iter()
        .map(|(_, item)| item.regn_cd.as_str())
        .collect();
    if chosen.len() != item_sns.len() || regions.len() != chosen.len() {
        return Err(invalid());
    }
    Ok(chosen)
}

/// `_render` : 고른 아이템의 구운 시트를 부위별로 입혀 합성
async fn render_chosen(
    found: &CharacterSheets,
    chosen: &[(i64, BakeItem)],
) -> Result<Vec<u8>, ApiError> {
    let worn = chosen
        .iter()
        .map(|(sn, item)| (item.regn_cd.clone(), found.sheets[sn].to_vec()))
        .collect();
    render(&found.sheet, found.layout, worn).await
}

/// 전체 굽기 한 묶음 결과 수
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemBakeResult {
    pub baked_cnt: usize,
    pub failed_cnt: usize,
    pub last: bool,
}

/// FN-ADM-070-02 : 안 구운 캐릭터 한 묶음을 관리자 쿠키로 굽고 기다림 -> 결과 수
///
/// - 구웠거나 실패 표시한 캐릭터는 Java 목록에서 빠짐 : 매번 첫 페이지만 읽음
/// - 관리자 프론트가 last가 될 때까지 다시 부름 : 요청마다 새 쿠키
pub async fn bake_item_batch<C, R>(
    client: Option<Arc<C>>,
    repo: Arc<R>,
    queue: &Arc<BakeQueue<C, R>>,
    item_sn: i64,
) -> Result<ItemBakeResult, ApiError>
where
    C: CharacterClient + 'static,
    R: CharacterJobRepository + 'static,
{
    let Some(client) = client else {
        logs::error(LOGGER, "전체 굽기 불가: 생성기가 아직 연결되지 않았습니다");
        return Err(external_error().into());
    };
    let Some(item) = repo
        .list_bake_items()
        .await?
        .into_iter()
        .find(|item| item.item_sn == item_sn)
    else {
        return Err(AppError::new("INVALID_REQUEST", 400, "구울 수 없는 아이템입니다.").into());
    };
    let page = repo.list_bake_targets(item_sn).await?;
    // dict.fromkeys : 처음 나온 순서로 중복 제거
    let mut candidates: Vec<i64> = Vec::new();
    for sn in page.pixel_char_sns {
        if !candidates.contains(&sn) {
            candidates.push(sn);
        }
    }
    let targets = &candidates[..candidates.len().min(BAKE_BATCH)];
    // 회원 미리보기(put_first)가 앞서도록 뒤에 추가
    let tasks = targets
        .iter()
        .map(|&sn| BakeTask {
            client: client.clone(),
            repo: repo.clone(),
            pixel_char_sn: sn,
            item_sn,
            item: item.clone(),
            sheet: None,
            pet_se_cd: None,
        })
        .collect();
    let outcomes = queue.run(tasks).await;
    let count = |want: BakeOutcome| outcomes.iter().filter(|o| **o == Some(want)).count();
    let (baked, failed) = (count(BakeOutcome::Baked), count(BakeOutcome::Failed));
    logs::info(
        LOGGER,
        format!(
            "전체 굽기 묶음: itemSn={item_sn}, 대상 {}개, 저장 {baked}, 실패 표시 {failed}",
            targets.len()
        ),
    );
    if !targets.is_empty() && baked + failed == 0 {
        // 하나도 줄지 않음 : 다시 불러도 같은 대상이라 반복을 멈춤
        logs::error(
            LOGGER,
            format!("전체 굽기 묶음이 하나도 처리되지 않았습니다: itemSn={item_sn}"),
        );
        return Err(external_error().into());
    }
    let done = baked + failed == targets.len();
    let last = page.last && candidates.len() <= BAKE_BATCH && done;
    Ok(ItemBakeResult {
        baked_cnt: baked,
        failed_cnt: failed,
        last,
    })
}

async fn report_failure<R: CharacterJobRepository>(
    repo: &R,
    job: &CharacterJob,
    reason: CharacterFailure,
) {
    if repo
        .report_failure(job.char_crt_job_sn, reason)
        .await
        .is_err()
    {
        logs::error(
            LOGGER,
            format!(
                "캐릭터 실패 보고 실패: charCrtJobSn={}, {}",
                job.char_crt_job_sn,
                reason.as_str()
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Layout;
    use crate::clients::OpenAiClient;
    use crate::repository::{BakeTargetPage, CharacterSheets};

    /// 어떤 메서드도 불리면 안 되는 저장소 : 생성기 없음 502는 Java를 부르기 전에 발생
    struct Untouched;

    fn touched<T>() -> Result<T, ApiError> {
        panic!("생성기가 없으면 Java를 부르지 않아야 한다")
    }

    impl CharacterJobRepository for Untouched {
        async fn create_job(&self, _: i64) -> Result<CharacterJob, ApiError> {
            touched()
        }
        async fn regen_start(&self, _: i64) -> Result<CharacterJob, ApiError> {
            touched()
        }
        async fn fetch_original_photo(&self, _: i64) -> Result<Bytes, ApiError> {
            touched()
        }
        async fn report_success(
            &self,
            _: i64,
            _: &[u8],
            _: &[u8],
            _: Option<Layout>,
            _: Option<(&str, &[u8])>,
        ) -> Result<i64, ApiError> {
            touched()
        }
        async fn report_failure(&self, _: i64, _: CharacterFailure) -> Result<(), ApiError> {
            touched()
        }
        async fn list_bake_items(&self) -> Result<Vec<BakeItem>, ApiError> {
            touched()
        }
        async fn fetch_item_image(&self, _: i64) -> Result<Bytes, ApiError> {
            touched()
        }
        async fn list_bake_targets(&self, _: i64) -> Result<BakeTargetPage, ApiError> {
            touched()
        }
        async fn save_item_sheet(&self, _: i64, _: i64, _: &[u8]) -> Result<(), ApiError> {
            touched()
        }
        async fn report_bake_failure(&self, _: i64, _: i64) -> Result<(), ApiError> {
            touched()
        }
        async fn find_sheets(&self, _: i64, _: &[i64]) -> Result<CharacterSheets, ApiError> {
            touched()
        }
        async fn save_equipped_image(&self, _: i64, _: &[i64], _: &[u8]) -> Result<(), ApiError> {
            touched()
        }
    }

    fn external(error: ApiError) -> (u16, String) {
        match error {
            ApiError::App(app) => (app.http_code, app.code),
            other => panic!("AppError여야 함: {other:?}"),
        }
    }

    // 원본 test_regenerate.py `생성기가_없으면_502이고_Java를_부르지_않는다`
    #[tokio::test]
    async fn regenerate_without_generator_is_502_without_calling_java() {
        let queue = Arc::new(BakeQueue::<OpenAiClient, Untouched>::default());

        let error = regenerate(None, Arc::new(Untouched), &queue, 5).unwrap_err();

        assert_eq!(external(error), (502, "EXTERNAL_API_ERROR".into()));
        assert!(queue.pending().is_empty());
    }

    // service.py `bake_item_batch` : 생성기 없음은 아이템 목록 조회 전에 502
    #[tokio::test]
    async fn bake_batch_without_generator_is_502_without_calling_java() {
        let queue = Arc::new(BakeQueue::<OpenAiClient, Untouched>::default());

        let error = bake_item_batch(None, Arc::new(Untouched), &queue, 9)
            .await
            .unwrap_err();

        assert_eq!(external(error), (502, "EXTERNAL_API_ERROR".into()));
        assert!(queue.pending().is_empty());
    }

    #[test]
    fn cause_names_backend_status_and_app_code() {
        use crate::backend::BackendError;
        let backend = ApiError::from(BackendError {
            status_code: 403,
            body: Bytes::from_static(b"{\"code\":\"FORBIDDEN\"}"),
            content_type: None,
            set_cookies: Vec::new(),
        });
        assert_eq!(cause(&backend), "Java 403");
        assert_eq!(cause(&external_error().into()), "EXTERNAL_API_ERROR");
        assert_eq!(cause(&ApiError::Internal), "Internal");
        assert_eq!(cause(&ApiError::Validation), "Validation");
        let http = ApiError::Http {
            status: 405,
            detail: "Method Not Allowed".into(),
            allow: None,
        };
        assert_eq!(cause(&http), "HTTP 405");
    }
}
