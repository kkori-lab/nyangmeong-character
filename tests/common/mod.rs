//! 굽기 큐·회원 경로 테스트가 같이 쓰는 가짜 : 원본 `tests/character/fakes.py`의 `FakeCharacterJobRepository`와
//! `test_bake.py`·`test_create.py`의 가짜 생성기를 옮김. 판정은 Java 몫이라 메모리 기록만 함
//! 시트는 원본처럼 그려 만들지 않고 `tests/fixtures/image/sheets`의 합성 시트 사용(부위별 착용 시트가 있음)

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use bytes::Bytes;
use nyangmeong_character::backend::BackendError;
use nyangmeong_character::canvas::Layout;
use nyangmeong_character::clients::{
    CharacterApiError, CharacterClient, CharacterRequest, GeneratedCharacter,
};
use nyangmeong_character::engine::BakeItem;
use nyangmeong_character::error::ApiError;
use nyangmeong_character::pil::{self, Img};
use nyangmeong_character::repository::{
    BakeTargetPage, CharacterFailure, CharacterJob, CharacterJobRepository, CharacterSheets,
};
use tokio::sync::Semaphore;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/image");

pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}/{name}")).unwrap()
}

pub fn bare() -> Bytes {
    Bytes::from(fixture("sheets/plain/bare.png"))
}

/// 부위에 아이템을 그린 착용 시트
pub fn worn(regn_cd: &str) -> Vec<u8> {
    fixture(&format!("sheets/plain/worn_{regn_cd}.png"))
}

pub const LAYOUT: Layout = Layout {
    scale: 0.5,
    origin_x: 10.5,
    origin_y: 20.0,
};

/// 투명한 w×h PNG
pub fn png(width: usize, height: usize) -> Vec<u8> {
    pil::encode_png(&Img::transparent(width, height))
}

pub fn item(item_sn: i64, regn_cd: &str) -> BakeItem {
    BakeItem {
        item_sn,
        regn_cd: regn_cd.into(),
        bake_hint: Some(format!("item {item_sn}")),
        by_image: false,
    }
}

pub fn backend_error(status: u16, body: &[u8]) -> ApiError {
    BackendError {
        status_code: status,
        body: Bytes::copy_from_slice(body),
        content_type: Some("application/json".into()),
        set_cookies: Vec::new(),
    }
    .into()
}

#[derive(Clone, Debug)]
pub struct SavedJob {
    pub job: CharacterJob,
    pub pet_sn: i64,
    pub image: Option<Vec<u8>>,
    pub sheet: Bytes,
    pub layout: Option<Layout>,
    pub failure: Option<CharacterFailure>,
    pub item_sheets: HashMap<i64, Bytes>,
    pub failed_items: HashSet<i64>,
    pub equipped: Option<(Vec<i64>, Vec<u8>)>,
    pub original: Option<(String, Vec<u8>)>,
}

impl SavedJob {
    pub fn new(job: CharacterJob, pet_sn: i64) -> Self {
        Self {
            job,
            pet_sn,
            image: None,
            sheet: Bytes::new(),
            layout: None,
            failure: None,
            item_sheets: HashMap::new(),
            failed_items: HashSet::new(),
            equipped: None,
            original: None,
        }
    }

    /// 맨몸 시트·배치값까지 있는 캐릭터
    pub fn with_sheet(sn: i64) -> Self {
        let mut saved = Self::new(job(sn, "PETC0001"), 3);
        saved.sheet = bare();
        saved.layout = Some(LAYOUT);
        saved
    }
}

pub fn job(sn: i64, pet_se_cd: &str) -> CharacterJob {
    CharacterJob {
        char_crt_job_sn: sn,
        pet_se_cd: pet_se_cd.into(),
        vrty_nm: None,
        colr_nm: None,
        chrc_cn: None,
    }
}

/// Java 흉내 : 캐릭터 번호는 작업 번호와 같음, petSn이 짝수면 고양이
pub struct FakeRepo {
    pub jobs: Mutex<HashMap<i64, SavedJob>>,
    pub next: AtomicUsize,
    pub items: Vec<BakeItem>,
    pub item_images: HashMap<i64, Bytes>,
    /// 작업 생성 때 Java가 돌려줄 등록 정보 (없으면 번호·종만)
    pub job_info: Option<CharacterJob>,
    pub create_error: Option<ApiError>,
    pub success_error: bool,
    pub list_items_error: bool,
    pub equip_error: Option<ApiError>,
    /// 맨몸 시트 조회 오류 (전체 굽기 : 파일 없음 흉내)
    pub sheets_error: Option<ApiError>,
    pub save_error: bool,
    pub targets: Option<BakeTargetPage>,
    pub regen_jobs: HashMap<i64, CharacterJob>,
    pub original_photos: HashMap<i64, Bytes>,
    pub regen_start_error: Option<ApiError>,
    pub regen_calls: Mutex<Vec<&'static str>>,
}

impl Default for FakeRepo {
    fn default() -> Self {
        Self {
            jobs: Mutex::default(),
            next: AtomicUsize::new(1),
            items: Vec::new(),
            item_images: HashMap::new(),
            job_info: None,
            create_error: None,
            success_error: false,
            list_items_error: false,
            equip_error: None,
            sheets_error: None,
            save_error: false,
            targets: None,
            regen_jobs: HashMap::new(),
            original_photos: HashMap::new(),
            regen_start_error: None,
            regen_calls: Mutex::default(),
        }
    }
}

impl FakeRepo {
    pub fn with_items(items: Vec<BakeItem>) -> Self {
        Self {
            items,
            ..Self::default()
        }
    }

    pub fn insert(&self, saved: SavedJob) {
        self.jobs
            .lock()
            .unwrap()
            .insert(saved.job.char_crt_job_sn, saved);
    }

    pub fn saved(&self, sn: i64) -> SavedJob {
        self.jobs.lock().unwrap()[&sn].clone()
    }

    pub fn job_count(&self) -> usize {
        self.jobs.lock().unwrap().len()
    }
}

fn not_found() -> ApiError {
    backend_error(404, br#"{"code":"PIXEL_CHARACTER_NOT_FOUND"}"#)
}

impl CharacterJobRepository for FakeRepo {
    async fn create_job(&self, pet_sn: i64) -> Result<CharacterJob, ApiError> {
        if let Some(error) = &self.create_error {
            return Err(error.clone());
        }
        let sn = self.next.fetch_add(1, Ordering::SeqCst) as i64;
        let job = match &self.job_info {
            Some(info) => CharacterJob {
                char_crt_job_sn: sn,
                ..info.clone()
            },
            None => job(
                sn,
                if pet_sn % 2 == 0 {
                    "PETC0002"
                } else {
                    "PETC0001"
                },
            ),
        };
        self.insert(SavedJob::new(job.clone(), pet_sn));
        Ok(job)
    }

    async fn regen_start(&self, char_crt_job_sn: i64) -> Result<CharacterJob, ApiError> {
        self.regen_calls.lock().unwrap().push("start");
        if let Some(error) = &self.regen_start_error {
            return Err(error.clone());
        }
        let job = self.regen_jobs[&char_crt_job_sn].clone();
        self.insert(SavedJob::new(job.clone(), 0));
        Ok(job)
    }

    async fn fetch_original_photo(&self, char_crt_job_sn: i64) -> Result<Bytes, ApiError> {
        self.regen_calls.lock().unwrap().push("photo");
        self.original_photos
            .get(&char_crt_job_sn)
            .cloned()
            .ok_or_else(not_found)
    }

    async fn report_success(
        &self,
        char_crt_job_sn: i64,
        image: &[u8],
        sheet: &[u8],
        layout: Option<Layout>,
        original: Option<(&str, &[u8])>,
    ) -> Result<i64, ApiError> {
        if self.success_error {
            return Err(backend_error(500, b""));
        }
        let mut jobs = self.jobs.lock().unwrap();
        let saved = jobs.get_mut(&char_crt_job_sn).unwrap();
        saved.image = Some(image.to_vec());
        saved.sheet = Bytes::copy_from_slice(sheet);
        saved.layout = layout;
        saved.original = original.map(|(name, photo)| (name.to_owned(), photo.to_vec()));
        Ok(char_crt_job_sn)
    }

    async fn report_failure(
        &self,
        char_crt_job_sn: i64,
        reason: CharacterFailure,
    ) -> Result<(), ApiError> {
        self.jobs
            .lock()
            .unwrap()
            .get_mut(&char_crt_job_sn)
            .unwrap()
            .failure = Some(reason);
        Ok(())
    }

    async fn list_bake_items(&self) -> Result<Vec<BakeItem>, ApiError> {
        if self.list_items_error {
            return Err(backend_error(401, b"{}"));
        }
        Ok(self.items.clone())
    }

    async fn fetch_item_image(&self, item_sn: i64) -> Result<Bytes, ApiError> {
        self.item_images
            .get(&item_sn)
            .cloned()
            .ok_or_else(|| backend_error(404, br#"{"code":"FILE_NOT_FOUND"}"#))
    }

    async fn list_bake_targets(&self, item_sn: i64) -> Result<BakeTargetPage, ApiError> {
        if let Some(page) = &self.targets {
            return Ok(page.clone());
        }
        // 맨몸 시트가 있고 그 아이템을 굽지도 실패하지도 않은 캐릭터의 첫 페이지(100개)
        let jobs = self.jobs.lock().unwrap();
        let mut found: Vec<i64> = jobs
            .iter()
            .filter(|(_, saved)| {
                saved.layout.is_some()
                    && !saved.item_sheets.contains_key(&item_sn)
                    && !saved.failed_items.contains(&item_sn)
            })
            .map(|(sn, _)| *sn)
            .collect();
        found.sort_unstable();
        let last = found.len() <= 100;
        found.truncate(100);
        Ok(BakeTargetPage {
            pixel_char_sns: found,
            last,
        })
    }

    async fn save_item_sheet(
        &self,
        pixel_char_sn: i64,
        item_sn: i64,
        sheet: &[u8],
    ) -> Result<(), ApiError> {
        if self.save_error {
            return Err(backend_error(500, b""));
        }
        self.jobs
            .lock()
            .unwrap()
            .get_mut(&pixel_char_sn)
            .unwrap()
            .item_sheets
            .insert(item_sn, Bytes::copy_from_slice(sheet));
        Ok(())
    }

    async fn report_bake_failure(&self, pixel_char_sn: i64, item_sn: i64) -> Result<(), ApiError> {
        self.jobs
            .lock()
            .unwrap()
            .get_mut(&pixel_char_sn)
            .unwrap()
            .failed_items
            .insert(item_sn);
        Ok(())
    }

    async fn find_sheets(
        &self,
        pixel_char_sn: i64,
        item_sns: &[i64],
    ) -> Result<CharacterSheets, ApiError> {
        if let Some(error) = &self.sheets_error {
            return Err(error.clone());
        }
        let jobs = self.jobs.lock().unwrap();
        let Some(saved) = jobs.get(&pixel_char_sn).filter(|s| s.layout.is_some()) else {
            return Err(not_found());
        };
        let wanted: HashSet<i64> = item_sns.iter().copied().collect();
        Ok(CharacterSheets {
            pet_se_cd: saved.job.pet_se_cd.clone(),
            sheet: saved.sheet.clone(),
            layout: saved.layout.unwrap(),
            sheets: saved
                .item_sheets
                .iter()
                .filter(|(sn, _)| wanted.contains(sn))
                .map(|(sn, sheet)| (*sn, sheet.clone()))
                .collect(),
            failed: saved.failed_items.intersection(&wanted).copied().collect(),
        })
    }

    async fn save_equipped_image(
        &self,
        pixel_char_sn: i64,
        item_sns: &[i64],
        image: &[u8],
    ) -> Result<(), ApiError> {
        if let Some(error) = &self.equip_error {
            return Err(error.clone());
        }
        self.jobs
            .lock()
            .unwrap()
            .get_mut(&pixel_char_sn)
            .unwrap()
            .equipped = Some((item_sns.to_vec(), image.to_vec()));
        Ok(())
    }
}

/// 굽기에 들어온 (맨몸 입력, 아이템 그림)
pub type Received = (Vec<u8>, Option<Vec<u8>>);

/// 생성 답
pub enum Generated {
    Ok(GeneratedCharacter),
    Fail,
}

/// 아이템별로 정해 둔 결과를 내는 가짜 생성기
pub struct FakeClient {
    pub generated: Generated,
    pub generate_calls: Mutex<Vec<CharacterRequest>>,
    /// 늘 굽기 실패
    pub fail: HashSet<i64>,
    /// 첫 굽기만 맨몸 그대로(빈 레이어)
    pub flaky: HashSet<i64>,
    pub error: AtomicBool,
    /// 굽기에 들어온 아이템 번호(순서대로)
    pub calls: Mutex<Vec<i64>>,
    /// 굽기에 들어온 (맨몸 입력, 아이템 그림)
    pub received: Mutex<HashMap<i64, Received>>,
    /// 있으면 굽기 한 번마다 허가 하나를 기다림
    pub gate: Option<Semaphore>,
    pub running: AtomicUsize,
    pub peak: AtomicUsize,
}

impl Default for FakeClient {
    fn default() -> Self {
        Self {
            generated: Generated::Ok(GeneratedCharacter {
                image: png(512, 512),
                sheet: bare().to_vec(),
                layout: Some(LAYOUT),
            }),
            generate_calls: Mutex::default(),
            fail: HashSet::new(),
            flaky: HashSet::new(),
            error: AtomicBool::new(false),
            calls: Mutex::default(),
            received: Mutex::default(),
            gate: None,
            running: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        }
    }
}

impl FakeClient {
    pub fn generating(generated: Generated) -> Self {
        Self {
            generated,
            ..Self::default()
        }
    }

    pub fn calls(&self) -> Vec<i64> {
        self.calls.lock().unwrap().clone()
    }

    pub fn count(&self, item_sn: i64) -> usize {
        self.calls().iter().filter(|sn| **sn == item_sn).count()
    }
}

impl CharacterClient for FakeClient {
    async fn generate(
        &self,
        request: CharacterRequest,
    ) -> Result<GeneratedCharacter, CharacterApiError> {
        self.generate_calls.lock().unwrap().push(request);
        match &self.generated {
            Generated::Ok(character) => Ok(character.clone()),
            Generated::Fail => Err(CharacterApiError),
        }
    }

    async fn bake(
        &self,
        sheet: &[u8],
        _pet_se_cd: &str,
        item: &BakeItem,
        item_image: Option<&[u8]>,
    ) -> Result<Vec<u8>, CharacterApiError> {
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        if let Some(gate) = &self.gate {
            gate.acquire().await.unwrap().forget();
        }
        self.running.fetch_sub(1, Ordering::SeqCst);
        let first = {
            let mut calls = self.calls.lock().unwrap();
            calls.push(item.item_sn);
            calls.iter().filter(|sn| **sn == item.item_sn).count() == 1
        };
        self.received.lock().unwrap().insert(
            item.item_sn,
            (sheet.to_vec(), item_image.map(<[u8]>::to_vec)),
        );
        if self.error.load(Ordering::SeqCst) || self.fail.contains(&item.item_sn) {
            return Err(CharacterApiError);
        }
        if first && self.flaky.contains(&item.item_sn) {
            return Ok(bare().to_vec());
        }
        Ok(worn(&item.regn_cd))
    }

    async fn describe_item(&self, _: &[u8], _: &str, _: &str) -> Result<String, CharacterApiError> {
        Err(CharacterApiError)
    }

    async fn draw_item(&self, _: &[u8], _: &str, _: &str) -> Result<Vec<u8>, CharacterApiError> {
        Err(CharacterApiError)
    }
}
