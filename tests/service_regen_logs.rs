//! 관리자 무료 재생성 실패 로그의 이유(원본에 없는 것, `migration.md` "알려진 차이")
//! : `재생성 시작 실패`·`재생성 원본 사진 조회 실패` 줄 끝에 Java 4xx 상태 또는 오류 코드
//! 전역 로그 파일을 쓰므로 `service_member.rs`의 재생성 테스트와 다른 실행 파일로 둔다

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use common::*;
use nyangmeong_character::logs;
use nyangmeong_character::queue::BakeQueue;
use nyangmeong_character::repository::{CharacterFailure, CharacterJob};
use nyangmeong_character::service;

const PHOTO: &[u8] = b"\x89PNG\r\n\x1a\n0000000000000000";

fn regen_repo() -> FakeRepo {
    let mut repo = FakeRepo::default();
    repo.regen_jobs.insert(
        5,
        CharacterJob {
            char_crt_job_sn: 5,
            pet_se_cd: "PETC0002".into(),
            vrty_nm: Some("코리안숏헤어".into()),
            colr_nm: Some("고등어".into()),
            chrc_cn: Some("귀 접힘".into()),
        },
    );
    repo.original_photos.insert(5, Bytes::from_static(PHOTO));
    repo
}

async fn regenerate(repo: FakeRepo) -> (Arc<FakeClient>, Arc<FakeRepo>) {
    let (client, repo) = (Arc::new(FakeClient::default()), Arc::new(repo));
    service::regenerate_and_report(
        client.clone(),
        repo.clone(),
        Arc::new(BakeQueue::default()),
        5,
    )
    .await;
    (client, repo)
}

fn error_lines(path: &PathBuf, prefix: &str) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once(" ERROR app.character.service - "))
        .map(|(_, message)| message.to_owned())
        .filter(|message| message.starts_with(prefix))
        .collect()
}

/// 전역 로그 파일을 쓰므로 사례를 한 테스트에서 차례로
#[tokio::test]
async fn regenerate_failure_logs_reason() {
    let folder = std::env::temp_dir().join(format!("nm-regen-logs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    logs::configure(folder.to_str().unwrap(), Box::new(logs::local_today));
    let path = folder.join(format!("character_{}.log", logs::local_today()));

    // 이미 처리됨 : regen-start 409 -> 이유 한 줄, 생성·보고 없음
    let (client, repo) = regenerate(FakeRepo {
        regen_start_error: Some(backend_error(409, br#"{"code":"ALREADY_PROCESSED"}"#)),
        ..regen_repo()
    })
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
    let (client, repo) = regenerate(missing).await;
    assert_eq!(
        error_lines(&path, "재생성 원본 사진 조회 실패"),
        ["재생성 원본 사진 조회 실패: charCrtJobSn=5: Java 404"]
    );
    assert!(client.generate_calls.lock().unwrap().is_empty());
    assert_eq!(repo.saved(5).failure, Some(CharacterFailure::InputError));

    logs::configure("", Box::new(logs::local_today));
    let _ = std::fs::remove_dir_all(&folder);
}
