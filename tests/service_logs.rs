//! 생성 뒤 Java 실패 로그의 이유(원본에 없는 것, `migration.md` "알려진 차이")
//! : `캐릭터 성공 보고 실패`·`굽기 대상 조회 실패` 줄 끝에 Java 4xx 상태 또는 오류 코드

mod common;

use std::sync::Arc;

use bytes::Bytes;
use common::*;
use nyangmeong_character::backend::external_error;
use nyangmeong_character::canvas::Layout;
use nyangmeong_character::engine::BakeItem;
use nyangmeong_character::error::ApiError;
use nyangmeong_character::logs;
use nyangmeong_character::queue::BakeQueue;
use nyangmeong_character::repository::{
    BakeTargetPage, CharacterFailure, CharacterJob, CharacterJobRepository, CharacterSheets,
};
use nyangmeong_character::service;

/// 굽기 대상 응답 모양이 틀린 Java : `list_bake_items`만 502(EXTERNAL_API_ERROR), 나머지는 FakeRepo
struct ShapeRepo(FakeRepo);

impl CharacterJobRepository for ShapeRepo {
    async fn create_job(&self, pet_sn: i64) -> Result<CharacterJob, ApiError> {
        self.0.create_job(pet_sn).await
    }
    async fn regen_start(&self, sn: i64) -> Result<CharacterJob, ApiError> {
        self.0.regen_start(sn).await
    }
    async fn fetch_original_photo(&self, sn: i64) -> Result<Bytes, ApiError> {
        self.0.fetch_original_photo(sn).await
    }
    async fn report_success(
        &self,
        sn: i64,
        image: &[u8],
        sheet: &[u8],
        layout: Option<Layout>,
        original: Option<(&str, &[u8])>,
    ) -> Result<i64, ApiError> {
        self.0
            .report_success(sn, image, sheet, layout, original)
            .await
    }
    async fn report_failure(&self, sn: i64, reason: CharacterFailure) -> Result<(), ApiError> {
        self.0.report_failure(sn, reason).await
    }
    async fn list_bake_items(&self) -> Result<Vec<BakeItem>, ApiError> {
        Err(external_error().into())
    }
    async fn fetch_item_image(&self, item_sn: i64) -> Result<Bytes, ApiError> {
        self.0.fetch_item_image(item_sn).await
    }
    async fn list_bake_targets(&self, item_sn: i64) -> Result<BakeTargetPage, ApiError> {
        self.0.list_bake_targets(item_sn).await
    }
    async fn save_item_sheet(&self, sn: i64, item_sn: i64, sheet: &[u8]) -> Result<(), ApiError> {
        self.0.save_item_sheet(sn, item_sn, sheet).await
    }
    async fn report_bake_failure(&self, sn: i64, item_sn: i64) -> Result<(), ApiError> {
        self.0.report_bake_failure(sn, item_sn).await
    }
    async fn find_sheets(&self, sn: i64, item_sns: &[i64]) -> Result<CharacterSheets, ApiError> {
        self.0.find_sheets(sn, item_sns).await
    }
    async fn save_equipped_image(
        &self,
        sn: i64,
        item_sns: &[i64],
        image: &[u8],
    ) -> Result<(), ApiError> {
        self.0.save_equipped_image(sn, item_sns, image).await
    }
}

/// 라우터 순서 : create_job -> (응답 뒤) generate_and_report
async fn create<R: CharacterJobRepository + 'static>(repo: R) {
    let (client, repo) = (Arc::new(FakeClient::default()), Arc::new(repo));
    let job = service::create_job(Some(&*client), &*repo, PHOTO, "pet.png", 3)
        .await
        .expect("작업 생성");
    service::generate_and_report(
        client,
        repo,
        Arc::new(BakeQueue::default()),
        job,
        PHOTO.to_vec(),
        Some("pet.png".into()),
    )
    .await;
}

/// 전역 로그 파일을 쓰므로 사례를 한 테스트에서 차례로
#[tokio::test]
async fn java_failure_after_generation_logs_reason() {
    let folder = std::env::temp_dir().join(format!("nm-service-logs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    logs::configure(folder.to_str().unwrap(), Box::new(logs::local_today));
    let path = folder.join(format!("character_{}.log", logs::local_today()));

    // Java 4xx : 굽기 대상 조회 401
    create(FakeRepo {
        list_items_error: true,
        ..FakeRepo::default()
    })
    .await;
    assert_eq!(
        error_lines(&path, "굽기 대상 조회 실패"),
        ["굽기 대상 조회 실패: pixelCharSn=1: Java 401"]
    );

    // 응답 모양 다름 : 502 오류 코드
    create(ShapeRepo(FakeRepo::default())).await;
    assert_eq!(
        error_lines(&path, "굽기 대상 조회 실패"),
        [
            "굽기 대상 조회 실패: pixelCharSn=1: Java 401",
            "굽기 대상 조회 실패: pixelCharSn=1: EXTERNAL_API_ERROR",
        ]
    );

    // 성공 보고 실패 : Java 500을 Java 4xx와 같은 꼴로 받은 가짜(상태 숫자 그대로)
    create(FakeRepo {
        success_error: true,
        ..FakeRepo::default()
    })
    .await;
    assert_eq!(
        error_lines(&path, "캐릭터 성공 보고 실패"),
        ["캐릭터 성공 보고 실패: charCrtJobSn=1: Java 500"]
    );

    logs::configure("", Box::new(logs::local_today));
    let _ = std::fs::remove_dir_all(&folder);
}
