//! `service.py`의 관리자 경로 흐름(describe_item·draw_item·trial_preview) : 가짜 생성기·가짜 저장소로 확인
//! 외부 API(OpenAI)는 부르지 않음. 원본 `tests/character/test_items.py`·`test_item_pixel.py`의 순서·오류 사례를 옮김

mod common;

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use common::{app_error, fixture};
use nyangmeong_character::canvas::Layout;
use nyangmeong_character::clients::{
    CharacterApiError, CharacterClient, CharacterRequest, GeneratedCharacter, HEAVY_CONCURRENCY,
    heavy,
};
use nyangmeong_character::engine::BakeItem;
use nyangmeong_character::error::ApiError;
use nyangmeong_character::pil;
use nyangmeong_character::repository::{
    BakeTargetPage, CharacterFailure, CharacterJob, CharacterJobRepository, CharacterSheets,
};
use nyangmeong_character::service;

/// 부른 횟수를 세고 정해 둔 답을 주는 생성기
#[derive(Default)]
struct FakeClient {
    hint: Option<String>,
    drawn: Option<Vec<u8>>,
    worn: Option<Vec<u8>>,
    calls: AtomicUsize,
    bake_args: Mutex<Vec<(String, Option<String>, bool)>>,
}

impl CharacterClient for FakeClient {
    async fn generate(&self, _: CharacterRequest) -> Result<GeneratedCharacter, CharacterApiError> {
        Err(CharacterApiError)
    }

    async fn bake(
        &self,
        _sheet: &[u8],
        pet_se_cd: &str,
        item: &BakeItem,
        item_image: Option<&[u8]>,
    ) -> Result<Vec<u8>, CharacterApiError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.bake_args.lock().unwrap().push((
            pet_se_cd.to_owned(),
            item.bake_hint.clone(),
            item_image.is_some(),
        ));
        self.worn.clone().ok_or(CharacterApiError)
    }

    async fn describe_item(&self, _: &[u8], _: &str, _: &str) -> Result<String, CharacterApiError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.hint.clone().ok_or(CharacterApiError)
    }

    async fn draw_item(&self, _: &[u8], _: &str, _: &str) -> Result<Vec<u8>, CharacterApiError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.drawn.clone().ok_or(CharacterApiError)
    }
}

/// `find_sheets`만 쓰이는 저장소
struct FakeRepo {
    found: Result<CharacterSheets, u16>,
}

fn unused<T>() -> Result<T, ApiError> {
    panic!("이 흐름에서 부르지 않는 메서드")
}

impl CharacterJobRepository for FakeRepo {
    async fn create_job(&self, _: i64) -> Result<CharacterJob, ApiError> {
        unused()
    }
    async fn regen_start(&self, _: i64) -> Result<CharacterJob, ApiError> {
        unused()
    }
    async fn fetch_original_photo(&self, _: i64) -> Result<Bytes, ApiError> {
        unused()
    }
    async fn report_success(
        &self,
        _: i64,
        _: &[u8],
        _: &[u8],
        _: Option<Layout>,
        _: Option<(&str, &[u8])>,
    ) -> Result<i64, ApiError> {
        unused()
    }
    async fn report_failure(&self, _: i64, _: CharacterFailure) -> Result<(), ApiError> {
        unused()
    }
    async fn list_bake_items(&self) -> Result<Vec<BakeItem>, ApiError> {
        unused()
    }
    async fn fetch_item_image(&self, _: i64) -> Result<Bytes, ApiError> {
        unused()
    }
    async fn list_bake_targets(&self, _: i64) -> Result<BakeTargetPage, ApiError> {
        unused()
    }
    async fn save_item_sheet(&self, _: i64, _: i64, _: &[u8]) -> Result<(), ApiError> {
        unused()
    }
    async fn report_bake_failure(&self, _: i64, _: i64) -> Result<(), ApiError> {
        unused()
    }
    async fn find_sheets(&self, _: i64, item_sns: &[i64]) -> Result<CharacterSheets, ApiError> {
        assert!(item_sns.is_empty(), "시험 굽기는 아이템 시트를 받지 않는다");
        match &self.found {
            Ok(found) => Ok(found.clone()),
            Err(status) => {
                Err(nyangmeong_character::response::AppError::new("X", *status, "x").into())
            }
        }
    }
    async fn save_equipped_image(&self, _: i64, _: &[i64], _: &[u8]) -> Result<(), ApiError> {
        unused()
    }
}

fn sheets() -> CharacterSheets {
    CharacterSheets {
        pet_se_cd: "PETC0002".into(),
        sheet: Bytes::from(fixture("sheets/plain/bare.png")),
        layout: Layout {
            scale: 0.5,
            origin_x: 10.5,
            origin_y: 20.0,
        },
        sheets: HashMap::new(),
        failed: HashSet::new(),
    }
}

const PNG_HAT: &str = "items/hat.png";

#[tokio::test]
async fn describe_item_checks_region_then_upload_then_generator() {
    let hat = fixture(PNG_HAT);
    let client = FakeClient {
        hint: Some("a cap".into()),
        ..Default::default()
    };

    // 부위 틀림 : 업로드 검증보다 먼저
    let error = service::describe_item(Some(&client), b"nope", "a.gif", "MYPC0006", "x")
        .await
        .unwrap_err();
    assert_eq!(app_error(error), (400, "INVALID_REQUEST".into()));
    // 업로드 틀림 : 생성기 없음보다 먼저
    let error = service::describe_item(None::<&FakeClient>, b"nope", "a.png", "MYPC0001", "x")
        .await
        .unwrap_err();
    assert_eq!(app_error(error), (400, "FILE_TYPE_NOT_ALLOWED".into()));
    // 생성기 없음 : 502
    let error = service::describe_item(None::<&FakeClient>, &hat, "a.png", "MYPC0001", "x")
        .await
        .unwrap_err();
    assert_eq!(app_error(error), (502, "EXTERNAL_API_ERROR".into()));
    assert_eq!(client.calls.load(Ordering::SeqCst), 0);

    let hint = service::describe_item(Some(&client), &hat, "a.png", "MYPC0001", "x")
        .await
        .unwrap();
    assert_eq!(hint, "a cap");
}

#[tokio::test]
async fn describe_item_generator_failure_is_external_error() {
    let hat = fixture(PNG_HAT);
    let client = FakeClient::default();

    let error = service::describe_item(Some(&client), &hat, "a.png", "MYPC0001", "x")
        .await
        .unwrap_err();

    assert_eq!(app_error(error), (502, "EXTERNAL_API_ERROR".into()));
}

#[tokio::test]
async fn draw_item_cuts_the_drawing_or_fails_with_item_image_error() {
    let hat = fixture(PNG_HAT);
    let ok = FakeClient {
        drawn: Some(hat.clone()),
        ..Default::default()
    };
    let png = service::draw_item(Some(&ok), &hat, "a.png", "MYPC0001", "x")
        .await
        .unwrap();
    // PNG 바이트는 압축기가 달라 다름(알려진 차이) : 화소로 비교
    let got = pil::open_rgba(&png).unwrap();
    let gold = pil::open_rgba(&fixture("items/hat.gold.png")).unwrap();
    assert!(got == gold, "cut_item 정답과 같은 화소여야 함");

    for drawn in [fixture("items/empty.png"), b"hello".to_vec()] {
        let client = FakeClient {
            drawn: Some(drawn),
            ..Default::default()
        };
        let error = service::draw_item(Some(&client), &hat, "a.png", "MYPC0001", "x")
            .await
            .unwrap_err();
        assert_eq!(app_error(error), (502, "CHARACTER_API_ERROR".into()));
    }
    let failing = FakeClient::default();
    let error = service::draw_item(Some(&failing), &hat, "a.png", "MYPC0001", "x")
        .await
        .unwrap_err();
    assert_eq!(app_error(error), (502, "CHARACTER_API_ERROR".into()));
}

#[tokio::test]
async fn trial_preview_bakes_with_the_hint_and_renders() {
    let client = FakeClient {
        worn: Some(fixture("sheets/plain/worn_MYPC0002.png")),
        ..Default::default()
    };
    let repo = FakeRepo {
        found: Ok(sheets()),
    };

    let png = service::trial_preview(Some(&client), &repo, 77, "MYPC0002", "round glasses")
        .await
        .unwrap();

    assert!(png.starts_with(b"\x89PNG"));
    assert_eq!(
        client.bake_args.lock().unwrap().as_slice(),
        [(
            "PETC0002".to_owned(),
            Some("round glasses".to_owned()),
            false
        )]
    );
}

#[tokio::test]
async fn trial_preview_failures() {
    let repo = FakeRepo {
        found: Ok(sheets()),
    };
    // 부위 틀림이 먼저
    let client = FakeClient::default();
    let error = service::trial_preview(Some(&client), &repo, 77, "MYPC0006", "h")
        .await
        .unwrap_err();
    assert_eq!(app_error(error), (400, "INVALID_REQUEST".into()));
    // 생성기 없음
    let error = service::trial_preview(None::<&FakeClient>, &repo, 77, "MYPC0002", "h")
        .await
        .unwrap_err();
    assert_eq!(app_error(error), (502, "EXTERNAL_API_ERROR".into()));
    // 굽기 실패·구운 시트를 쓸 수 없음 : 둘 다 미리보기 502
    for worn in [None, Some(fixture("sheets/plain/bare.png"))] {
        let client = FakeClient {
            worn,
            ..Default::default()
        };
        let error = service::trial_preview(Some(&client), &repo, 77, "MYPC0002", "h")
            .await
            .unwrap_err();
        assert_eq!(app_error(error), (502, "CHARACTER_API_ERROR".into()));
    }
    // Java 쪽 오류는 그대로
    let missing = FakeRepo { found: Err(404) };
    let error = service::trial_preview(Some(&client_with_worn()), &missing, 77, "MYPC0002", "h")
        .await
        .unwrap_err();
    assert_eq!(app_error(error).0, 404);
}

fn client_with_worn() -> FakeClient {
    FakeClient {
        worn: Some(fixture("sheets/plain/worn_MYPC0002.png")),
        ..Default::default()
    }
}

#[tokio::test]
async fn heavy_runs_at_most_the_limit_at_once() {
    let running = std::sync::Arc::new(AtomicUsize::new(0));
    let peak = std::sync::Arc::new(AtomicUsize::new(0));
    let jobs: Vec<_> = (0..8)
        .map(|_| {
            let (running, peak) = (running.clone(), peak.clone());
            tokio::spawn(async move {
                heavy(move || {
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(40));
                    running.fetch_sub(1, Ordering::SeqCst);
                })
                .await
                .unwrap();
            })
        })
        .collect();
    for job in jobs {
        job.await.unwrap();
    }
    assert_eq!(peak.load(Ordering::SeqCst), HEAVY_CONCURRENCY);
}
