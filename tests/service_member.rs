//! `service.py`의 회원·재생성 흐름(create_job·generate_and_report·preview·save_equipped_image·regenerate_and_report)
//! : 원본 `test_create.py`·`test_create_without_generator.py`·`test_preview.py`·`test_equipped_image.py`·`test_regenerate.py`의
//! 서비스 수준 사례를 옮김. 인증·X-Requested-With·본문 값 검증 같은 경로 수준 사례는 대조(`contract/`)에서 확인

mod common;

use std::sync::Arc;

use bytes::Bytes;
use common::*;
use nyangmeong_character::clients::GeneratedCharacter;
use nyangmeong_character::error::ApiError;
use nyangmeong_character::queue::BakeQueue;
use nyangmeong_character::repository::{CharacterFailure, CharacterJob};
use nyangmeong_character::service;

type Queue = BakeQueue<FakeClient, FakeRepo>;

const PHOTO: &[u8] = b"\x89PNG\r\n\x1a\n0000000000000000";

fn info(sn: i64) -> CharacterJob {
    CharacterJob {
        char_crt_job_sn: sn,
        pet_se_cd: "PETC0002".into(),
        vrty_nm: Some("코리안숏헤어".into()),
        colr_nm: Some("고등어".into()),
        chrc_cn: Some("귀 접힘".into()),
    }
}

fn status(error: ApiError) -> (u16, String) {
    match error {
        ApiError::App(app) => (app.http_code, app.code),
        ApiError::Backend(backend) => {
            let body: serde_json::Value = serde_json::from_slice(&backend.body).unwrap();
            (
                backend.status_code,
                body["code"].as_str().unwrap().to_owned(),
            )
        }
        other => panic!("예상 밖 오류 {other:?}"),
    }
}

fn generated(image: Vec<u8>) -> Generated {
    Generated::Ok(GeneratedCharacter {
        image,
        sheet: Vec::new(),
        layout: None,
    })
}

// FN-MYP-010-08·09 : 작업 생성 -> 응답 뒤 생성·보고

/// 라우터 순서 : create_job -> (응답 뒤) generate_and_report
async fn create(
    client: Option<FakeClient>,
    repo: FakeRepo,
    photo: &[u8],
    name: &str,
) -> (
    Result<CharacterJob, ApiError>,
    Option<Arc<FakeClient>>,
    Arc<FakeRepo>,
) {
    let (client, repo) = (client.map(Arc::new), Arc::new(repo));
    let created = service::create_job(client.as_deref(), &*repo, photo, name, 3).await;
    if let (Ok(job), Some(client)) = (&created, &client) {
        service::generate_and_report(
            client.clone(),
            repo.clone(),
            Arc::new(Queue::default()),
            job.clone(),
            photo.to_vec(),
            Some(name.to_owned()),
        )
        .await;
    }
    (created, client, repo)
}

fn info_repo() -> FakeRepo {
    FakeRepo {
        job_info: Some(info(0)),
        ..FakeRepo::default()
    }
}

#[tokio::test]
async fn create_reports_success_with_512_png_and_original_photo() {
    let (created, client, repo) = create(
        Some(FakeClient::generating(generated(png(512, 512)))),
        info_repo(),
        PHOTO,
        "dog.png",
    )
    .await;

    assert_eq!(created.unwrap().char_crt_job_sn, 1);
    let saved = repo.saved(1);
    assert_eq!(saved.pet_sn, 3);
    assert!(service::matches_spec(saved.image.as_deref().unwrap()));
    assert_eq!(saved.failure, None);
    assert_eq!(saved.original, Some(("dog.png".into(), PHOTO.to_vec())));
    // 생성기는 Java가 준 등록 정보로 생성
    let calls = client.unwrap().generate_calls.lock().unwrap().clone();
    let request = &calls[0];
    assert_eq!(request.photo, PHOTO);
    assert_eq!(
        (
            request.pet_se_cd.as_str(),
            request.vrty_nm.as_deref(),
            request.colr_nm.as_deref(),
            request.chrc_cn.as_deref()
        ),
        (
            "PETC0002",
            Some("코리안숏헤어"),
            Some("고등어"),
            Some("귀 접힘")
        )
    );
}

#[tokio::test]
async fn create_passes_java_rejection_up_and_does_not_generate() {
    let repo = FakeRepo {
        create_error: Some(backend_error(409, br#"{"code":"CONFLICT"}"#)),
        ..FakeRepo::default()
    };

    let (created, client, _) = create(Some(FakeClient::default()), repo, PHOTO, "p.png").await;

    assert_eq!(status(created.unwrap_err()), (409, "CONFLICT".into()));
    assert!(client.unwrap().generate_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn create_without_generator_is_502_after_upload_checks() {
    // 업로드 검증이 먼저 : 생성기가 없어도 PDF는 400
    let (created, _, repo) = create(None, FakeRepo::default(), b"%PDF-1.7", "p.pdf").await;
    assert_eq!(
        status(created.unwrap_err()),
        (400, "FILE_TYPE_NOT_ALLOWED".into())
    );
    assert_eq!(repo.job_count(), 0);

    let (created, _, repo) = create(None, FakeRepo::default(), PHOTO, "p.png").await;
    assert_eq!(
        status(created.unwrap_err()),
        (502, "EXTERNAL_API_ERROR".into())
    );
    assert_eq!(repo.job_count(), 0);
}

#[tokio::test]
async fn create_rejects_bad_uploads_before_java() {
    let big = [PHOTO, &vec![b'0'; 10 * 1024 * 1024]].concat();
    let cases: [(&[u8], &str, &str); 4] = [
        (b"not an image", "p.png", "FILE_TYPE_NOT_ALLOWED"),
        (PHOTO, "p.jpg", "FILE_TYPE_NOT_ALLOWED"),
        (b"%PDF-1.7", "p.pdf", "FILE_TYPE_NOT_ALLOWED"),
        (&big, "p.png", "FILE_SIZE_EXCEEDED"),
    ];
    for (photo, name, code) in cases {
        let (created, client, repo) = create(
            Some(FakeClient::default()),
            FakeRepo::default(),
            photo,
            name,
        )
        .await;

        assert_eq!(status(created.unwrap_err()), (400, code.into()), "{name}");
        assert_eq!(repo.job_count(), 0);
        assert!(client.unwrap().generate_calls.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn off_spec_or_failed_generation_is_reported_as_failure() {
    let cases = [
        (generated(png(256, 256)), CharacterFailure::SpecMismatch),
        (
            generated(b"not a png".to_vec()),
            CharacterFailure::SpecMismatch,
        ),
        (Generated::Fail, CharacterFailure::ApiError),
    ];
    for (answer, reason) in cases {
        let (_, _, repo) = create(
            Some(FakeClient::generating(answer)),
            FakeRepo::default(),
            PHOTO,
            "p.png",
        )
        .await;

        let saved = repo.saved(1);
        assert_eq!(saved.failure, Some(reason));
        assert_eq!(saved.image, None);
    }
}

#[tokio::test]
async fn broken_success_report_stops_quietly() {
    let repo = FakeRepo {
        success_error: true,
        items: vec![item(1, "MYPC0004")],
        ..info_repo()
    };

    let (created, client, _) = create(Some(FakeClient::default()), repo, PHOTO, "p.png").await;

    assert!(created.is_ok());
    assert!(client.unwrap().calls().is_empty());
}

#[test]
fn spec_is_a_512_by_512_png() {
    assert!(service::matches_spec(&png(512, 512)));
    assert!(!service::matches_spec(&png(512, 511)));
    assert!(!service::matches_spec(b"\x89PNG\r\n\x1a\n"));
}

// FN-MYP-011-01·FN-MYP-010-13 : 미리보기, FN-MYP-010-16 : 대표 이미지 저장

const SWEATER: i64 = 108;
const HOODIE: i64 = 111;
const TIE: i64 = 11;
const BOOTS: i64 = 21;

fn shop() -> FakeRepo {
    let repo = FakeRepo::with_items(vec![
        item(SWEATER, "MYPC0004"),
        item(HOODIE, "MYPC0004"),
        item(TIE, "MYPC0003"),
    ]);
    repo.insert(SavedJob::with_sheet(1));
    repo
}

fn bake_sheet(repo: &FakeRepo, item_sn: i64, sheet: Vec<u8>) {
    repo.jobs
        .lock()
        .unwrap()
        .get_mut(&1)
        .unwrap()
        .item_sheets
        .insert(item_sn, Bytes::from(sheet));
}

struct Shop {
    client: Option<Arc<FakeClient>>,
    repo: Arc<FakeRepo>,
    queue: Arc<Queue>,
}

impl Shop {
    fn new(client: Option<FakeClient>, repo: FakeRepo) -> Self {
        Self {
            client: client.map(Arc::new),
            repo: Arc::new(repo),
            queue: Arc::new(Queue::default()),
        }
    }

    async fn preview(&self, sn: i64, item_sns: &[i64]) -> Result<Option<Vec<u8>>, ApiError> {
        service::preview(
            self.client.clone(),
            self.repo.clone(),
            &self.queue,
            sn,
            item_sns,
        )
        .await
    }

    async fn save(&self, sn: i64, item_sns: &[i64]) -> Result<bool, ApiError> {
        service::save_equipped_image(
            self.client.clone(),
            self.repo.clone(),
            &self.queue,
            sn,
            item_sns,
        )
        .await
    }

    fn equipped(&self) -> Option<(Vec<i64>, Vec<u8>)> {
        self.repo.saved(1).equipped
    }
}

#[tokio::test]
async fn preview_of_nothing_is_the_bare_512_png() {
    let shop = Shop::new(Some(FakeClient::default()), shop());

    let png = shop.preview(1, &[]).await.unwrap().unwrap();

    assert!(service::matches_spec(&png));
}

#[tokio::test]
async fn preview_wears_baked_items() {
    let repo = shop();
    bake_sheet(&repo, SWEATER, worn("MYPC0004"));
    bake_sheet(&repo, TIE, worn("MYPC0003"));
    let shop = Shop::new(Some(FakeClient::default()), repo);

    let png = shop.preview(1, &[SWEATER, TIE]).await.unwrap().unwrap();

    assert!(service::matches_spec(&png));
    assert_ne!(png, shop.preview(1, &[]).await.unwrap().unwrap());
}

#[tokio::test]
async fn unbaked_item_is_queued_first_and_baked_after_drain() {
    let shop = Shop::new(Some(FakeClient::default()), shop());

    assert_eq!(shop.preview(1, &[SWEATER]).await.unwrap(), None);
    assert_eq!(shop.queue.pending(), [(1, SWEATER)]);
    shop.queue.drain().await;

    assert_eq!(shop.client.as_ref().unwrap().calls(), [SWEATER]);
    assert!(shop.preview(1, &[SWEATER]).await.unwrap().is_some());
}

#[tokio::test]
async fn several_unbaked_items_keep_request_order_at_the_front() {
    let shop = Shop::new(Some(FakeClient::default()), shop());
    shop.queue.add(BakeTaskOf::other(&shop));

    assert_eq!(shop.preview(1, &[SWEATER, TIE]).await.unwrap(), None);

    assert_eq!(shop.queue.pending(), [(1, SWEATER), (1, TIE), (2, HOODIE)]);
}

/// 큐에 먼저 들어 있던 다른 작업
struct BakeTaskOf;

impl BakeTaskOf {
    fn other(shop: &Shop) -> nyangmeong_character::queue::BakeTask<FakeClient, FakeRepo> {
        nyangmeong_character::queue::BakeTask {
            client: shop.client.clone().unwrap(),
            repo: shop.repo.clone(),
            pixel_char_sn: 2,
            item_sn: HOODIE,
            item: item(HOODIE, "MYPC0004"),
            sheet: None,
            pet_se_cd: None,
        }
    }
}

#[tokio::test]
async fn failed_bake_makes_preview_and_save_502() {
    for save in [false, true] {
        let shop = Shop::new(Some(FakeClient::default()), shop());
        shop.client
            .as_ref()
            .unwrap()
            .error
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let first = if save {
            shop.save(1, &[SWEATER])
                .await
                .map(|saved| saved.then_some(Vec::new()))
        } else {
            shop.preview(1, &[SWEATER]).await
        };
        assert_eq!(first.unwrap(), None);
        shop.queue.drain().await;
        let second = if save {
            shop.save(1, &[SWEATER]).await.map(|_| None)
        } else {
            shop.preview(1, &[SWEATER]).await
        };

        assert_eq!(
            status(second.unwrap_err()),
            (502, "CHARACTER_API_ERROR".into())
        );
        assert_eq!(shop.equipped(), None);
    }
}

#[tokio::test]
async fn broken_item_sheet_is_502() {
    let repo = shop();
    bake_sheet(&repo, SWEATER, b"not image".to_vec());
    let shop = Shop::new(Some(FakeClient::default()), repo);

    let error = shop.preview(1, &[SWEATER]).await.unwrap_err();
    assert_eq!(status(error), (502, "CHARACTER_API_ERROR".into()));
    let error = shop.save(1, &[SWEATER]).await.unwrap_err();
    assert_eq!(status(error), (502, "CHARACTER_API_ERROR".into()));
    assert_eq!(shop.equipped(), None);
}

#[tokio::test]
async fn unbaked_item_without_generator_is_502() {
    let shop = Shop::new(None, shop());

    let error = shop.preview(1, &[SWEATER]).await.unwrap_err();
    assert_eq!(status(error), (502, "EXTERNAL_API_ERROR".into()));
    let error = shop.save(1, &[SWEATER]).await.unwrap_err();
    assert_eq!(status(error), (502, "EXTERNAL_API_ERROR".into()));
    assert_eq!(shop.equipped(), None);
}

#[tokio::test]
async fn items_that_cannot_be_worn_together_are_400() {
    let shop = Shop::new(Some(FakeClient::default()), shop());
    // 신발 미지원·굽기 대상 아님·같은 부위 2개·같은 아이템 2번
    for item_sns in [
        vec![BOOTS],
        vec![999],
        vec![SWEATER, HOODIE],
        vec![SWEATER, SWEATER],
    ] {
        let error = shop.preview(1, &item_sns).await.unwrap_err();
        assert_eq!(
            status(error),
            (400, "INVALID_REQUEST".into()),
            "{item_sns:?}"
        );
        let error = shop.save(1, &item_sns).await.unwrap_err();
        assert_eq!(
            status(error),
            (400, "INVALID_REQUEST".into()),
            "{item_sns:?}"
        );
    }
    assert_eq!(shop.equipped(), None);
}

#[tokio::test]
async fn missing_character_is_java_404_as_is() {
    let shop = Shop::new(Some(FakeClient::default()), shop());

    let error = shop.preview(99, &[]).await.unwrap_err();
    assert_eq!(status(error), (404, "PIXEL_CHARACTER_NOT_FOUND".into()));
    let error = shop.save(99, &[]).await.unwrap_err();
    assert_eq!(status(error), (404, "PIXEL_CHARACTER_NOT_FOUND".into()));
}

#[tokio::test]
async fn save_stores_the_same_png_as_preview() {
    let repo = shop();
    bake_sheet(&repo, SWEATER, worn("MYPC0004"));
    bake_sheet(&repo, TIE, worn("MYPC0003"));
    let shop = Shop::new(Some(FakeClient::default()), repo);

    assert!(shop.save(1, &[SWEATER, TIE]).await.unwrap());

    let (item_sns, image) = shop.equipped().unwrap();
    assert_eq!(item_sns, [SWEATER, TIE]);
    assert!(service::matches_spec(&image));
    assert_eq!(
        image,
        shop.preview(1, &[SWEATER, TIE]).await.unwrap().unwrap()
    );

    assert!(shop.save(1, &[]).await.unwrap());
    let (item_sns, image) = shop.equipped().unwrap();
    assert!(item_sns.is_empty());
    assert_eq!(image, shop.preview(1, &[]).await.unwrap().unwrap());
}

#[tokio::test]
async fn unbaked_item_is_not_saved_until_baked() {
    let shop = Shop::new(Some(FakeClient::default()), shop());

    assert!(!shop.save(1, &[SWEATER]).await.unwrap());
    assert_eq!(shop.equipped(), None);
    shop.queue.drain().await;

    assert!(shop.save(1, &[SWEATER]).await.unwrap());
    assert_eq!(shop.equipped().unwrap().0, [SWEATER]);
}

#[tokio::test]
async fn java_4xx_on_save_is_passed_up() {
    for (code, body) in [
        (409, &br#"{"code":"EQUIPPED_ITEMS_CHANGED"}"#[..]),
        (404, &br#"{"code":"PIXEL_CHARACTER_NOT_FOUND"}"#[..]),
    ] {
        let repo = FakeRepo {
            equip_error: Some(backend_error(code, body)),
            ..shop()
        };
        let shop = Shop::new(Some(FakeClient::default()), repo);

        let error = shop.save(1, &[]).await.unwrap_err();

        assert_eq!(status(error).0, code);
    }
}

// FN-MYP-010-23 : 관리자 무료 재생성 (응답 뒤 흐름)

fn regen_repo() -> FakeRepo {
    let mut repo = FakeRepo::default();
    repo.regen_jobs.insert(5, info(5));
    repo.original_photos.insert(5, Bytes::from_static(PHOTO));
    repo
}

async fn regenerate(client: FakeClient, repo: FakeRepo) -> (Arc<FakeClient>, Arc<FakeRepo>) {
    let (client, repo) = (Arc::new(client), Arc::new(repo));
    service::regenerate_and_report(client.clone(), repo.clone(), Arc::new(Queue::default()), 5)
        .await;
    (client, repo)
}

#[tokio::test]
async fn regenerate_starts_fetches_photo_generates_and_reports_in_order() {
    let (client, repo) = regenerate(FakeClient::default(), regen_repo()).await;

    assert_eq!(*repo.regen_calls.lock().unwrap(), ["start", "photo"]);
    let calls = client.generate_calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].photo, PHOTO);
    assert_eq!(
        (
            calls[0].pet_se_cd.as_str(),
            calls[0].vrty_nm.as_deref(),
            calls[0].chrc_cn.as_deref()
        ),
        ("PETC0002", Some("코리안숏헤어"), Some("귀 접힘"))
    );
    let saved = repo.saved(5);
    assert!(saved.image.is_some());
    assert_eq!(saved.failure, None);
    // 재생성 success에는 원본을 보내지 않음(Java가 무시)
    assert_eq!(saved.original, None);
}

#[tokio::test]
async fn regenerate_reports_off_spec_or_failed_generation() {
    for (answer, reason) in [
        (generated(png(256, 256)), CharacterFailure::SpecMismatch),
        (Generated::Fail, CharacterFailure::ApiError),
    ] {
        let (_, repo) = regenerate(FakeClient::generating(answer), regen_repo()).await;

        let saved = repo.saved(5);
        assert_eq!(saved.failure, Some(reason));
        assert_eq!(saved.image, None);
    }
}

#[tokio::test]
async fn rejected_start_neither_generates_nor_reports() {
    for code in [404, 409] {
        let repo = FakeRepo {
            regen_start_error: Some(backend_error(code, b"{}")),
            ..regen_repo()
        };

        let (client, repo) = regenerate(FakeClient::default(), repo).await;

        assert_eq!(*repo.regen_calls.lock().unwrap(), ["start"]);
        assert!(client.generate_calls.lock().unwrap().is_empty());
        assert_eq!(repo.job_count(), 0);
    }
}

#[tokio::test]
async fn missing_original_photo_is_reported_as_input_error() {
    let mut repo = regen_repo();
    repo.original_photos.clear();

    let (client, repo) = regenerate(FakeClient::default(), repo).await;

    assert!(client.generate_calls.lock().unwrap().is_empty());
    assert_eq!(repo.saved(5).failure, Some(CharacterFailure::InputError));
}

#[tokio::test]
async fn regenerated_character_gets_every_item_baked() {
    let repo = FakeRepo {
        items: vec![item(1, "MYPC0004"), item(2, "MYPC0003")],
        ..regen_repo()
    };

    let (client, repo) = regenerate(FakeClient::default(), repo).await;

    assert_eq!(client.calls().len(), 2);
    assert_eq!(repo.saved(5).item_sheets.len(), 2);
}

#[tokio::test]
async fn bake_item_lookup_failure_does_not_escape() {
    let repo = FakeRepo {
        list_items_error: true,
        ..regen_repo()
    };

    let (client, repo) = regenerate(FakeClient::default(), repo).await;

    assert!(repo.saved(5).image.is_some());
    assert!(client.calls().is_empty());
}
