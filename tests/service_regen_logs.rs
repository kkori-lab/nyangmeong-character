//! 관리자 무료 재생성 실패 로그의 이유(원본에 없는 것, `migration.md` "알려진 차이")
//! : `재생성 시작 실패`·`재생성 원본 사진 조회 실패` 줄 끝에 Java 4xx 상태 또는 오류 코드
//! 전역 로그 파일을 쓰므로 `service_member.rs`의 재생성 테스트와 다른 실행 파일로 둠

mod common;

use common::*;
use nyangmeong_character::logs;
use nyangmeong_character::repository::CharacterFailure;

/// 전역 로그 파일을 쓰므로 사례를 한 테스트에서 차례로
#[tokio::test]
async fn regenerate_failure_logs_reason() {
    let folder = std::env::temp_dir().join(format!("nm-regen-logs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    logs::configure(folder.to_str().unwrap(), Box::new(logs::local_today));
    let path = folder.join(format!("character_{}.log", logs::local_today()));

    // 이미 처리됨 : regen-start 409 -> 이유 한 줄, 생성·보고 없음
    let (client, repo) = regenerate(
        FakeClient::default(),
        FakeRepo {
            regen_start_error: Some(backend_error(409, br#"{"code":"ALREADY_PROCESSED"}"#)),
            ..regen_repo()
        },
    )
    .await;
    assert_eq!(
        error_lines(&path, "재생성 시작 실패"),
        ["재생성 시작 실패: charCrtJobSn=5: Java 409"]
    );
    assert!(client.generate_calls.lock().unwrap().is_empty());
    assert_eq!(repo.job_count(), 0);

    // 원본 사진 없음 : original-photo 404 -> 이유 한 줄, INPUT_ERROR 보고
    let mut missing = regen_repo();
    missing.original_photos.clear();
    let (client, repo) = regenerate(FakeClient::default(), missing).await;
    assert_eq!(
        error_lines(&path, "재생성 원본 사진 조회 실패"),
        ["재생성 원본 사진 조회 실패: charCrtJobSn=5: Java 404"]
    );
    assert!(client.generate_calls.lock().unwrap().is_empty());
    assert_eq!(repo.saved(5).failure, Some(CharacterFailure::InputError));

    logs::configure("", Box::new(logs::local_today));
    let _ = std::fs::remove_dir_all(&folder);
}
