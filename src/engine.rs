//! `app/character/engine.py` : 사진 정규화, 2×2 시트에서 서기 정면 컷 자르기, 아이템 그림 자르기
//!
//! 프롬프트·눈 판독·OpenAI 호출 쪽(`build_prompt` 등)은 M4에서 옮긴다. 여기는 이미지 계산뿐이다.
//! - 차이: numpy의 `int`(8바이트)·`float64` 배열 대신 `u8`·`bool`로 두고, `_defringe`의 `t`·거리는 화소마다 f64로 계산해 판정만 저장한다(같은 연산 순서 → 같은 결과)

use std::fmt;

use crate::ndimage::{self, Structure};
use crate::pil::{self, Img, Rgb};

pub const PHOTO_LONG_EDGE: usize = 1024;
pub const SHEET_PX: usize = 1024;
/// 배경과 다른 화소로 보는 RGB 차이 합
pub const BG_THRESHOLD: i32 = 55;
const BLOB_AMBIGUOUS: f64 = 0.25;
const POCKET_MAX_FRAC: f64 = 0.02;
const POCKET_MIN_PX: u64 = 16;
const POCKET_BG_SHARE: f64 = 0.9;
const FRINGE_BG_CHROMA: f64 = 128.0;
const FRINGE_T_MIN: f64 = 0.2;
const FRINGE_T_MAX: f64 = 1.3;
const FRINGE_DIST: f64 = 40.0;
const ITEM_PX: usize = 256;
const ITEM_PAD: usize = 16;
const ITEM_MIN_FRAC: f64 = 0.005;
const ITEM_SPECK_PX: u64 = 16;
const FRINGE_PEEL: usize = 3;
const FRINGE_STRONG_T: f64 = 0.35;
const FRINGE_STRONG_DIST: f64 = 60.0;
const FRINGE_REACH: usize = 2;

/// `SheetError`·`ItemImageError`와 그 밖의 이미지 오류(Python에서는 PIL 예외 등)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// 생성된 시트를 쓸 수 없음
    Sheet(String),
    /// 픽셀 아이템 그림에서 아이템을 찾지 못함
    ItemImage(String),
    /// 이미지를 열지 못하는 등 Python이면 PIL·numpy 예외가 되는 것
    Image(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Sheet(message)
            | EngineError::ItemImage(message)
            | EngineError::Image(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for EngineError {}

/// 서기 정면 컷과 시트 위 왼쪽 위 좌표
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cut {
    pub image: Img,
    pub left: usize,
    pub top: usize,
}

/// `_load` 결과 : RGB와, 투명 배경일 때만 알파 마스크
pub struct Loaded {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
    pub alpha: Option<Vec<bool>>,
}

/// `_load` : 정사각이며 1024보다 크면 NEAREST로 1024로 줄임
pub fn load(image: &Img) -> Loaded {
    let resized;
    let image = if image.width == image.height && image.width > SHEET_PX {
        resized = pil::resize_nearest(image, SHEET_PX, SHEET_PX);
        &resized
    } else {
        image
    };
    let mut rgb = Vec::with_capacity(image.width * image.height * 3);
    let mut alpha = Vec::with_capacity(image.width * image.height);
    for px in image.data.as_chunks::<4>().0.iter() {
        rgb.extend_from_slice(&px[..3]);
        alpha.push(px[3] >= 128);
    }
    let all = alpha.iter().all(|&on| on);
    Loaded {
        width: image.width,
        height: image.height,
        rgb,
        alpha: if all { None } else { Some(alpha) },
    }
}

fn open_loaded(png: &[u8]) -> Result<Loaded, EngineError> {
    let image = pil::open_rgba(png).map_err(EngineError::Image)?;
    Ok(load(&image))
}

/// 왼쪽 위 화소와 색이 다른 화소(`BG_THRESHOLD` 초과)
fn differs_from_corner(loaded: &Loaded) -> Vec<bool> {
    let background = [
        loaded.rgb[0] as i32,
        loaded.rgb[1] as i32,
        loaded.rgb[2] as i32,
    ];
    loaded
        .rgb
        .as_chunks::<3>()
        .0
        .iter()
        .map(|px| {
            (px[0] as i32 - background[0]).abs()
                + (px[1] as i32 - background[1]).abs()
                + (px[2] as i32 - background[2]).abs()
                > BG_THRESHOLD
        })
        .collect()
}

/// `_fill_holes` : 흰 털이 배경으로 빠진 구멍 복구, 배경색의 작은 구멍만 제외
fn fill_holes(blob: &[bool], loaded: &Loaded) -> Vec<bool> {
    let (width, height) = (loaded.width, loaded.height);
    let mut filled = ndimage::binary_fill_holes(blob, width, height);
    let holes_mask: Vec<bool> = filled
        .iter()
        .zip(blob)
        .map(|(&filled, &blob)| filled && !blob)
        .collect();
    let (holes, count) = ndimage::label(&holes_mask, width, height);
    if count == 0 {
        return filled;
    }
    let background = [
        loaded.rgb[0] as i32,
        loaded.rgb[1] as i32,
        loaded.rgb[2] as i32,
    ];
    let bgish: Vec<bool> = loaded
        .rgb
        .as_chunks::<3>()
        .0
        .iter()
        .map(|px| {
            (px[0] as i32 - background[0]).abs()
                + (px[1] as i32 - background[1]).abs()
                + (px[2] as i32 - background[2]).abs()
                <= BG_THRESHOLD
        })
        .collect();
    let sizes = ndimage::count_by_label(None, &holes, count);
    let pocket_px = ndimage::count_by_label(Some(&bgish), &holes, count);
    // Python은 매 구멍마다 `filled.sum()`을 다시 센다 : 지운 만큼 줄어든다
    let mut filled_count = filled.iter().filter(|&&on| on).count() as u64;
    for k in 0..count {
        let size = sizes[k];
        if POCKET_MIN_PX <= size
            && (size as f64) < POCKET_MAX_FRAC * filled_count as f64
            && pocket_px[k] as f64 >= POCKET_BG_SHARE * size as f64
        {
            for (at, &id) in holes.iter().enumerate() {
                if id as usize == k + 1 {
                    filled[at] = false;
                }
            }
            filled_count -= size;
        }
    }
    filled
}

/// `_defringe` : 검정~배경색 직선 위의 화소만 바깥부터 제거 (안쪽 같은 색 유지)
fn defringe(mask: Vec<bool>, loaded: &Loaded) -> Vec<bool> {
    let (width, height) = (loaded.width, loaded.height);
    let background = [
        loaded.rgb[0] as f64,
        loaded.rgb[1] as f64,
        loaded.rgb[2] as f64,
    ];
    let top = background.iter().cloned().fold(f64::MIN, f64::max);
    let low = background.iter().cloned().fold(f64::MAX, f64::min);
    if top - low < FRINGE_BG_CHROMA {
        return mask;
    }
    let norm = background[0] * background[0]
        + background[1] * background[1]
        + background[2] * background[2];
    let mut blend = vec![false; mask.len()];
    let mut strong_cond = vec![false; mask.len()];
    for (at, px) in loaded.rgb.as_chunks::<3>().0.iter().enumerate() {
        let p = [px[0] as f64, px[1] as f64, px[2] as f64];
        // 정수 곱·합이라 numpy의 행렬곱과 같은 값
        let t = (p[0] * background[0] + p[1] * background[1] + p[2] * background[2]) / norm;
        let distance = (p[0] - t * background[0]).abs()
            + (p[1] - t * background[1]).abs()
            + (p[2] - t * background[2]).abs();
        blend[at] = (FRINGE_T_MIN..=FRINGE_T_MAX).contains(&t) && distance <= FRINGE_DIST;
        strong_cond[at] =
            (FRINGE_STRONG_T..=FRINGE_T_MAX).contains(&t) && distance <= FRINGE_STRONG_DIST;
    }

    let mut mask = mask;
    for _ in 0..FRINGE_PEEL {
        let eroded = ndimage::binary_erosion(&mask, width, height, Structure::Cross, 1);
        let rim: Vec<bool> = (0..mask.len())
            .map(|at| mask[at] && !eroded[at] && blend[at])
            .collect();
        if !rim.iter().any(|&on| on) {
            break;
        }
        for (cell, &peeled) in mask.iter_mut().zip(&rim) {
            *cell &= !peeled;
        }
    }
    // 3겹보다 깊게 남은 마젠타 : 바깥에서 이어진 진한 화소만 제거
    let strong: Vec<bool> = (0..mask.len())
        .map(|at| mask[at] && strong_cond[at])
        .collect();
    let outside: Vec<bool> = mask.iter().map(|&on| !on).collect();
    let near_outside =
        ndimage::binary_dilation(&outside, width, height, Structure::Cross, FRINGE_REACH);
    let seeds: Vec<bool> = (0..mask.len())
        .map(|at| strong[at] && near_outside[at])
        .collect();
    let reached = ndimage::binary_propagation(&seeds, &strong, width, height);
    (0..mask.len()).map(|at| mask[at] && !reached[at]).collect()
}

/// `_crop` : 마스크의 바깥 상자로 자르고 투명한 화소는 RGB도 0
fn crop(loaded: &Loaded, mask: &[bool]) -> Result<(Img, usize, usize), EngineError> {
    let (width, height) = (loaded.width, loaded.height);
    let (mut x0, mut y0, mut x1, mut y1) = (width, height, 0, 0);
    for y in 0..height {
        for x in 0..width {
            if mask[y * width + x] {
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
    }
    if x0 > x1 {
        return Err(EngineError::Image(
            "zero-size array to reduction operation minimum which has no identity".into(),
        ));
    }
    let (out_w, out_h) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut data = vec![0u8; out_w * out_h * 4];
    for y in 0..out_h {
        for x in 0..out_w {
            let at = (y0 + y) * width + x0 + x;
            if mask[at] {
                let to = (y * out_w + x) * 4;
                data[to..to + 3].copy_from_slice(&loaded.rgb[at * 3..at * 3 + 3]);
                data[to + 3] = 255;
            }
        }
    }
    Ok((
        Img {
            width: out_w,
            height: out_h,
            data,
        },
        x0,
        y0,
    ))
}

pub fn stand_front(sheet_png: &[u8]) -> Result<Img, EngineError> {
    Ok(stand_front_cut(sheet_png)?.image)
}

/// `stand_front_cut` : 2×2 시트(윗줄 앉기, 아랫줄 서기 / 왼쪽 정면, 오른쪽 옆모습)의 서기 정면 컷과 시트 위 위치
pub fn stand_front_cut(sheet_png: &[u8]) -> Result<Cut, EngineError> {
    stand_front_cut_loaded(&open_loaded(sheet_png)?)
}

/// 이미 열어 둔 시트로 `stand_front_cut`
pub fn stand_front_cut_image(sheet: &Img) -> Result<Cut, EngineError> {
    stand_front_cut_loaded(&load(sheet))
}

fn stand_front_cut_loaded(loaded: &Loaded) -> Result<Cut, EngineError> {
    let (width, height) = (loaded.width, loaded.height);
    let mask = match &loaded.alpha {
        Some(alpha) => alpha.clone(),
        None => differs_from_corner(loaded),
    };
    let (labels, count) = ndimage::label(&mask, width, height);
    if count < 4 {
        return Err(EngineError::Sheet("캐릭터 네 마리를 찾지 못함".into()));
    }
    let sizes = ndimage::count_by_label(None, &labels, count);
    // 크기가 같은 덩어리의 순서는 결과에 영향이 없다(네 개 안이면 모두 쓰고, 경계면 오류)
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|&a, &b| sizes[b].cmp(&sizes[a]));
    let keep = &order[..4];
    if count > 4 && sizes[order[4]] as f64 >= BLOB_AMBIGUOUS * sizes[keep[3]] as f64 {
        return Err(EngineError::Sheet("캐릭터 조각이 떨어져 있음".into()));
    }

    let mut sum_y = vec![0u64; count];
    let mut sum_x = vec![0u64; count];
    for y in 0..height {
        for x in 0..width {
            let id = labels[y * width + x];
            if id != 0 {
                sum_y[id as usize - 1] += y as u64;
                sum_x[id as usize - 1] += x as u64;
            }
        }
    }
    let mut quadrants: [Option<usize>; 4] = [None; 4];
    for &k in keep {
        let n = sizes[k] as f64;
        let row = (sum_y[k] as f64 / n >= height as f64 / 2.0) as usize;
        let col = (sum_x[k] as f64 / n >= width as f64 / 2.0) as usize;
        if quadrants[row * 2 + col].is_some() {
            return Err(EngineError::Sheet("배치가 2×2가 아님".into()));
        }
        quadrants[row * 2 + col] = Some(k);
    }
    let chosen = quadrants[2].expect("네 덩어리가 서로 다른 칸이면 모든 칸이 찬다") as u32 + 1;
    let mut blob: Vec<bool> = labels.iter().map(|&id| id == chosen).collect();
    if loaded.alpha.is_none() {
        blob = defringe(fill_holes(&blob, loaded), loaded);
    }
    let (image, left, top) = crop(loaded, &blob)?;
    Ok(Cut { image, left, top })
}

/// `cut_item` : 단색 배경 그림 → 배경 투명, 아이템만 가운데 둔 `ITEM_PX` 정사각 PNG
///
/// - 배경색과 같은 색의 구멍(안경알·가방끈 안쪽)은 크기와 관계없이 투명
/// - 도트가 뭉개지지 않게 NEAREST로 줄임
pub fn cut_item(drawn_png: &[u8]) -> Result<Vec<u8>, EngineError> {
    Ok(pil::encode_png(&cut_item_image(&open_loaded(drawn_png)?)?))
}

pub fn cut_item_image(loaded: &Loaded) -> Result<Img, EngineError> {
    let (width, height) = (loaded.width, loaded.height);
    let mut mask = match &loaded.alpha {
        Some(alpha) => alpha.clone(),
        None => defringe(differs_from_corner(loaded), loaded),
    };
    let (labels, count) = ndimage::label(&mask, width, height);
    if count > 0 {
        let sizes = ndimage::count_by_label(None, &labels, count);
        mask = labels
            .iter()
            .map(|&id| id != 0 && sizes[id as usize - 1] >= ITEM_SPECK_PX)
            .collect();
    }
    let kept = mask.iter().filter(|&&on| on).count();
    if (kept as f64) < ITEM_MIN_FRAC * mask.len() as f64 {
        return Err(EngineError::ItemImage(
            "배경을 떼고 남은 그림이 없음".into(),
        ));
    }
    let (item, _, _) = crop(loaded, &mask)?;
    let box_px = (ITEM_PX - 2 * ITEM_PAD) as f64;
    let scale = box_px / item.width.max(item.height) as f64;
    let size = (
        pil::py_round(item.width as f64 * scale).max(1) as usize,
        pil::py_round(item.height as f64 * scale).max(1) as usize,
    );
    let item = pil::resize_nearest(&item, size.0, size.1);
    let mut canvas = Img::transparent(ITEM_PX, ITEM_PX);
    pil::paste(
        &mut canvas,
        &item,
        ((ITEM_PX - item.width) / 2) as i64,
        ((ITEM_PX - item.height) / 2) as i64,
    );
    Ok(canvas)
}

fn normalize(opened_orientation: u8, rgb: Rgb) -> Vec<u8> {
    let rgb = pil::transpose_exif(&rgb, opened_orientation);
    pil::encode_png_rgb(&pil::thumbnail(&rgb, PHOTO_LONG_EDGE))
}

/// FN-MYP-010-08 `normalize_photo` : 회전 적용·EXIF 제거·크기 제한 PNG
pub fn normalize_photo(raw: &[u8]) -> Result<Vec<u8>, EngineError> {
    let opened = pil::open(raw).map_err(EngineError::Image)?;
    Ok(normalize(opened.orientation, pil::to_rgb(&opened.image)))
}

/// FN-ADM-070-02 `normalize_item_image` : 투명 배경은 흰 바탕, 크기 제한 PNG
pub fn normalize_item_image(raw: &[u8]) -> Result<Vec<u8>, EngineError> {
    let opened = pil::open(raw).map_err(EngineError::Image)?;
    let rgba = pil::to_rgba(&opened.image);
    // Image.new("RGB", white).paste(image, mask=alpha) : 마스크 섞기는 `paste_mask_L`
    let mut data = Vec::with_capacity(rgba.width * rgba.height * 3);
    for px in rgba.data.as_chunks::<4>().0.iter() {
        let mask = px[3] as u32;
        for &channel in &px[..3] {
            data.push(match mask {
                255 => channel,
                0 => 255,
                m => {
                    let tmp = 255 * (255 - m) + channel as u32 * m + 128;
                    (((tmp >> 8) + tmp) >> 8) as u8
                }
            });
        }
    }
    let flat = Rgb {
        width: rgba.width,
        height: rgba.height,
        data,
    };
    Ok(normalize(opened.orientation, flat))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet_with_blobs(blobs: &[(usize, usize, usize, usize)]) -> Vec<u8> {
        // 마젠타 배경 1024×1024에 사각형 덩어리
        let mut image = Img {
            width: 1024,
            height: 1024,
            data: [[255, 0, 255, 255]; 1024 * 1024].concat(),
        };
        for &(x0, y0, w, h) in blobs {
            for y in y0..y0 + h {
                for x in x0..x0 + w {
                    let at = (y * 1024 + x) * 4;
                    image.data[at..at + 4].copy_from_slice(&[30, 160, 40, 255]);
                }
            }
        }
        pil::encode_png(&image)
    }

    #[test]
    fn picks_the_lower_left_blob_with_its_sheet_position() {
        let sheet = sheet_with_blobs(&[
            (100, 100, 200, 200),
            (600, 100, 200, 200),
            (120, 600, 180, 220),
            (620, 600, 200, 200),
        ]);
        let cut = stand_front_cut(&sheet).unwrap();
        assert_eq!((cut.left, cut.top), (120, 600));
        assert_eq!((cut.image.width, cut.image.height), (180, 220));
        assert_eq!(cut.image.pixel(0, 0), [30, 160, 40, 255]);
    }

    #[test]
    fn reports_the_same_sheet_errors_as_python() {
        let three = sheet_with_blobs(&[
            (100, 100, 200, 200),
            (600, 100, 200, 200),
            (120, 600, 180, 220),
        ]);
        assert_eq!(
            stand_front_cut(&three).unwrap_err(),
            EngineError::Sheet("캐릭터 네 마리를 찾지 못함".into())
        );
        let crowded = sheet_with_blobs(&[
            (100, 100, 200, 200),
            (150, 400, 200, 100),
            (120, 600, 180, 220),
            (620, 600, 200, 200),
        ]);
        assert_eq!(
            stand_front_cut(&crowded).unwrap_err(),
            EngineError::Sheet("배치가 2×2가 아님".into())
        );
        let stray = sheet_with_blobs(&[
            (100, 100, 200, 200),
            (600, 100, 200, 200),
            (120, 600, 180, 220),
            (620, 600, 200, 200),
            (870, 870, 150, 150),
        ]);
        assert_eq!(
            stand_front_cut(&stray).unwrap_err(),
            EngineError::Sheet("캐릭터 조각이 떨어져 있음".into())
        );
    }
}
