//! `app/character/canvas.py` : 512 캔버스 배치
//!
//! - 규칙은 `nyangmeong-ai` `docs/design/pixel-character-engine.md` 4번

use crate::ndimage::{self, Structure};
use crate::pil::{self, Img};

pub const CANVAS_SIZE: usize = 512;
/// FN-MYP-010-17 : 캐릭터 높이 통일 뒤 눈 위치 고정 -> 크기·얼굴 자리 일정
pub const CHARACTER_HEIGHT: f64 = 341.0;
pub const EYE_X: f64 = 256.0;
pub const EYE_Y: f64 = 207.36;
/// 눈이 그림 위쪽에 있는 캐릭터 보정 : 입이 얼굴 영역 아래로 잘림 방지
pub const TOP_MAX: f64 = 100.0;

const DARK_MAX: u8 = 70;
/// 턱·몸의 어두운 무늬 제외
const EYE_SEARCH_TOP: f64 = 0.6;
const OPEN_PASSES: usize = 2;
const OUTLINE_REACH: usize = 3;
const BLOB_MIN_PX: usize = 60;
const BLOB_MAX_PX: usize = 4000;
const BLOB_RATIO_MIN: f64 = 0.5;
const BLOB_RATIO_MAX: f64 = 2.0;
const SAME_ROW: f64 = 0.06;
const MIN_APART: f64 = 0.15;
const SIZE_SIMILAR: f64 = 0.5;
const HIGHLIGHT_MIN: u8 = 200;
const HIGHLIGHT_MIN_PX: usize = 30;
const HIGHLIGHT_MAX_PX: usize = 400;

pub type Point = (f64, f64);

/// 맨몸 컷에서 잰 배치값 : 모든 착장에 재사용 -> 얼굴 자리 고정
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub scale: f64,
    pub origin_x: f64,
    pub origin_y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub image: Img,
    pub eyes: (Point, Point),
    pub scale: f64,
    pub origin: Point,
    pub clipped: bool,
}

impl Placement {
    pub fn png(&self) -> Vec<u8> {
        pil::encode_png(&self.image)
    }

    pub fn layout(&self) -> Layout {
        Layout {
            scale: self.scale,
            origin_x: self.origin.0,
            origin_y: self.origin.1,
        }
    }

    /// 512 캔버스 위 눈 좌표 (`eyes`는 원본 컷 기준)
    pub fn canvas_eyes(&self) -> (Point, Point) {
        let ((x1, y1), (x2, y2)) = self.eyes;
        let (origin_x, origin_y) = self.origin;
        (
            (origin_x + x1 * self.scale, origin_y + y1 * self.scale),
            (origin_x + x2 * self.scale, origin_y + y2 * self.scale),
        )
    }
}

/// (화소 수, 가운데 x, 가운데 y)
type Blob = (usize, f64, f64);

/// 나란하고 크기가 비슷한 덩어리 쌍 중 가장 큰 것
fn pair(blobs: &[Blob], width: usize, height: usize) -> Option<(Point, Point)> {
    let mut best: Option<(usize, Point, Point)> = None;
    for (i, &(size1, x1, y1)) in blobs.iter().enumerate() {
        for &(size2, x2, y2) in &blobs[i + 1..] {
            if (y1 - y2).abs() < SAME_ROW * height as f64
                && (x1 - x2).abs() > MIN_APART * width as f64
                && size1.min(size2) as f64 / size1.max(size2) as f64 > SIZE_SIMILAR
                && best.is_none_or(|(total, _, _)| size1 + size2 > total)
            {
                let (a, b) = ((x1, y1), (x2, y2));
                // Python `sorted([(x1, y1), (x2, y2)])` : 튜플 순서(x, 그다음 y)
                let (left, right) = if a <= b { (a, b) } else { (b, a) };
                best = Some((size1 + size2, left, right));
            }
        }
    }
    best.map(|(_, left, right)| (left, right))
}

/// 둥글고 그림 바깥에 닿지 않은 덩어리 (바깥에 닿으면 눈이 아닌 외곽선)
fn blobs(
    mask: &[bool],
    outside: &[bool],
    width: usize,
    height: usize,
    min_px: usize,
    max_px: usize,
) -> Vec<Blob> {
    let (labels, count) = ndimage::label(mask, width, height);
    struct Stats {
        size: usize,
        sum_x: u64,
        sum_y: u64,
        x: (usize, usize),
        y: (usize, usize),
        touches_outside: bool,
    }
    let mut stats: Vec<Stats> = (0..count)
        .map(|_| Stats {
            size: 0,
            sum_x: 0,
            sum_y: 0,
            x: (usize::MAX, 0),
            y: (usize::MAX, 0),
            touches_outside: false,
        })
        .collect();
    for y in 0..height {
        for x in 0..width {
            let at = y * width + x;
            let id = labels[at];
            if id == 0 {
                continue;
            }
            let s = &mut stats[id as usize - 1];
            s.size += 1;
            s.sum_x += x as u64;
            s.sum_y += y as u64;
            s.x = (s.x.0.min(x), s.x.1.max(x));
            s.y = (s.y.0.min(y), s.y.1.max(y));
            s.touches_outside |= outside[at];
        }
    }
    let mut found = Vec::new();
    for s in stats {
        if s.touches_outside {
            continue;
        }
        let ratio = (s.y.1 - s.y.0 + 1) as f64 / (s.x.1 - s.x.0 + 1) as f64;
        if (min_px..=max_px).contains(&s.size) && (BLOB_RATIO_MIN..=BLOB_RATIO_MAX).contains(&ratio)
        {
            found.push((
                s.size,
                s.sum_x as f64 / s.size as f64,
                s.sum_y as f64 / s.size as f64,
            ));
        }
    }
    found
}

/// 위쪽 60%의 어두운 눈동자 쌍, 없으면 흰 하이라이트 쌍 (맨몸 그림 전용 : 모자 그림은 오검출)
pub fn find_eyes(image: &Img) -> Option<(Point, Point)> {
    let (width, height) = (image.width, image.height);
    let opaque: Vec<bool> = image
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| px[3] > 0)
        .collect();
    let top_rows = (height as f64 * EYE_SEARCH_TOP) as usize;
    let in_top = |at: usize| at / width < top_rows;
    let transparent: Vec<bool> = opaque.iter().map(|&on| !on).collect();
    let outside =
        ndimage::binary_dilation(&transparent, width, height, Structure::Cross, OUTLINE_REACH);

    let dark: Vec<bool> = image
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .map(|(at, px)| opaque[at] && in_top(at) && px[0].max(px[1]).max(px[2]) < DARK_MAX)
        .collect();
    let opened = ndimage::binary_opening(&dark, width, height, Structure::Cross, OPEN_PASSES);
    let found = blobs(&opened, &outside, width, height, BLOB_MIN_PX, BLOB_MAX_PX);
    if let Some(eyes) = pair(&found, width, height) {
        return Some(eyes);
    }

    // 검은 털 : 눈동자가 털과 붙음 -> 프롬프트가 고정한 눈마다 흰 점 1개
    let white: Vec<bool> = image
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .map(|(at, px)| opaque[at] && in_top(at) && px[0].min(px[1]).min(px[2]) > HIGHLIGHT_MIN)
        .collect();
    let found = blobs(
        &white,
        &outside,
        width,
        height,
        HIGHLIGHT_MIN_PX,
        HIGHLIGHT_MAX_PX,
    );
    pair(&found, width, height)
}

/// FN-MYP-010-17 `place` : 높이 통일 뒤 두 눈 기준 512×512 배치 (눈을 못 찾으면 `None`)
pub fn place(image: &Img) -> Option<Placement> {
    let eyes = find_eyes(image)?;
    let ((x1, y1), (x2, y2)) = eyes;
    let scale = CHARACTER_HEIGHT / image.height as f64;
    let origin_x = EYE_X - (x1 + x2) / 2.0 * scale;
    // 입력은 그림에 맞게 잘린 컷 : origin_y가 곧 캐릭터 상단
    let origin_y = (EYE_Y - (y1 + y2) / 2.0 * scale).min(TOP_MAX);

    let size = (
        pil::py_round(image.width as f64 * scale).max(1) as usize,
        pil::py_round(image.height as f64 * scale).max(1) as usize,
    );
    // 도트 유지 : NEAREST
    let big = pil::resize_nearest(image, size.0, size.1);
    let (x, y) = (pil::py_round(origin_x), pil::py_round(origin_y));
    let mut canvas = Img::transparent(CANVAS_SIZE, CANVAS_SIZE);
    pil::paste_masked(&mut canvas, &big, x, y);
    let clipped = x < 0
        || y < 0
        || x + size.0 as i64 > CANVAS_SIZE as i64
        || y + size.1 as i64 > CANVAS_SIZE as i64;
    Some(Placement {
        image: canvas,
        eyes,
        scale,
        origin: (origin_x, origin_y),
        clipped,
    })
}
