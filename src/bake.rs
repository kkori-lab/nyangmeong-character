//! `app/character/bake.py` : 착용 컷 정렬·아이템 레이어 추출
//!
//! - 굽기 캐시·판별 사다리는 제외 : 시트는 Java 보관. 외부 호출 없음
//! - 차이: numpy `int`(8바이트) 배열 대신 `u8`로 두고 색 차이는 `i16`으로 잰다(결과는 같음)

use crate::engine::{self, BG_THRESHOLD, Cut, EngineError};
use crate::ndimage::{self, Structure};
use crate::pil::{self, Img};

/// MYPG01 공통코드 : 머리는 착용 컷 전체가 바탕, 나머지는 레이어
pub const REGN_HEAD: &str = "MYPC0001";

/// 채널 차이 : 8이면 다시 그려진 털 음영까지 잡힘
const DIFF_TOL: i16 = 32;
/// 시트가 조금 밀려 그려지는 경우의 평행이동 탐색 : 성긴 탐색 뒤 그 주변만
const ALIGN_SEARCH: i64 = 8;
const ALIGN_COARSE_STEP: usize = 4;
const ALIGN_FINE_SEARCH: i64 = 3;
const ALIGN_FINE_STEP: usize = 2;
const MIN_BLOB_PX: f64 = 40.0;
/// 가장 큰 덩어리 대비 : 끈·안경처럼 나뉜 아이템 유지
const KEEP_BLOB_RATIO: f64 = 0.3;

/// 어두운 털 판정 : 외곽선보다 밝고 채도 낮은 화소가 캐릭터의 절반 이상
const FUR_MIN: u8 = 26;
const FUR_MAX: u8 = 120;
const FUR_CHROMA_MAX: u8 = 40;
const DARK_FUR_SHARE: f64 = 0.5;
/// 밝힌 털 : 검은 외곽선·아이템과 구분되는 밝은 회색
const LIT_BASE: f64 = 170.0;
const LIT_GAIN: f64 = 0.6;
const LIT_CHROMA_MAX: u8 = 24;
/// 맨몸과 색이 비슷해 빠진 외곽선 복구 범위
const OUTLINE_MAX: u8 = 60;
const OUTLINE_REACH: usize = 8;

/// 맨몸 컷 기준 좌표에 놓일 그림
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sprite {
    pub image: Img,
    pub x: i64,
    pub y: i64,
}

/// 레이어 차이의 기준 컷 : `lit`이면 털을 밝힌 시트의 컷
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerBase {
    pub cut: Cut,
    pub lit: bool,
}

/// 어두운 털을 밝힌 시트 (어두운 털이 아니면 `None`) : 크기는 그대로(1024로 줄이지 않음)
fn lit_image(sheet: &Img) -> Option<Img> {
    let background = [
        sheet.data[0] as i32,
        sheet.data[1] as i32,
        sheet.data[2] as i32,
    ];
    let mut shown = 0u64;
    let mut fur_at = Vec::new();
    for (at, px) in sheet.data.as_chunks::<4>().0.iter().enumerate() {
        let diff = (px[0] as i32 - background[0]).abs()
            + (px[1] as i32 - background[1]).abs()
            + (px[2] as i32 - background[2]).abs();
        if px[3] < 128 || diff <= BG_THRESHOLD {
            continue;
        }
        shown += 1;
        let top = px[0].max(px[1]).max(px[2]);
        let low = px[0].min(px[1]).min(px[2]);
        if (FUR_MIN..FUR_MAX).contains(&top) && top - low < FUR_CHROMA_MAX {
            fur_at.push(at);
        }
    }
    if (fur_at.len() as f64) < DARK_FUR_SHARE * shown.max(1) as f64 {
        return None;
    }
    let mut lit = sheet.clone();
    for at in fur_at {
        let px = &mut lit.data[at * 4..at * 4 + 4];
        let top = px[0].max(px[1]).max(px[2]);
        let value = (LIT_BASE + top as f64 * LIT_GAIN) as i64 as u8;
        px[..3].fill(value);
    }
    Some(lit)
}

/// 굽기 입력 : 검은 털 위 검은 외곽선은 차이로 못 찾음, 머리는 합성 바탕이라 원본
pub fn bake_source(sheet: &[u8], regn_cd: &str) -> Result<Vec<u8>, EngineError> {
    if regn_cd == REGN_HEAD {
        return Ok(sheet.to_vec());
    }
    let image = pil::open_rgba(sheet).map_err(EngineError::Image)?;
    Ok(match lit_image(&image) {
        Some(lit) => pil::encode_png(&lit),
        None => sheet.to_vec(),
    })
}

pub fn layer_base(sheet: &[u8]) -> Result<LayerBase, EngineError> {
    let image = pil::open_rgba(sheet).map_err(EngineError::Image)?;
    let lit = lit_image(&image);
    let cut = engine::stand_front_cut_image(lit.as_ref().unwrap_or(&image))?;
    Ok(LayerBase {
        cut,
        lit: lit.is_some(),
    })
}

/// worn을 (x, y)에 놓을 때 맨몸과 색이 같은 화소 수 (step 간격 표본)
fn matched(bare: &Img, worn: &Img, x: i64, y: i64, step: usize) -> u64 {
    let top = y.max(0);
    let left = x.max(0);
    let bottom = (bare.height as i64).min(y + worn.height as i64);
    let right = (bare.width as i64).min(x + worn.width as i64);
    if top >= bottom || left >= right {
        return 0;
    }
    let mut count = 0;
    let mut row = top;
    while row < bottom {
        let mut col = left;
        while col < right {
            let a = bare.pixel(col as usize, row as usize);
            let b = worn.pixel((col - x) as usize, (row - y) as usize);
            let same = (0..3).all(|c| (a[c] as i16 - b[c] as i16).abs() <= DIFF_TOL);
            if same && a[3] > 0 && b[3] > 0 {
                count += 1;
            }
            col += step as i64;
        }
        row += step as i64;
    }
    count
}

fn best(
    bare: &Img,
    worn: &Img,
    center: (i64, i64),
    reach: i64,
    gap: usize,
    step: usize,
) -> (i64, i64) {
    let mut best: Option<((i64, i64), u64)> = None;
    let mut dy = -reach;
    while dy <= reach {
        let mut dx = -reach;
        while dx <= reach {
            let offset = (center.0 + dx, center.1 + dy);
            let score = matched(bare, worn, offset.0, offset.1, step);
            // Python max() : 같은 점수면 먼저 나온 것
            if best.is_none_or(|(_, top)| score > top) {
                best = Some((offset, score));
            }
            dx += gap as i64;
        }
        dy += gap as i64;
    }
    best.expect("탐색 후보는 항상 있다").0
}

/// 착용 컷을 맨몸 컷 좌표로 정렬 (컷마다 잘린 범위가 달라 시트 위치에서 출발)
pub fn worn_sprite(bare: &Cut, worn: &Cut) -> Sprite {
    let start = (
        worn.left as i64 - bare.left as i64,
        worn.top as i64 - bare.top as i64,
    );
    let coarse = best(
        &bare.image,
        &worn.image,
        start,
        ALIGN_SEARCH,
        ALIGN_COARSE_STEP,
        ALIGN_COARSE_STEP,
    );
    let (x, y) = best(
        &bare.image,
        &worn.image,
        coarse,
        ALIGN_FINE_SEARCH,
        1,
        ALIGN_FINE_STEP,
    );
    Sprite {
        image: worn.image.clone(),
        x,
        y,
    }
}

fn keep(mask: Vec<bool>, width: usize, height: usize) -> Vec<bool> {
    let (labels, count) = ndimage::label(&mask, width, height);
    if count == 0 {
        return mask;
    }
    let sizes = ndimage::count_by_label(None, &labels, count);
    let biggest = *sizes.iter().max().expect("덩어리가 있다") as f64;
    let floor = MIN_BLOB_PX.max(KEEP_BLOB_RATIO * biggest);
    labels
        .iter()
        .map(|&id| id != 0 && sizes[id as usize - 1] as f64 >= floor)
        .collect()
}

/// 바깥에서 이어진 밝힌 털만 제거 : 외곽선 안쪽의 흰 진주·회색 옷은 유지
fn without_lit_fur(mask: &[bool], over: &[u8], width: usize, height: usize) -> Vec<bool> {
    let lit: Vec<bool> = over
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| {
            let top = px[0].max(px[1]).max(px[2]);
            top as f64 >= LIT_BASE && top - px[0].min(px[1]).min(px[2]) <= LIT_CHROMA_MAX
        })
        .collect();
    let outside: Vec<bool> = mask.iter().map(|&on| !on).collect();
    let touching = ndimage::binary_dilation(&outside, width, height, Structure::Cross, 1);
    let seeds: Vec<bool> = (0..mask.len()).map(|at| lit[at] && touching[at]).collect();
    let fur = ndimage::binary_propagation(&seeds, &lit, width, height);
    (0..mask.len()).map(|at| mask[at] && !fur[at]).collect()
}

/// 착용 컷에서 맨몸과 다른 화소만 남긴 레이어 (달라진 곳이 없으면 `None`)
pub fn item_layer(bare: &Img, worn: &Sprite, lit: bool) -> Option<Sprite> {
    let left = worn.x.min(0);
    let top = worn.y.min(0);
    let right = (bare.width as i64).max(worn.x + worn.image.width as i64);
    let bottom = (bare.height as i64).max(worn.y + worn.image.height as i64);
    let (width, height) = ((right - left) as usize, (bottom - top) as usize);

    let mut base = vec![0u8; width * height * 4];
    let mut over = vec![0u8; width * height * 4];
    for y in 0..bare.height {
        let to = ((y as i64 - top) as usize * width + (0 - left) as usize) * 4;
        base[to..to + bare.width * 4]
            .copy_from_slice(&bare.data[y * bare.width * 4..(y + 1) * bare.width * 4]);
    }
    for y in 0..worn.image.height {
        let to = (((worn.y - top) as usize + y) * width + (worn.x - left) as usize) * 4;
        over[to..to + worn.image.width * 4].copy_from_slice(
            &worn.image.data[y * worn.image.width * 4..(y + 1) * worn.image.width * 4],
        );
    }

    let opaque: Vec<bool> = over.as_chunks::<4>().0.iter().map(|px| px[3] > 0).collect();
    let mask: Vec<bool> = (0..width * height)
        .map(|at| {
            let (b, o) = (&base[at * 4..at * 4 + 4], &over[at * 4..at * 4 + 4]);
            let changed = (0..3).any(|c| (b[c] as i16 - o[c] as i16).abs() > DIFF_TOL);
            opaque[at] && (changed || b[3] == 0)
        })
        .collect();
    let opened = ndimage::binary_opening(&mask, width, height, Structure::Square, 1);
    let mut mask = keep(opened, width, height);
    if !mask.iter().any(|&on| on) {
        return None;
    }
    // 안경알 안쪽 : 밝힌 시트에서 채우면 밝힌 털이 바탕을 가림
    mask = if lit {
        without_lit_fur(&mask, &over, width, height)
    } else {
        let filled = ndimage::binary_fill_holes(&mask, width, height);
        (0..mask.len()).map(|at| filled[at] && opaque[at]).collect()
    };
    let dark: Vec<bool> = (0..mask.len())
        .map(|at| {
            let px = &over[at * 4..at * 4 + 4];
            opaque[at] && px[0].max(px[1]).max(px[2]) < OUTLINE_MAX
        })
        .collect();
    let near = ndimage::binary_dilation(&mask, width, height, Structure::Square, OUTLINE_REACH);
    for at in 0..mask.len() {
        mask[at] |= near[at] && dark[at];
    }
    if !mask.iter().any(|&on| on) {
        return None;
    }

    let (mut x1, mut y1, mut x2, mut y2) = (width, height, 0, 0);
    for y in 0..height {
        for x in 0..width {
            if mask[y * width + x] {
                x1 = x1.min(x);
                x2 = x2.max(x + 1);
                y1 = y1.min(y);
                y2 = y2.max(y + 1);
            }
        }
    }
    let (out_w, out_h) = (x2 - x1, y2 - y1);
    let mut data = vec![0u8; out_w * out_h * 4];
    for y in 0..out_h {
        for x in 0..out_w {
            let at = (y1 + y) * width + x1 + x;
            if mask[at] {
                let to = (y * out_w + x) * 4;
                data[to..to + 4].copy_from_slice(&over[at * 4..at * 4 + 4]);
            }
        }
    }
    Some(Sprite {
        image: Img {
            width: out_w,
            height: out_h,
            data,
        },
        x: x1 as i64 + left,
        y: y1 as i64 + top,
    })
}

/// 구운 시트를 합성에 쓸 수 있는지 판정 (쓸 수 없으면 `EngineError::Sheet`)
pub fn validate(bare_sheet: &[u8], worn_sheet: &[u8], regn_cd: &str) -> Result<(), EngineError> {
    let worn = engine::stand_front_cut(worn_sheet)?;
    if regn_cd == REGN_HEAD {
        return Ok(());
    }
    let base = layer_base(bare_sheet)?;
    if item_layer(&base.cut.image, &worn_sprite(&base.cut, &worn), base.lit).is_none() {
        return Err(EngineError::Sheet("아이템 레이어가 비어 있음".into()));
    }
    Ok(())
}
