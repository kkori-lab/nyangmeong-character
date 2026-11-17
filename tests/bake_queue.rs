//! `service.py` 굽기 큐(`BakeQueue`·`_bake_one`·`_draw`·`bake_item_batch`) : 가짜 생성기·가짜 저장소로 확인한다.
//! 원본 `tests/character/test_bake.py`의 큐 사례를 옮기고, 러스트에서 새로 생긴 동시성(잠금 하나)을 더 본다.
//! 로그 문구 사례(`굽기 시간`·`전체 굽기 시간`)는 stderr라 여기서 잡지 않는다

mod common;

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use bytes::Bytes;
use common::*;
use nyangmeong_character::bake;
use nyangmeong_character::engine::BakeItem;
use nyangmeong_character::error::ApiError;
use nyangmeong_character::queue::{BAKE_BATCH, BAKE_CONCURRENCY, BakeOutcome, BakeQueue, BakeTask};
use nyangmeong_character::repository::BakeTargetPage;
use nyangmeong_character::service;
use tokio::sync::Semaphore;

type Queue = BakeQueue<FakeClient, FakeRepo>;

/// 원본 `Repo` : 몸통 2개·목 1개
fn three_items() -> Vec<BakeItem> {
    vec![
        item(1, "MYPC0004"),
        item(2, "MYPC0004"),
        item(3, "MYPC0003"),
    ]
}

/// 원본 `run` : 작업 1번을 생성·보고하고 전 아이템을 굽는다
async fn run(client: FakeClient, repo: FakeRepo) -> (Arc<FakeClient>, Arc<FakeRepo>) {
    let (client, repo) = (Arc::new(client), Arc::new(repo));
    repo.insert(SavedJob::new(job(1, "PETC0001"), 3));
    let queue = Arc::new(Queue::default());
    service::generate_and_report(
        client.clone(),
        repo.clone(),
        queue,
        job(1, "PETC0001"),
        b"photo".to_vec(),
        None,
    )
    .await;
    (client, repo)
}

fn task(
    client: &Arc<FakeClient>,
    repo: &Arc<FakeRepo>,
    pixel: i64,
    item_sn: i64,
) -> BakeTask<FakeClient, FakeRepo> {
    BakeTask {
        client: client.clone(),
        repo: repo.clone(),
        pixel_char_sn: pixel,
        item_sn,
        item: item(item_sn, "MYPC0004"),
        sheet: Some(bare()),
        pet_se_cd: Some("PETC0001".into()),
    }
}

fn sns(items: impl IntoIterator<Item = i64>) -> HashSet<i64> {
    items.into_iter().collect()
}

#[tokio::test]
async fn after_generation_every_item_is_baked_once() {
    let (client, repo) = run(FakeClient::default(), FakeRepo::with_items(three_items())).await;

    let mut calls = client.calls();
    calls.sort_unstable();
    assert_eq!(calls, [1, 2, 3]);
    assert_eq!(sns(repo.saved(1).item_sheets.into_keys()), sns([1, 2, 3]));
}

#[tokio::test]
async fn bake_failure_is_retried_once_then_reported_failed() {
    let client = FakeClient {
        fail: sns([2]),
        ..FakeClient::default()
    };

    let (client, repo) = run(client, FakeRepo::with_items(three_items())).await;

    assert_eq!(client.count(2), 2);
    let saved = repo.saved(1);
    assert_eq!(saved.failed_items, sns([2]));
    assert_eq!(sns(saved.item_sheets.into_keys()), sns([1, 3]));
}

#[tokio::test]
async fn empty_layer_is_baked_again() {
    let client = FakeClient {
        flaky: sns([2]),
        ..FakeClient::default()
    };

    let (client, repo) = run(client, FakeRepo::with_items(three_items())).await;

    assert_eq!(client.count(2), 2);
    assert!(repo.saved(1).item_sheets.contains_key(&2));
}

#[tokio::test]
async fn put_first_moves_to_front_and_skips_a_running_key() {
    let gate = Semaphore::new(0);
    let client = Arc::new(FakeClient {
        gate: Some(gate),
        ..FakeClient::default()
    });
    let repo = Arc::new(FakeRepo::default());
    repo.insert(SavedJob::with_sheet(1));
    let queue = Arc::new(Queue::default());

    // 넣기 : 같은 키는 한 번만, put_first는 앞으로
    queue.add(task(&client, &repo, 1, 1));
    queue.add(task(&client, &repo, 1, 2));
    queue.add(task(&client, &repo, 1, 1));
    queue.put_first(task(&client, &repo, 1, 2));
    assert_eq!(queue.pending(), [(1, 2), (1, 1)]);

    // 굽는 중인 키 : 다시 넣지도 앞으로 올리지도 않음
    let running = {
        let queue = queue.clone();
        tokio::spawn(async move { queue.drain().await })
    };
    while client.peak.load(Ordering::SeqCst) < 2 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(queue.pending().is_empty());
    queue.put_first(task(&client, &repo, 1, 2));
    queue.add(task(&client, &repo, 1, 1));
    assert!(queue.pending().is_empty());

    client.gate.as_ref().unwrap().add_permits(10);
    running.await.unwrap();
    assert_eq!(client.calls().len(), 2);
}

#[tokio::test]
async fn at_most_bake_concurrency_items_bake_at_once() {
    let client = Arc::new(FakeClient {
        gate: Some(Semaphore::new(0)),
        ..FakeClient::default()
    });
    let repo = Arc::new(FakeRepo::default());
    repo.insert(SavedJob::with_sheet(1));
    let queue = Arc::new(Queue::default());
    for item_sn in 0..20 {
        queue.add(task(&client, &repo, 1, item_sn));
    }

    // 두 번 불러도 일꾼은 상한까지만
    let first = {
        let queue = queue.clone();
        tokio::spawn(async move { queue.drain().await })
    };
    let second = {
        let queue = queue.clone();
        tokio::spawn(async move { queue.drain().await })
    };
    while client.peak.load(Ordering::SeqCst) < BAKE_CONCURRENCY {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    client.gate.as_ref().unwrap().add_permits(100);
    first.await.unwrap();
    second.await.unwrap();

    assert_eq!(client.peak.load(Ordering::SeqCst), BAKE_CONCURRENCY);
    assert_eq!(client.calls().len(), 20);
    assert_eq!(repo.saved(1).item_sheets.len(), 20);
}

#[tokio::test]
async fn waiters_on_the_same_key_share_one_bake() {
    let client = Arc::new(FakeClient {
        gate: Some(Semaphore::new(0)),
        ..FakeClient::default()
    });
    let repo = Arc::new(FakeRepo::default());
    repo.insert(SavedJob::with_sheet(1));
    let queue = Arc::new(Queue::default());

    let runs: Vec<_> = (0..2)
        .map(|_| {
            let (queue, client, repo) = (queue.clone(), client.clone(), repo.clone());
            tokio::spawn(async move { queue.run(vec![task(&client, &repo, 1, 7)]).await })
        })
        .collect();
    while client.peak.load(Ordering::SeqCst) < 1 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    client.gate.as_ref().unwrap().add_permits(10);

    for run in runs {
        assert_eq!(run.await.unwrap(), [Some(BakeOutcome::Baked)]);
    }
    assert_eq!(client.calls(), [7]);
}

#[tokio::test]
async fn dark_fur_is_baked_from_the_lightened_source() {
    let client = Arc::new(FakeClient::default());
    let repo = Arc::new(FakeRepo::default());
    repo.insert(SavedJob::with_sheet(1));
    let dark = Bytes::from(fixture("sheets/dark/bare.png"));
    let mut bake_task = task(&client, &repo, 1, 3);
    bake_task.item = item(3, "MYPC0003");
    bake_task.sheet = Some(dark.clone());

    nyangmeong_character::queue::bake_one(bake_task).await;

    let source = bake::bake_source(&dark, "MYPC0003").unwrap();
    assert_ne!(source, dark.to_vec());
    assert_eq!(client.received.lock().unwrap()[&3].0, source);
}

fn by_image() -> BakeItem {
    BakeItem {
        item_sn: 1,
        regn_cd: "MYPC0004".into(),
        bake_hint: None,
        by_image: true,
    }
}

#[tokio::test]
async fn only_by_image_items_get_the_item_image() {
    let mut repo = FakeRepo::with_items(vec![by_image(), item(2, "MYPC0004")]);
    let hat = Bytes::from(fixture("items/hat.png"));
    repo.item_images.insert(1, hat.clone());

    let (client, repo) = run(FakeClient::default(), repo).await;

    let received = client.received.lock().unwrap();
    assert!(received[&1].1.is_some());
    assert_eq!(received[&2].1, None);
    assert_eq!(sns(repo.saved(1).item_sheets.into_keys()), sns([1, 2]));
}

#[tokio::test]
async fn item_image_fetch_failure_is_not_baked_and_reported_failed() {
    let repo = FakeRepo::with_items(vec![by_image(), item(2, "MYPC0004")]);

    let (client, repo) = run(FakeClient::default(), repo).await;

    assert!(!client.calls().contains(&1));
    let saved = repo.saved(1);
    assert_eq!(saved.failed_items, sns([1]));
    assert_eq!(sns(saved.item_sheets.into_keys()), sns([2]));
}

#[tokio::test]
async fn save_failure_is_not_baked_again() {
    let repo = FakeRepo {
        save_error: true,
        ..FakeRepo::with_items(vec![item(1, "MYPC0004")])
    };

    let (client, repo) = run(FakeClient::default(), repo).await;

    assert_eq!(client.calls(), [1]);
    let saved = repo.saved(1);
    assert!(saved.item_sheets.is_empty() && saved.failed_items.is_empty());
}

// FN-ADM-070-02 : 전체 굽기 한 묶음

async fn batch(
    client: Option<FakeClient>,
    repo: FakeRepo,
    item_sn: i64,
) -> (Result<service::ItemBakeResult, ApiError>, Arc<FakeRepo>) {
    let repo = Arc::new(repo);
    let queue = Arc::new(Queue::default());
    let result =
        service::bake_item_batch(client.map(Arc::new), repo.clone(), &queue, item_sn).await;
    (result, repo)
}

fn app_error(error: ApiError) -> (u16, String) {
    match error {
        ApiError::App(app) => (app.http_code, app.code),
        other => panic!("AppError여야 함: {other:?}"),
    }
}

#[tokio::test]
async fn batch_bakes_characters_fetching_sheets_when_taken() {
    let repo = FakeRepo::with_items(vec![item(9, "MYPC0004")]);
    repo.insert(SavedJob::with_sheet(1));
    repo.insert(SavedJob::with_sheet(2));

    let (result, repo) = batch(Some(FakeClient::default()), repo, 9).await;

    let result = result.unwrap();
    assert_eq!(
        (result.baked_cnt, result.failed_cnt, result.last),
        (2, 0, true)
    );
    assert!(repo.saved(1).item_sheets.contains_key(&9));
    assert!(repo.saved(2).item_sheets.contains_key(&9));
}

#[tokio::test]
async fn batch_dedupes_and_cuts_to_bake_batch() {
    let repo = FakeRepo {
        targets: Some(BakeTargetPage {
            pixel_char_sns: (1..=15).chain([1, 2]).collect(),
            last: true,
        }),
        ..FakeRepo::with_items(vec![item(9, "MYPC0004")])
    };
    for sn in 1..=15 {
        repo.insert(SavedJob::with_sheet(sn));
    }

    let (result, repo) = batch(Some(FakeClient::default()), repo, 9).await;

    let result = result.unwrap();
    // 후보 15개(중복 제거) 중 13개만, 남은 후보가 있어 last 아님
    assert_eq!(
        (result.baked_cnt, result.failed_cnt, result.last),
        (BAKE_BATCH, 0, false)
    );
    let baked: Vec<i64> = (1..=15)
        .filter(|sn| repo.saved(*sn).item_sheets.contains_key(&9))
        .collect();
    assert_eq!(baked, (1..=13).collect::<Vec<_>>());
}

#[tokio::test]
async fn batch_marks_missing_sheet_file_failed_but_leaves_other_errors() {
    let file_missing = FakeRepo {
        sheets_error: Some(backend_error(404, br#"{"code":"FILE_NOT_FOUND"}"#)),
        ..FakeRepo::with_items(vec![item(9, "MYPC0004")])
    };
    file_missing.insert(SavedJob::with_sheet(1));

    let (result, repo) = batch(Some(FakeClient::default()), file_missing, 9).await;

    let result = result.unwrap();
    assert_eq!(
        (result.baked_cnt, result.failed_cnt, result.last),
        (0, 1, true)
    );
    assert_eq!(repo.saved(1).failed_items, sns([9]));

    // 404여도 FILE_NOT_FOUND가 아니거나 403이면 남겨 둠 : 하나도 못 줄여 502
    for error in [
        backend_error(404, br#"{"code":"PIXEL_CHARACTER_NOT_FOUND"}"#),
        backend_error(404, b"not json"),
        backend_error(403, br#"{"code":"FILE_NOT_FOUND"}"#),
    ] {
        let repo = FakeRepo {
            sheets_error: Some(error),
            ..FakeRepo::with_items(vec![item(9, "MYPC0004")])
        };
        repo.insert(SavedJob::with_sheet(1));

        let (result, repo) = batch(Some(FakeClient::default()), repo, 9).await;

        assert_eq!(
            app_error(result.unwrap_err()),
            (502, "EXTERNAL_API_ERROR".into())
        );
        assert!(repo.saved(1).failed_items.is_empty());
    }
}

#[tokio::test]
async fn batch_failures() {
    // 생성기 없음 : Java를 부르기 전에 502
    let (result, _) = batch(None, FakeRepo::default(), 9).await;
    assert_eq!(
        app_error(result.unwrap_err()),
        (502, "EXTERNAL_API_ERROR".into())
    );
    // 굽기 대상 아닌 아이템 : 400
    let (result, _) = batch(Some(FakeClient::default()), FakeRepo::default(), 9).await;
    assert_eq!(
        app_error(result.unwrap_err()),
        (400, "INVALID_REQUEST".into())
    );
    // 대상이 없으면 0·0·last
    let (result, _) = batch(
        Some(FakeClient::default()),
        FakeRepo::with_items(vec![item(9, "MYPC0004")]),
        9,
    )
    .await;
    assert_eq!(
        result.unwrap(),
        service::ItemBakeResult {
            baked_cnt: 0,
            failed_cnt: 0,
            last: true
        }
    );
}
