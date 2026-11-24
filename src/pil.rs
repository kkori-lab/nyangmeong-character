//! Pillow 13.0.0 : 이미지 열기·RGBA 변환, NEAREST 크기 바꾸기, 마스크 붙이기, `thumbnail`(`reduce` + Lanczos), PNG 저장
//!
//! 이미지 계산(`engine.py`·`canvas.py`·`bake.py`·`compose.py`)이 쓰는 Pillow 동작만 옮김
//! - 정수 반올림·고정소수점·float32 변환은 Pillow의 C 코드(`Geometry.c`·`Paste.c`·`Reduce.c`·`Resample.c`)와 같은 순서
//! - JPEG·손실 WebP 디코더는 Pillow(libjpeg-turbo·libwebp)와 화소가 조금 다름(`docs/design/migration.md` "알려진 차이")

use std::io::Cursor;

use image::{ColorType, DynamicImage, ImageDecoder, ImageEncoder, ImageReader};

/// RGBA 8비트 이미지(행 우선, 화소마다 R·G·B·A)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Img {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl Img {
    /// `Image.new("RGBA", size)` : 투명한 검정
    pub fn transparent(width: usize, height: usize) -> Img {
        Img {
            width,
            height,
            data: vec![0; width * height * 4],
        }
    }

    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let at = (y * self.width + x) * 4;
        [
            self.data[at],
            self.data[at + 1],
            self.data[at + 2],
            self.data[at + 3],
        ]
    }
}

/// RGB 8비트 이미지
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

/// 열어 본 이미지: 화소와 EXIF 방향(1~8, 없으면 1)
pub struct Opened {
    pub image: DynamicImage,
    pub orientation: u8,
}

/// `Image.open(io.BytesIO(raw))` + `.load()`
pub fn open(raw: &[u8]) -> Result<Opened, String> {
    let reader = ImageReader::new(Cursor::new(raw))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let mut decoder = reader.into_decoder().map_err(|error| error.to_string())?;
    let orientation = decoder
        .orientation()
        .map(|orientation| orientation.to_exif())
        .unwrap_or(1);
    let image = DynamicImage::from_decoder(decoder).map_err(|error| error.to_string())?;
    Ok(Opened { image, orientation })
}

/// `image.convert("RGBA")` (16비트는 Pillow처럼 위 바이트만, 회색은 255로 자름)
pub fn to_rgba(image: &DynamicImage) -> Img {
    let (width, height) = (image.width() as usize, image.height() as usize);
    let data = match image {
        DynamicImage::ImageRgba16(buffer) => {
            buffer.as_raw().iter().map(|v| (v >> 8) as u8).collect()
        }
        DynamicImage::ImageRgb16(buffer) => buffer
            .as_raw()
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|px| {
                [
                    (px[0] >> 8) as u8,
                    (px[1] >> 8) as u8,
                    (px[2] >> 8) as u8,
                    255,
                ]
            })
            .collect(),
        // 16비트 회색(Pillow "I;16")은 위 바이트가 아니라 255에서 잘린 값
        DynamicImage::ImageLuma16(buffer) => buffer
            .as_raw()
            .iter()
            .flat_map(|&v| {
                let luma = v.min(255) as u8;
                [luma, luma, luma, 255]
            })
            .collect(),
        DynamicImage::ImageLumaA16(buffer) => buffer
            .as_raw()
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|px| {
                let luma = (px[0] >> 8) as u8;
                [luma, luma, luma, (px[1] >> 8) as u8]
            })
            .collect(),
        other => other.to_rgba8().into_raw(),
    };
    Img {
        width,
        height,
        data,
    }
}

/// `image.convert("RGB")` : 투명도는 합성하지 않고 버림
pub fn to_rgb(image: &DynamicImage) -> Rgb {
    let rgba = to_rgba(image);
    Rgb {
        width: rgba.width,
        height: rgba.height,
        data: rgba
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|px| [px[0], px[1], px[2]])
            .collect(),
    }
}

/// `Image.open(...).convert("RGBA")`
pub fn open_rgba(raw: &[u8]) -> Result<Img, String> {
    Ok(to_rgba(&open(raw)?.image))
}

/// Python `round()` : 소수 .5는 짝수 쪽으로
pub fn py_round(value: f64) -> i64 {
    value.round_ties_even() as i64
}

/// `Image.resize(size, Image.NEAREST)` (`ImagingScaleAffine` : 입력 좌표를 `+=`로 쌓음)
pub fn resize_nearest(image: &Img, width: usize, height: usize) -> Img {
    let x_step = image.width as f64 / width as f64;
    let y_step = image.height as f64 / height as f64;
    let coord = |value: f64| if value < 0.0 { -1 } else { value as i64 };

    let mut x_table = vec![-1i64; width];
    let mut xx = x_step * 0.5;
    for slot in x_table.iter_mut() {
        let at = coord(xx);
        if at >= 0 && (at as usize) < image.width {
            *slot = at;
        }
        xx += x_step;
    }
    let mut out = Img::transparent(width, height);
    let mut yy = y_step * 0.5;
    for y in 0..height {
        let row = coord(yy);
        if row >= 0 && (row as usize) < image.height {
            for (x, &at) in x_table.iter().enumerate() {
                if at >= 0 {
                    let from = (row as usize * image.width + at as usize) * 4;
                    let to = (y * width + x) * 4;
                    out.data[to..to + 4].copy_from_slice(&image.data[from..from + 4]);
                }
            }
        }
        yy += y_step;
    }
    out
}

fn div255(value: u32) -> u8 {
    let tmp = value + 128;
    (((tmp >> 8) + tmp) >> 8) as u8
}

fn clip_region(
    dest: &Img,
    source: &Img,
    x: i64,
    y: i64,
) -> Option<(usize, usize, usize, usize, usize, usize)> {
    let (mut width, mut height) = (source.width as i64, source.height as i64);
    let (mut dx, mut dy) = (x, y);
    let (mut sx, mut sy) = (0i64, 0i64);
    if dx < 0 {
        width += dx;
        sx = -dx;
        dx = 0;
    }
    if dy < 0 {
        height += dy;
        sy = -dy;
        dy = 0;
    }
    if dx + width > dest.width as i64 {
        width = dest.width as i64 - dx;
    }
    if dy + height > dest.height as i64 {
        height = dest.height as i64 - dy;
    }
    if width <= 0 || height <= 0 {
        return None;
    }
    Some((
        dx as usize,
        dy as usize,
        sx as usize,
        sy as usize,
        width as usize,
        height as usize,
    ))
}

/// `dest.paste(source, (x, y), source)` : 마스크는 source의 알파 (`paste_mask_RGBA`)
pub fn paste_masked(dest: &mut Img, source: &Img, x: i64, y: i64) {
    let Some((dx, dy, sx, sy, width, height)) = clip_region(dest, source, x, y) else {
        return;
    };
    for row in 0..height {
        for col in 0..width {
            let from = ((sy + row) * source.width + sx + col) * 4;
            let to = ((dy + row) * dest.width + dx + col) * 4;
            let mask = source.data[from + 3];
            if mask == 255 {
                dest.data[to..to + 4].copy_from_slice(&source.data[from..from + 4]);
            } else if mask != 0 {
                for channel in 0..4 {
                    let blended = dest.data[to + channel] as u32 * (255 - mask as u32)
                        + source.data[from + channel] as u32 * mask as u32;
                    dest.data[to + channel] = div255(blended);
                }
            }
        }
    }
}

/// `dest.paste(source, (x, y))` : 마스크 없이 덮어쓰기
pub fn paste(dest: &mut Img, source: &Img, x: i64, y: i64) {
    let Some((dx, dy, sx, sy, width, height)) = clip_region(dest, source, x, y) else {
        return;
    };
    for row in 0..height {
        let from = ((sy + row) * source.width + sx) * 4;
        let to = ((dy + row) * dest.width + dx) * 4;
        dest.data[to..to + width * 4].copy_from_slice(&source.data[from..from + width * 4]);
    }
}

fn png_bytes(data: &[u8], width: usize, height: usize, color: ColorType) -> Vec<u8> {
    let mut buffer = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(
        &mut buffer,
        image::codecs::png::CompressionType::Best,
        image::codecs::png::FilterType::Adaptive,
    )
    .write_image(data, width as u32, height as u32, color.into())
    .expect("PNG 인코딩은 메모리에 쓰므로 실패하지 않는다");
    buffer
}

/// `image.save(buffer, "PNG", optimize=True)` (압축 결과 바이트는 Pillow와 다르고 화소는 같음)
pub fn encode_png(image: &Img) -> Vec<u8> {
    png_bytes(&image.data, image.width, image.height, ColorType::Rgba8)
}

pub fn encode_png_rgb(image: &Rgb) -> Vec<u8> {
    png_bytes(&image.data, image.width, image.height, ColorType::Rgb8)
}

/// `ImageOps.exif_transpose` 가 쓰는 8가지 방향 바꾸기 (Pillow `Transpose`)
pub fn transpose_exif(image: &Rgb, orientation: u8) -> Rgb {
    let (w, h) = (image.width, image.height);
    // (새 너비, 새 높이, 새 (x, y)에 놓일 원본 좌표)
    type Source = fn(usize, usize, usize, usize) -> (usize, usize);
    let (new_w, new_h, source): (usize, usize, Source) = match orientation {
        2 => (w, h, |x, y, w, _| (w - 1 - x, y)),
        3 => (w, h, |x, y, w, h| (w - 1 - x, h - 1 - y)),
        4 => (w, h, |x, y, _, h| (x, h - 1 - y)),
        5 => (h, w, |x, y, _, _| (y, x)),
        6 => (h, w, |x, y, _, h| (y, h - 1 - x)),
        7 => (h, w, |x, y, w, h| (w - 1 - y, h - 1 - x)),
        8 => (h, w, |x, y, w, _| (w - 1 - y, x)),
        _ => return image.clone(),
    };
    let mut data = Vec::with_capacity(image.data.len());
    for y in 0..new_h {
        for x in 0..new_w {
            let (sx, sy) = source(x, y, w, h);
            let at = (sy * w + sx) * 3;
            data.extend_from_slice(&image.data[at..at + 3]);
        }
    }
    Rgb {
        width: new_w,
        height: new_h,
        data,
    }
}

// ---- thumbnail : Image.reduce + ImagingResample(LANCZOS) ----

const PRECISION_BITS: u32 = 32 - 8 - 2;
const LANCZOS_SUPPORT: f64 = 3.0;

/// `division_UINT32(divider, 8)` : float로 `2^32 / (256 × divider)` 를 구해 버림
fn division_u32(divider: u64) -> u64 {
    let max_dividend = (256 * divider) as u32;
    (4294967296.0f32 / max_dividend as f32) as u32 as u64
}

/// `Image.reduce((fx, fy), box)` 의 RGB 평균 (`Reduce.c` : ((합 + 칸 수/2) × 승수) >> 24, 가장자리 조각 블록도 같은 식)
fn reduce(image: &Rgb, factor: (usize, usize), area: (usize, usize, usize, usize)) -> Rgb {
    let (x0, y0, x1, y1) = area;
    let (box_w, box_h) = (x1 - x0, y1 - y0);
    let out_w = box_w.div_ceil(factor.0);
    let out_h = box_h.div_ceil(factor.1);
    let mut data = vec![0u8; out_w * out_h * 3];
    for oy in 0..out_h {
        let from_y = y0 + oy * factor.1;
        let to_y = (from_y + factor.1).min(y1);
        for ox in 0..out_w {
            let from_x = x0 + ox * factor.0;
            let to_x = (from_x + factor.0).min(x1);
            let count = ((to_x - from_x) * (to_y - from_y)) as u64;
            let amend = count / 2;
            let multiplier = division_u32(count);
            let mut sums = [0u64; 3];
            for y in from_y..to_y {
                for x in from_x..to_x {
                    let at = (y * image.width + x) * 3;
                    for (channel, sum) in sums.iter_mut().enumerate() {
                        *sum += image.data[at + channel] as u64;
                    }
                }
            }
            for (channel, sum) in sums.iter().enumerate() {
                data[(oy * out_w + ox) * 3 + channel] = (((sum + amend) * multiplier) >> 24) as u8;
            }
        }
    }
    Rgb {
        width: out_w,
        height: out_h,
        data,
    }
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    let x = x * std::f64::consts::PI;
    x.sin() / x
}

fn lanczos(x: f64) -> f64 {
    if (-3.0..3.0).contains(&x) {
        sinc(x) * sinc(x / 3.0)
    } else {
        0.0
    }
}

/// 출력 한 칸마다 (쓰는 입력의 시작, 개수)와 고정소수점 계수(`ksize`개씩)
struct Coeffs {
    ksize: usize,
    bounds: Vec<(usize, usize)>,
    kernels: Vec<i32>,
}

/// `precompute_coeffs` + `normalize_coeffs_8bpc`
fn coefficients(in_size: usize, in0: f32, in1: f32, out_size: usize) -> Coeffs {
    // 인자는 float(차이도 float), 나눗셈은 double : Pillow `precompute_coeffs`
    let scale = (in1 - in0) as f64 / out_size as f64;
    let filter_scale = scale.max(1.0);
    let support = LANCZOS_SUPPORT * filter_scale;
    let ksize = support.ceil() as usize * 2 + 1;
    let mut bounds = Vec::with_capacity(out_size);
    let mut kernels = vec![0i32; out_size * ksize];
    for xx in 0..out_size {
        let center = in0 as f64 + (xx as f64 + 0.5) * scale;
        let ss = 1.0 / filter_scale;
        let xmin = ((center - support + 0.5) as i64).max(0);
        let xmax = (((center + support + 0.5) as i64).min(in_size as i64) - xmin).max(0);
        let mut weights: Vec<f64> = (0..xmax)
            .map(|x| lanczos((x as f64 + xmin as f64 - center + 0.5) * ss))
            .collect();
        let total: f64 = weights.iter().sum();
        if total != 0.0 {
            weights.iter_mut().for_each(|w| *w /= total);
        }
        for (x, w) in weights.iter().enumerate() {
            kernels[xx * ksize + x] = if *w < 0.0 {
                (-0.5 + w * (1 << PRECISION_BITS) as f64) as i32
            } else {
                (0.5 + w * (1 << PRECISION_BITS) as f64) as i32
            };
        }
        bounds.push((xmin as usize, xmax as usize));
    }
    Coeffs {
        ksize,
        bounds,
        kernels,
    }
}

fn clip8(sum: i64) -> u8 {
    (sum >> PRECISION_BITS).clamp(0, 255) as u8
}

/// 가로 패스 : 원본의 `row_offset`행부터 `rows`행을 `out_w` 너비로
fn horizontal_pass(
    image: &Rgb,
    row_offset: usize,
    rows: usize,
    out_w: usize,
    coeffs: &Coeffs,
) -> Rgb {
    let mut data = vec![0u8; out_w * rows * 3];
    for y in 0..rows {
        let source = (y + row_offset) * image.width * 3;
        for (x, &(xmin, count)) in coeffs.bounds.iter().enumerate() {
            let kernel = &coeffs.kernels[x * coeffs.ksize..x * coeffs.ksize + count];
            for channel in 0..3 {
                let mut sum = 1i64 << (PRECISION_BITS - 1);
                for (k, weight) in kernel.iter().enumerate() {
                    sum += image.data[source + (xmin + k) * 3 + channel] as i64 * *weight as i64;
                }
                data[(y * out_w + x) * 3 + channel] = clip8(sum);
            }
        }
    }
    Rgb {
        width: out_w,
        height: rows,
        data,
    }
}

/// 세로 패스 : 모든 열을 `out_h` 높이로
fn vertical_pass(image: &Rgb, out_h: usize, coeffs: &Coeffs) -> Rgb {
    let row_len = image.width * 3;
    let mut data = vec![0u8; row_len * out_h];
    for (y, &(ymin, count)) in coeffs.bounds.iter().enumerate() {
        let kernel = &coeffs.kernels[y * coeffs.ksize..y * coeffs.ksize + count];
        for x in 0..row_len {
            let mut sum = 1i64 << (PRECISION_BITS - 1);
            for (k, weight) in kernel.iter().enumerate() {
                sum += image.data[(ymin + k) * row_len + x] as i64 * *weight as i64;
            }
            data[y * row_len + x] = clip8(sum);
        }
    }
    Rgb {
        width: image.width,
        height: out_h,
        data,
    }
}

/// `ImagingResampleInner` (Pillow 13.0.0) : 보통 가로 먼저(쓰는 행만), 세로가 가로보다 두 배 넘게 줄면 세로 먼저.
/// 중간 값은 8비트로 반올림
fn resample(image: &Rgb, out_w: usize, out_h: usize, area: (f32, f32, f32, f32)) -> Rgb {
    let need_horizontal = out_w != image.width || area.0 != 0.0 || area.2 != out_w as f32;
    let need_vertical = out_h != image.height || area.1 != 0.0 || area.3 != out_h as f32;
    let shrink_h = image.height as i64 - out_h as i64;
    let shrink_w = image.width as i64 - out_w as i64;
    let horizontal_first = !(shrink_h > 0 && shrink_h > shrink_w * 2);

    let mut vertical = (need_horizontal && horizontal_first || need_vertical)
        .then(|| coefficients(image.height, area.1, area.3, out_h));
    let horizontal = need_horizontal.then(|| coefficients(image.width, area.0, area.2, out_w));

    let mut current = image.clone();
    if horizontal_first {
        if let (Some(h), Some(v)) = (&horizontal, vertical.as_mut()) {
            let first = v.bounds[0].0;
            let last = v.bounds[out_h - 1].0 + v.bounds[out_h - 1].1;
            for bound in v.bounds.iter_mut() {
                bound.0 -= first;
            }
            current = horizontal_pass(&current, first, last - first, out_w, h);
        }
        if let (true, Some(v)) = (need_vertical, &vertical) {
            current = vertical_pass(&current, out_h, v);
        }
    } else {
        if let (true, Some(v)) = (need_vertical, &vertical) {
            current = vertical_pass(&current, out_h, v);
        }
        if let Some(h) = &horizontal {
            current = horizontal_pass(&current, 0, out_h, out_w, h);
        }
    }
    current
}

/// `Image.thumbnail((max_edge, max_edge), Image.LANCZOS)` (`reducing_gap=2.0`, RGB)
pub fn thumbnail(image: &Rgb, max_edge: usize) -> Rgb {
    let (width, height) = (image.width, image.height);
    if max_edge >= width && max_edge >= height {
        return image.clone();
    }
    let aspect = width as f64 / height as f64;
    let (mut x, mut y) = (max_edge, max_edge);
    // round_aspect : floor·ceil 중 가까운 쪽(같으면 floor), 최소 1
    let round_aspect = |number: f64, key: &dyn Fn(f64) -> f64| -> usize {
        let (floor, ceil) = (number.floor(), number.ceil());
        let pick = if key(ceil) < key(floor) { ceil } else { floor };
        (pick as usize).max(1)
    };
    if x as f64 / y as f64 >= aspect {
        x = round_aspect(y as f64 * aspect, &|n| (aspect - n / y as f64).abs());
    } else {
        y = round_aspect(x as f64 / aspect, &|n| {
            if n == 0.0 {
                0.0
            } else {
                (aspect - x as f64 / n).abs()
            }
        });
    }
    if (width, height) == (x, y) {
        return image.clone();
    }

    let mut source = image;
    let reduced;
    let mut area = (0.0f64, 0.0f64, width as f64, height as f64);
    let factor_x = ((width as f64 / x as f64 / 2.0) as usize).max(1);
    let factor_y = ((height as f64 / y as f64 / 2.0) as usize).max(1);
    if factor_x > 1 || factor_y > 1 {
        // _get_safe_box : Lanczos 지원 범위(3.0 - 0.5)만큼 여유를 둔 상자
        let support_x = (LANCZOS_SUPPORT - 0.5) * (width as f64 / x as f64);
        let support_y = (LANCZOS_SUPPORT - 0.5) * (height as f64 / y as f64);
        let safe = (
            (0.0 - support_x).max(0.0) as usize,
            (0.0 - support_y).max(0.0) as usize,
            ((width as f64 + support_x).ceil() as usize).min(width),
            ((height as f64 + support_y).ceil() as usize).min(height),
        );
        reduced = reduce(image, (factor_x, factor_y), safe);
        source = &reduced;
        area = (
            (area.0 - safe.0 as f64) / factor_x as f64,
            (area.1 - safe.1 as f64) / factor_y as f64,
            (area.2 - safe.0 as f64) / factor_x as f64,
            (area.3 - safe.1 as f64) / factor_y as f64,
        );
    }
    resample(
        source,
        x,
        y,
        (area.0 as f32, area.1 as f32, area.2 as f32, area.3 as f32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn py_round_goes_to_even() {
        assert_eq!(py_round(0.5), 0);
        assert_eq!(py_round(1.5), 2);
        assert_eq!(py_round(2.5), 2);
        assert_eq!(py_round(-0.5), 0);
        assert_eq!(py_round(341.4999), 341);
    }

    #[test]
    fn paste_clips_to_the_canvas() {
        let mut canvas = Img::transparent(4, 4);
        let tile = Img {
            width: 2,
            height: 2,
            data: [[9, 8, 7, 255]; 4].concat(),
        };
        paste_masked(&mut canvas, &tile, -1, 3);
        assert_eq!(canvas.pixel(0, 3), [9, 8, 7, 255]);
        assert_eq!(canvas.pixel(1, 3), [0, 0, 0, 0]);
        paste_masked(&mut canvas, &tile, 10, 10);
    }

    #[test]
    fn half_alpha_blends_like_pillow() {
        let mut canvas = Img {
            width: 1,
            height: 1,
            data: vec![200, 100, 0, 255],
        };
        let top = Img {
            width: 1,
            height: 1,
            data: vec![0, 0, 100, 128],
        };
        paste_masked(&mut canvas, &top, 0, 0);
        // Pillow 13.0.0 (2026-11-16 확인) : [(100, 50, 50, 191)]
        assert_eq!(canvas.pixel(0, 0), [100, 50, 50, 191]);
    }

    #[test]
    fn exif_orientation_six_turns_a_landscape_clockwise() {
        // 2×1 [A B] → 6(시계 방향 90도): 위 A, 아래 B
        let image = Rgb {
            width: 2,
            height: 1,
            data: vec![1, 1, 1, 2, 2, 2],
        };
        let turned = transpose_exif(&image, 6);
        assert_eq!((turned.width, turned.height), (1, 2));
        assert_eq!(turned.data, vec![1, 1, 1, 2, 2, 2]);
        let turned = transpose_exif(&image, 8);
        assert_eq!(turned.data, vec![2, 2, 2, 1, 1, 1]);
    }

    #[test]
    fn thumbnail_keeps_aspect_and_skips_small_images() {
        let small = Rgb {
            width: 10,
            height: 5,
            data: vec![7; 150],
        };
        assert_eq!(thumbnail(&small, 1024), small);
        let wide = Rgb {
            width: 2048,
            height: 1000,
            data: vec![128; 2048 * 1000 * 3],
        };
        let result = thumbnail(&wide, 1024);
        assert_eq!((result.width, result.height), (1024, 500));
        assert!(result.data.iter().all(|&v| v == 128));
    }
}
