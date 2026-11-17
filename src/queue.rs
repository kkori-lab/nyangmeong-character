//! `app/character/service.py` 굽기 큐 : `BakeTask`·`BakeQueue`·`_bake_one`·`_draw`·`_sheet_file_missing`·`_mark_bake_failed`
//!
//! - 프로세스 메모리 : 재시작하면 사라지고 프로세스 1개 전제(원본과 같음)
//! - 이미 굽는 아이템은 다시 넣지 않음 : 같은 아이템 이중 과금 방지
//! - Python은 한 스레드라 "확인 후 넣기"·"꺼내고 굽는 중 표시"가 끊기지 않는다. 여기는 상태를 잠금 하나에 두고
//!   그 두 동작을 잠금 안에서 한 번에 한다. 잠금은 `.await` 동안 잡지 않는다
//! - 차이: 일꾼 수는 잠금 안에서 세어 늘 `BAKE_CONCURRENCY` 이하다. Python은 `_work`가 처음 돌 때 수를 올려,
//!   같은 틈에 `drain`이 두 번 불리면 잠깐 넘을 수 있다(결과는 같음)
//! - 차이: 끊긴 요청의 대기자는 `future.done()` 확인 대신 `oneshot` 보내기 실패를 무시한다(결과 같음)
//! - 일꾼은 `tokio::spawn`으로 띄운다 : 기다리던 요청이 끊겨도 굽기는 이어진다(Python `create_task`와 같음)

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use bytes::Bytes;
use tokio::sync::oneshot;

use crate::clients::CharacterClient;
use crate::engine::BakeItem;
use crate::error::ApiError;
use crate::logs;
use crate::pyjson;
use crate::repository::CharacterJobRepository;
use crate::service;

const LOGGER: &str = "app.character.service";

/// OpenAI 동시 호출 상한
pub const BAKE_CONCURRENCY: usize = 13;
pub const BAKE_ATTEMPTS: u32 = 2;
/// 전체 굽기 한 번 호출의 캐릭터 수 : 한 번 굽는 시간 안에 끝나 쿠키(30분) 만료 밖
pub const BAKE_BATCH: usize = BAKE_CONCURRENCY;

/// 굽기 결과 : 시트 저장·실패 표시. `None`(바깥 `Option`)은 둘 다 못 함 (Java 목록에 남음)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BakeOutcome {
    Baked,
    Failed,
}

pub type Key = (i64, i64);

pub struct BakeTask<C, R> {
    pub client: Arc<C>,
    pub repo: Arc<R>,
    pub pixel_char_sn: i64,
    pub item_sn: i64,
    pub item: BakeItem,
    /// 전체 굽기 : 없으면 꺼낼 때 Java에서 받음 (캐릭터 수만큼 메모리에 올리지 않음)
    pub sheet: Option<Bytes>,
    pub pet_se_cd: Option<String>,
}

impl<C, R> BakeTask<C, R> {
    pub fn key(&self) -> Key {
        (self.pixel_char_sn, self.item_sn)
    }
}

struct State<C, R> {
    tasks: VecDeque<BakeTask<C, R>>,
    running: HashSet<Key>,
    workers: usize,
    /// 결과를 기다리는 호출 : 같은 키가 굽는 중이면 그 결과를 함께 받음
    waiters: HashMap<Key, Vec<oneshot::Sender<Option<BakeOutcome>>>>,
}

pub struct BakeQueue<C, R> {
    state: Mutex<State<C, R>>,
}

impl<C, R> Default for BakeQueue<C, R> {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                tasks: VecDeque::new(),
                running: HashSet::new(),
                workers: 0,
                waiters: HashMap::new(),
            }),
        }
    }
}

impl<C, R> BakeQueue<C, R> {
    fn lock(&self) -> MutexGuard<'_, State<C, R>> {
        // 잠금 안에서는 패닉할 코드가 없다 : 독이 들어도 상태는 온전
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn add_locked(state: &mut State<C, R>, task: BakeTask<C, R>) {
        let key = task.key();
        if !state.running.contains(&key) && state.tasks.iter().all(|t| t.key() != key) {
            state.tasks.push_back(task);
        }
    }

    pub fn add(&self, task: BakeTask<C, R>) {
        Self::add_locked(&mut self.lock(), task);
    }

    pub fn put_first(&self, task: BakeTask<C, R>) {
        let key = task.key();
        let mut state = self.lock();
        if state.running.contains(&key) {
            return;
        }
        state.tasks.retain(|t| t.key() != key);
        state.tasks.push_front(task);
    }

    /// 대기 중인 작업의 키(앞에서부터) : 확인용
    pub fn pending(&self) -> Vec<Key> {
        self.lock().tasks.iter().map(BakeTask::key).collect()
    }

    fn notify(state: &mut State<C, R>, key: Key, outcome: Option<BakeOutcome>) {
        for waiter in state.waiters.remove(&key).unwrap_or_default() {
            // 요청이 끊겨 받는 쪽이 없어진 대기는 건너뜀
            let _ = waiter.send(outcome);
        }
    }
}

/// 굽던 작업이 끝나면(패닉 포함) 굽는 중 표시를 지우고 대기자에게 알린다
struct Running<'a, C, R> {
    queue: &'a BakeQueue<C, R>,
    key: Key,
    outcome: Option<BakeOutcome>,
}

impl<C, R> Drop for Running<'_, C, R> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            logs::error(
                LOGGER,
                format!("굽기 실패: {}, itemSn={}", self.key.0, self.key.1),
            );
        }
        let mut state = self.queue.lock();
        state.running.remove(&self.key);
        BakeQueue::notify(&mut state, self.key, self.outcome);
    }
}

/// 일꾼이 끝나면(패닉 포함) 일꾼 수를 줄인다
struct Worker<'a, C, R>(&'a BakeQueue<C, R>);

impl<C, R> Drop for Worker<'_, C, R> {
    fn drop(&mut self) {
        self.0.lock().workers -= 1;
    }
}

impl<C, R> BakeQueue<C, R>
where
    C: CharacterClient + 'static,
    R: CharacterJobRepository + 'static,
{
    /// 큐 뒤에 넣고 끝날 때까지 기다림 -> 작업별 결과
    pub async fn run(self: &Arc<Self>, tasks: Vec<BakeTask<C, R>>) -> Vec<Option<BakeOutcome>> {
        let waits: Vec<_> = {
            let mut state = self.lock();
            tasks
                .into_iter()
                .map(|task| {
                    let key = task.key();
                    Self::add_locked(&mut state, task);
                    let (sender, receiver) = oneshot::channel();
                    state.waiters.entry(key).or_default().push(sender);
                    receiver
                })
                .collect()
        };
        let queue = self.clone();
        tokio::spawn(async move { queue.drain().await });
        let mut outcomes = Vec::with_capacity(waits.len());
        for wait in waits {
            outcomes.push(wait.await.unwrap_or(None));
        }
        outcomes
    }

    /// 빈 일꾼 자리만큼 일꾼을 띄우고 모두 끝날 때까지 기다림
    pub async fn drain(self: &Arc<Self>) {
        let idle = {
            let mut state = self.lock();
            let idle = BAKE_CONCURRENCY.saturating_sub(state.workers);
            state.workers += idle;
            idle
        };
        let workers: Vec<_> = (0..idle)
            .map(|_| {
                let queue = self.clone();
                tokio::spawn(async move { queue.work().await })
            })
            .collect();
        for worker in workers {
            let _ = worker.await;
        }
    }

    async fn work(&self) {
        let _worker = Worker(self);
        loop {
            let task = {
                let mut state = self.lock();
                let Some(task) = state.tasks.pop_front() else {
                    break;
                };
                state.running.insert(task.key());
                task
            };
            let mut running = Running {
                queue: self,
                key: task.key(),
                outcome: None,
            };
            running.outcome = bake_one(task).await;
        }
    }
}

/// 한 작업 굽기 : 저장 실패는 다시 굽지 않음(이미 낸 비용)
pub async fn bake_one<C: CharacterClient, R: CharacterJobRepository>(
    task: BakeTask<C, R>,
) -> Option<BakeOutcome> {
    let (sheet, pet_se_cd) = match (&task.sheet, &task.pet_se_cd) {
        (Some(sheet), Some(pet_se_cd)) => (sheet.clone(), pet_se_cd.clone()),
        _ => match task.repo.find_sheets(task.pixel_char_sn, &[]).await {
            Ok(found) => (found.sheet, found.pet_se_cd),
            Err(error) if sheet_file_missing(&error) => {
                // 파일이 사라진 캐릭터 : 실패 표시로 Java 목록에서 빼야 다음 묶음으로 넘어감
                logs::warning(
                    LOGGER,
                    format!(
                        "맨몸 시트 파일 없음, 굽기 실패 표시: {}, itemSn={}",
                        task.pixel_char_sn, task.item_sn
                    ),
                );
                return mark_bake_failed(&task).await;
            }
            Err(_) => {
                logs::error(
                    LOGGER,
                    format!("굽기용 맨몸 시트 조회 실패: {}", task.pixel_char_sn),
                );
                return None;
            }
        },
    };
    let started = Instant::now();
    let (worn, attempts) = draw(&task, &sheet, &pet_se_cd).await;
    logs::info(
        LOGGER,
        format!(
            "굽기 시간: pixelCharSn={}, itemSn={}, {:.1}초, {}회 시도, {}",
            task.pixel_char_sn,
            task.item_sn,
            started.elapsed().as_secs_f64(),
            attempts,
            if worn.is_none() { "실패" } else { "성공" }
        ),
    );
    let reported = match &worn {
        None => task
            .repo
            .report_bake_failure(task.pixel_char_sn, task.item_sn)
            .await
            .map(|()| BakeOutcome::Failed),
        Some(worn) => task
            .repo
            .save_item_sheet(task.pixel_char_sn, task.item_sn, worn)
            .await
            .map(|()| BakeOutcome::Baked),
    };
    match reported {
        Ok(outcome) => Some(outcome),
        Err(_) => {
            logs::error(
                LOGGER,
                format!(
                    "굽기 결과 보고 실패: {}, itemSn={}",
                    task.pixel_char_sn, task.item_sn
                ),
            );
            None
        }
    }
}

/// 403·그 밖의 4xx·502는 일시 오류일 수 있어 남겨 둔다
fn sheet_file_missing(error: &ApiError) -> bool {
    let ApiError::Backend(backend) = error else {
        return false;
    };
    if backend.status_code != 404 {
        return false;
    }
    // `json.loads(exc.body).get("code")` : JSON이 아니거나 객체가 아니면 False
    let body = backend
        .body
        .strip_prefix(b"\xef\xbb\xbf")
        .unwrap_or(&backend.body);
    let Some(data) = std::str::from_utf8(body)
        .ok()
        .and_then(pyjson::loads_str_lenient)
    else {
        return false;
    };
    data.as_object()
        .and_then(|map| map.get("code"))
        .and_then(|code| code.as_str())
        == Some("FILE_NOT_FOUND")
}

async fn mark_bake_failed<C, R: CharacterJobRepository>(
    task: &BakeTask<C, R>,
) -> Option<BakeOutcome> {
    match task
        .repo
        .report_bake_failure(task.pixel_char_sn, task.item_sn)
        .await
    {
        Ok(()) => Some(BakeOutcome::Failed),
        Err(_) => {
            logs::error(
                LOGGER,
                format!(
                    "굽기 실패 표시 실패: {}, itemSn={}",
                    task.pixel_char_sn, task.item_sn
                ),
            );
            None
        }
    }
}

/// 최대 `BAKE_ATTEMPTS`번 굽기 -> (구운 시트, 시도 횟수)
async fn draw<C: CharacterClient, R: CharacterJobRepository>(
    task: &BakeTask<C, R>,
    sheet: &Bytes,
    pet_se_cd: &str,
) -> (Option<Vec<u8>>, u32) {
    for attempt in 1..=BAKE_ATTEMPTS {
        match service::bake_once(&*task.client, &*task.repo, sheet, pet_se_cd, &task.item).await {
            Ok(worn) => return (Some(worn), attempt),
            // 큐 일꾼 유지 : 한 아이템 실패가 나머지 굽기를 막지 않게
            Err(error) => logs::warning(
                LOGGER,
                format!("굽기 시도 실패: itemSn={}, {}", task.item_sn, error.name()),
            ),
        }
    }
    (None, BAKE_ATTEMPTS)
}
