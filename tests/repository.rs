//! `app/character/repository.py` 옮김 확인 : 원본 `tests/character/test_repository.py`의 사례를 그대로 옮겼다.
//! httpx `MockTransport` 대신 이 PC의 임시 포트에 가짜 Java를 띄운다(기대값은 원본 테스트 그대로).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use nyangmeong_character::backend::Backend;
use nyangmeong_character::canvas::Layout;
use nyangmeong_character::engine::BakeItem;
use nyangmeong_character::error::ApiError;
use nyangmeong_character::repository::{CharacterFailure, CharacterJobRepository, JavaRepository};
use serde_json::{Value, json};

const COOKIE: &str = "access_token=abc";

#[derive(Clone, Debug)]
struct Recorded {
    method: String,
    path: String,
    query: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl Recorded {
    fn content(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    fn count(&self, needle: &str) -> usize {
        self.content().matches(needle).count()
    }
}

type Handler = Arc<dyn Fn(&Recorded) -> (u16, Vec<u8>) + Send + Sync>;

#[derive(Clone)]
struct Mock {
    log: Arc<Mutex<Vec<Recorded>>>,
    handler: Handler,
}

async fn record(State(mock): State<Mock>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let body: Bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    let recorded = Recorded {
        method: parts.method.to_string(),
        path: parts.uri.path().to_owned(),
        query: parts.uri.query().unwrap_or("").to_owned(),
        headers: parts
            .headers
            .iter()
            .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned()))
            .collect(),
        body: body.to_vec(),
    };
    let (status, content) = (mock.handler)(&recorded);
    mock.log.lock().unwrap().push(recorded);
    Response::builder()
        .status(StatusCode::from_u16(status).unwrap())
        .body(axum::body::Body::from(content))
        .unwrap()
}

/// 가짜 Java를 띄우고 그쪽을 보는 저장소와 받은 요청 기록을 돌려준다
async fn make_repo(
    handler: impl Fn(&Recorded) -> (u16, Vec<u8>) + Send + Sync + 'static,
) -> (JavaRepository, Arc<Mutex<Vec<Recorded>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mock = Mock {
        log: log.clone(),
        handler: Arc::new(handler),
    };
    let app = Router::new().fallback(record).with_state(mock);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let repo = JavaRepository::new(Backend::new(&base), Some(HeaderValue::from_static(COOKIE)));
    (repo, log)
}

fn ok(data: Value) -> (u16, Vec<u8>) {
    ok_status(200, data)
}

fn ok_status(status: u16, data: Value) -> (u16, Vec<u8>) {
    (status, json!({ "data": data }).to_string().into_bytes())
}

fn empty() -> (u16, Vec<u8>) {
    (200, Vec::new())
}

fn sheets_data() -> Value {
    json!({
        "petSeCd": "PETC0002",
        "sheetFileSn": 50,
        "scale": 0.5,
        "originX": 10.5,
        "originY": 20,
        "items": [
            {"itemSn": 5, "atchFileSn": 51, "bakeFailed": false},
            {"itemSn": 6, "atchFileSn": null, "bakeFailed": false},
            {"itemSn": 7, "atchFileSn": null, "bakeFailed": true},
        ],
    })
}

fn sheets_handler(request: &Recorded) -> (u16, Vec<u8>) {
    if request.path.ends_with("/sheets") {
        return ok(sheets_data());
    }
    let number = request.path.rsplit('/').next().unwrap();
    (200, format!("FILE{number}").into_bytes())
}

const BASE: &str = "/internal/ai/characters";

fn assert_cookie(request: &Recorded) {
    assert_eq!(request.headers["cookie"], COOKIE);
    assert_eq!(request.headers["x-requested-with"], "XMLHttpRequest");
}

fn status_of(error: &ApiError) -> u16 {
    match error {
        ApiError::App(app) => app.http_code,
        ApiError::Backend(backend) => backend.status_code,
        other => panic!("예상 밖 오류 {other:?}"),
    }
}

// 호출마다 경로·메서드·쿠키 : 모두 쿠키를 보낸다(Java 쿠키 인증)

#[tokio::test]
async fn create_job_sends_cookie_and_returns_registration() {
    let data = json!({
        "charCrtJobSn": 9, "petSeCd": "PETC0002", "vrtyNm": "코숏", "colrNm": "삼색", "chrcCn": "귀 접힘",
    });
    let (repo, log) = make_repo(move |_| ok_status(201, data.clone())).await;

    let job = repo.create_job(3).await.unwrap();

    assert_eq!(
        (
            job.char_crt_job_sn,
            job.pet_se_cd.as_str(),
            job.vrty_nm.as_deref()
        ),
        (9, "PETC0002", Some("코숏"))
    );
    assert_eq!(
        (job.colr_nm.as_deref(), job.chrc_cn.as_deref()),
        (Some("삼색"), Some("귀 접힘"))
    );
    let log = log.lock().unwrap();
    let request = &log[0];
    assert_eq!(
        (request.method.as_str(), request.path.as_str()),
        ("POST", "/internal/ai/characters/jobs")
    );
    assert_cookie(request);
    assert_eq!(
        serde_json::from_slice::<Value>(&request.body).unwrap(),
        json!({"petSn": 3})
    );
}

#[tokio::test]
async fn report_success_sends_image_sheet_and_placement() {
    let (repo, log) = make_repo(|_| ok(json!({"pixelCharSn": 77}))).await;

    let sn = repo
        .report_success(
            9,
            b"IMAGE",
            b"SHEET",
            Some(Layout {
                scale: 0.5,
                origin_x: 10.5,
                origin_y: 20.0,
            }),
        )
        .await
        .unwrap();

    assert_eq!(sn, 77);
    let log = log.lock().unwrap();
    let request = &log[0];
    assert_eq!(
        (request.method.as_str(), request.path.as_str()),
        ("POST", "/internal/ai/characters/jobs/9/success")
    );
    assert_cookie(request);
    assert!(request.headers["content-type"].starts_with("multipart/form-data"));
    for part in [
        "name=\"image\"",
        "IMAGE",
        "name=\"sheet\"",
        "SHEET",
        "name=\"originX\"",
        "10.5",
        "name=\"originY\"",
    ] {
        assert!(request.content().contains(part), "{part}");
    }
}

#[tokio::test]
async fn report_success_sends_python_str_of_placement_floats() {
    // `place`가 min(.., 100)으로 정수 100을 돌려주면 Python은 "100", 실수는 repr(`0.5`·`20.0`)
    let (repo, log) = make_repo(|_| ok(json!({"pixelCharSn": 77}))).await;
    repo.report_success(
        9,
        b"I",
        b"S",
        Some(Layout {
            scale: 1.0,
            origin_x: -3.0,
            origin_y: 100.0,
        }),
    )
    .await
    .unwrap();
    let body = log.lock().unwrap()[0].content();
    let value = |name: &str| {
        let at = body.find(&format!("name=\"{name}\"")).unwrap();
        body[at..]
            .split("\r\n\r\n")
            .nth(1)
            .unwrap()
            .split("\r\n")
            .next()
            .unwrap()
            .to_owned()
    };
    assert_eq!(value("scale"), "1.0");
    assert_eq!(value("originX"), "-3.0");
    assert_eq!(value("originY"), "100");
}

#[tokio::test]
async fn report_success_without_layout_sends_no_placement() {
    let (repo, log) = make_repo(|_| ok(json!({"pixelCharSn": 77}))).await;
    repo.report_success(9, b"I", b"S", None).await.unwrap();
    assert!(
        !log.lock().unwrap()[0]
            .content()
            .contains("name=\"originX\"")
    );
}

#[tokio::test]
async fn report_failure_sends_reason() {
    let (repo, log) = make_repo(|_| empty()).await;

    repo.report_failure(9, CharacterFailure::SpecMismatch)
        .await
        .unwrap();

    let log = log.lock().unwrap();
    let request = &log[0];
    assert_eq!(
        (request.method.as_str(), request.path.as_str()),
        ("POST", "/internal/ai/characters/jobs/9/failure")
    );
    assert_cookie(request);
    assert_eq!(
        serde_json::from_slice::<Value>(&request.body).unwrap(),
        json!({"failRsn": "CHARACTER_SPEC_MISMATCH"})
    );
}

#[tokio::test]
async fn list_bake_items_returns_number_region_and_hint() {
    let data = json!([{
        "itemSn": 5, "regnCd": "MYPC0002", "bakeHint": "round thin black glasses",
        "bakeImgRefYn": false, "atchFileSn": 31,
    }]);
    let (repo, log) = make_repo(move |_| ok(data.clone())).await;

    let targets = repo.list_bake_items().await.unwrap();

    assert_eq!(
        targets,
        vec![BakeItem {
            item_sn: 5,
            regn_cd: "MYPC0002".into(),
            bake_hint: Some("round thin black glasses".into()),
            by_image: false,
        }]
    );
    let log = log.lock().unwrap();
    assert_eq!(
        (log[0].method.as_str(), log[0].path.as_str()),
        ("GET", "/internal/ai/characters/bake-items")
    );
    assert_eq!(log[0].headers["cookie"], COOKIE);
}

#[tokio::test]
async fn list_bake_items_skips_rows_without_hint_or_image_and_keeps_last_duplicate() {
    let data = json!([
        {"itemSn": 1, "regnCd": "MYPC0001", "bakeHint": "a", "bakeImgRefYn": false},
        {"itemSn": 2, "regnCd": "MYPC0002", "bakeHint": null, "bakeImgRefYn": false},
        {"itemSn": 3, "regnCd": "MYPC0003", "bakeImgRefYn": "yes"},
        {"itemSn": 1, "regnCd": "MYPC0004", "bakeHint": "b", "bakeImgRefYn": 0},
    ]);
    let (repo, _) = make_repo(move |_| ok(data.clone())).await;

    let targets = repo.list_bake_items().await.unwrap();

    let summary: Vec<_> = targets
        .iter()
        .map(|t| (t.item_sn, t.regn_cd.as_str(), t.by_image))
        .collect();
    assert_eq!(summary, vec![(1, "MYPC0004", false), (3, "MYPC0003", true)]);
}

#[tokio::test]
async fn save_item_sheet_puts_multipart() {
    let (repo, log) = make_repo(|_| empty()).await;

    repo.save_item_sheet(77, 5, b"WORN").await.unwrap();

    let log = log.lock().unwrap();
    let request = &log[0];
    assert_eq!(
        (request.method.as_str(), request.path.as_str()),
        ("PUT", "/internal/ai/characters/77/item-sheets/5")
    );
    assert_eq!(request.headers["cookie"], COOKIE);
    assert!(request.content().contains("name=\"sheet\"") && request.content().contains("WORN"));
}

#[tokio::test]
async fn save_equipped_image_puts_image_and_item_list() {
    let (repo, log) = make_repo(|_| empty()).await;

    repo.save_equipped_image(77, &[108, 11], b"WORN")
        .await
        .unwrap();

    let log = log.lock().unwrap();
    let request = &log[0];
    assert_eq!(
        (request.method.as_str(), request.path.as_str()),
        ("PUT", "/internal/ai/characters/77/equipped-image")
    );
    assert_cookie(request);
    assert!(request.headers["content-type"].starts_with("multipart/form-data"));
    assert!(request.content().contains("name=\"image\"") && request.content().contains("WORN"));
    assert_eq!(request.count("name=\"itemSns\""), 2);
    assert!(request.content().contains("108") && request.content().contains("11"));
}

#[tokio::test]
async fn save_equipped_image_without_items_sends_no_item_sns() {
    let (repo, log) = make_repo(|_| empty()).await;

    repo.save_equipped_image(77, &[], b"BARE").await.unwrap();

    let request = log.lock().unwrap()[0].clone();
    assert!(request.content().contains("name=\"image\""));
    assert!(!request.content().contains("name=\"itemSns\""));
}

#[tokio::test]
async fn report_bake_failure_posts() {
    let (repo, log) = make_repo(|_| empty()).await;

    repo.report_bake_failure(77, 5).await.unwrap();

    let log = log.lock().unwrap();
    assert_eq!(
        (log[0].method.as_str(), log[0].path.as_str()),
        ("POST", "/internal/ai/characters/77/item-sheets/5/failure")
    );
    assert_eq!(log[0].headers["cookie"], COOKIE);
}

#[tokio::test]
async fn find_sheets_downloads_sheet_files() {
    let (repo, log) = make_repo(sheets_handler).await;

    let found = repo.find_sheets(77, &[7, 5, 6, 5]).await.unwrap();

    assert_eq!(found.pet_se_cd, "PETC0002");
    assert_eq!(&found.sheet[..], b"FILE50");
    assert_eq!(
        found.layout,
        Layout {
            scale: 0.5,
            origin_x: 10.5,
            origin_y: 20.0
        }
    );
    assert_eq!(
        found.sheets,
        HashMap::from([(5, Bytes::from_static(b"FILE51"))])
    );
    assert_eq!(found.failed, HashSet::from([7]));
    let log = log.lock().unwrap();
    assert_eq!(log[0].query, "itemSns=5&itemSns=6&itemSns=7");
    let urls: HashSet<_> = log[1..].iter().map(|r| r.path.clone()).collect();
    assert_eq!(
        urls,
        HashSet::from([
            format!("{BASE}/77/sheet-files/50"),
            format!("{BASE}/77/sheet-files/51")
        ])
    );
    assert!(log.iter().all(|r| r.headers["cookie"] == COOKIE));
}

#[tokio::test]
async fn find_sheets_without_items_sends_no_query() {
    let (repo, log) = make_repo(sheets_handler).await;
    repo.find_sheets(77, &[]).await.unwrap();
    assert_eq!(log.lock().unwrap()[0].query, "");
}

#[tokio::test]
async fn find_sheets_drops_items_that_were_not_requested() {
    let (repo, _) = make_repo(sheets_handler).await;

    let found = repo.find_sheets(77, &[6]).await.unwrap();

    assert!(found.sheets.is_empty());
    assert!(found.failed.is_empty());
}

// 오류 : 4xx 그대로, 5xx·연결 실패·응답 모양 불일치 502

#[tokio::test]
async fn four_xx_is_passed_up_with_its_body() {
    let body = br#"{"code":"PET_ACCESS_DENIED"}"#.to_vec();
    let expected = body.clone();
    let (repo, _) = make_repo(move |_| (403, body.clone())).await;

    let error = repo.create_job(3).await.unwrap_err();

    let ApiError::Backend(backend) = error else {
        panic!("BackendError여야 함")
    };
    assert_eq!(
        (backend.status_code, backend.body.to_vec()),
        (403, expected)
    );
}

#[tokio::test]
async fn five_xx_and_wrong_shapes_are_502() {
    let responses = [
        (500, Vec::new()),
        ok(json!({"charCrtJobSn": "없음"})),
        ok(json!({"charCrtJobSn": 9, "petSeCd": "PETC9999"})),
    ];
    for response in responses {
        let (repo, _) = make_repo(move |_| response.clone()).await;
        let error = repo.create_job(3).await.unwrap_err();
        assert_eq!(status_of(&error), 502);
    }
}

#[tokio::test]
async fn connection_failure_is_502() {
    // 아무도 듣지 않는 포트
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let repo = JavaRepository::new(Backend::new(&base), None);

    let error = repo.list_bake_items().await.unwrap_err();

    assert_eq!(status_of(&error), 502);
}

#[tokio::test]
async fn list_bake_items_that_is_not_a_list_is_502() {
    let (repo, _) = make_repo(|_| ok(Value::Null)).await;

    let error = repo.list_bake_items().await.unwrap_err();

    assert_eq!(status_of(&error), 502);
}

#[tokio::test]
async fn empty_sheet_file_is_502() {
    let (repo, _) = make_repo(|request| {
        if request.path.ends_with("/sheets") {
            ok(sheets_data())
        } else {
            empty()
        }
    })
    .await;

    let error = repo.find_sheets(77, &[5]).await.unwrap_err();

    assert_eq!(status_of(&error), 502);
}

#[tokio::test]
async fn bake_targets_and_item_image() {
    let (repo, log) = make_repo(|request| {
        if request.path.ends_with("/bake-targets") {
            ok(json!({"pixelCharSns": [1, "2"], "last": "true"}))
        } else {
            (200, b"IMG".to_vec())
        }
    })
    .await;

    let page = repo.list_bake_targets(9).await.unwrap();
    let image = repo.fetch_item_image(9).await.unwrap();

    assert_eq!((page.pixel_char_sns, page.last), (vec![1, 2], true));
    assert_eq!(&image[..], b"IMG");
    let log = log.lock().unwrap();
    assert_eq!(
        (log[0].path.as_str(), log[0].query.as_str()),
        ("/internal/ai/items/9/bake-targets", "page=0")
    );
    assert_eq!(log[1].path, "/internal/ai/items/9/bake-image");
    assert!(log.iter().all(|r| r.headers["cookie"] == COOKIE));
}
