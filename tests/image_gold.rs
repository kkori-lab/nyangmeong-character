//! 이미지 계산 정답 비교 : 원본 Python(`contract/gold_images.py`)이 만든 PNG·값과 픽셀 단위로 같은지 확인
//!
//! - 합성 사례(`tests/fixtures/image/`, 실제 반려동물 아님)는 항상 실행
//! - 실제 시트는 `CHARACTER_SHEETS_DIR`(bare.png, worn_<부위코드>.png)와 `CHARACTER_GOLD_DIR`가 있을 때만 실행(커밋하지 않음)
//! - PNG 바이트는 압축기가 달라 다름 : 디코드한 RGBA 화소로 비교

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nyangmeong_character::bake;
use nyangmeong_character::canvas::{self, Layout};
use nyangmeong_character::compose;
use nyangmeong_character::engine::{self, EngineError};
use nyangmeong_character::pil::{self, Img};
use serde_json::Value;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/image")
}

fn json(path: &Path) -> Option<Value> {
    let bytes = std::fs::read(path).ok()?;
    Some(serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
}

fn png(path: &Path) -> Img {
    pil::open_rgba(&std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
        .unwrap()
}

/// 같으면 `None`, 다르면 어디가 다른지
fn diff(actual: &Img, want: &Img) -> Option<String> {
    if (actual.width, actual.height) != (want.width, want.height) {
        return Some(format!(
            "크기 {}×{} ≠ 정답 {}×{}",
            actual.width, actual.height, want.width, want.height
        ));
    }
    let mut count = 0;
    let mut first = None;
    for y in 0..want.height {
        for x in 0..want.width {
            if actual.pixel(x, y) != want.pixel(x, y) {
                count += 1;
                first.get_or_insert((x, y, actual.pixel(x, y), want.pixel(x, y)));
            }
        }
    }
    first.map(|(x, y, a, w)| format!("화소 {count}개 다름, 처음 ({x}, {y}): {a:?} ≠ 정답 {w:?}"))
}

struct Check {
    failures: Vec<String>,
    label: String,
}

impl Check {
    fn fail(&mut self, what: &str, why: String) {
        self.failures
            .push(format!("[{}] {what}: {why}", self.label));
    }

    fn image(&mut self, what: &str, actual: &Img, gold: &Path) {
        if let Some(why) = diff(actual, &png(gold)) {
            self.fail(what, why);
        }
    }

    fn same<T: PartialEq + std::fmt::Debug>(&mut self, what: &str, actual: T, want: T) {
        if actual != want {
            self.fail(what, format!("{actual:?} ≠ 정답 {want:?}"));
        }
    }
}

/// 정답의 float는 repr 문자열(serde_json의 기본 실수 읽기는 마지막 한 비트가 틀릴 수 있음)
fn number(value: &Value) -> f64 {
    value
        .as_str()
        .unwrap_or_else(|| panic!("float 문자열이 아님: {value}"))
        .parse()
        .unwrap_or_else(|_| panic!("float가 아님: {value}"))
}

fn point(value: &Value) -> (f64, f64) {
    (number(&value[0]), number(&value[1]))
}

fn error_text(error: &EngineError) -> String {
    error.to_string()
}

/// 시트 한 사례(맨몸 + 부위별 착용)의 모든 정답 비교
fn check_sheets(label: &str, input: &Path, gold: &Path) -> Vec<String> {
    let mut check = Check {
        failures: Vec::new(),
        label: label.to_string(),
    };
    let bare = std::fs::read(input.join("bare.png")).unwrap();
    let mut worn: HashMap<String, Vec<u8>> = HashMap::new();
    for entry in std::fs::read_dir(input).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if let Some(regn) = name
            .strip_prefix("worn_")
            .and_then(|n| n.strip_suffix(".png"))
        {
            worn.insert(regn.to_string(), std::fs::read(&path).unwrap());
        }
    }

    let cut_gold = json(&gold.join("cut.json")).expect("cut.json");
    let cut = engine::stand_front_cut(&bare);
    if let Some(message) = cut_gold.get("error") {
        match cut {
            Err(error) => check.same(
                "cut 오류",
                error_text(&error),
                message.as_str().unwrap().into(),
            ),
            Ok(_) => check.fail("cut", "오류가 나야 하는데 성공".into()),
        }
        return check.failures;
    }
    let cut = match cut {
        Ok(cut) => cut,
        Err(error) => {
            check.fail("cut", format!("오류 {error}"));
            return check.failures;
        }
    };
    check.image("cut", &cut.image, &gold.join("cut.png"));
    check.same(
        "cut.left",
        cut.left as u64,
        cut_gold["left"].as_u64().unwrap(),
    );
    check.same("cut.top", cut.top as u64, cut_gold["top"].as_u64().unwrap());

    // place·layout
    let place_gold = json(&gold.join("place.json")).expect("place.json");
    let placement = canvas::place(&cut.image);
    let layout = match (&placement, place_gold.get("none")) {
        (None, Some(_)) => Layout {
            scale: 0.8,
            origin_x: 40.0,
            origin_y: 60.0,
        },
        (None, None) => {
            check.fail("place", "눈을 못 찾았는데 정답에는 있음".into());
            return check.failures;
        }
        (Some(_), Some(_)) => {
            check.fail("place", "정답은 눈을 못 찾음".into());
            return check.failures;
        }
        (Some(placement), None) => {
            check.image("place", &placement.image, &gold.join("place.png"));
            check.same(
                "place.eyes.0",
                placement.eyes.0,
                point(&place_gold["eyes"][0]),
            );
            check.same(
                "place.eyes.1",
                placement.eyes.1,
                point(&place_gold["eyes"][1]),
            );
            check.same("place.scale", placement.scale, number(&place_gold["scale"]));
            check.same(
                "place.origin",
                placement.origin,
                point(&place_gold["origin"]),
            );
            check.same(
                "place.clipped",
                placement.clipped,
                place_gold["clipped"].as_bool().unwrap(),
            );
            check.same(
                "place.canvas_eyes",
                placement.canvas_eyes(),
                (
                    point(&place_gold["canvas_eyes"][0]),
                    point(&place_gold["canvas_eyes"][1]),
                ),
            );
            placement.layout()
        }
    };
    let layout_gold = json(&gold.join("layout.json")).expect("layout.json");
    check.same("layout.scale", layout.scale, number(&layout_gold["scale"]));
    check.same(
        "layout.origin_x",
        layout.origin_x,
        number(&layout_gold["origin_x"]),
    );
    check.same(
        "layout.origin_y",
        layout.origin_y,
        number(&layout_gold["origin_y"]),
    );

    let base = bake::layer_base(&bare).unwrap();
    let base_gold = json(&gold.join("layer_base.json")).expect("layer_base.json");
    check.same(
        "layer_base.lit",
        base.lit,
        base_gold["lit"].as_bool().unwrap(),
    );
    check.same(
        "layer_base.left",
        base.cut.left as u64,
        base_gold["left"].as_u64().unwrap(),
    );
    check.same(
        "layer_base.top",
        base.cut.top as u64,
        base_gold["top"].as_u64().unwrap(),
    );
    check.image("layer_base", &base.cut.image, &gold.join("layer_base.png"));

    let mut regns: Vec<&String> = worn.keys().collect();
    regns.sort();
    for regn in regns {
        let sheet = &worn[regn];
        for (what, source_of, file) in [
            ("bake_source", &bare, format!("bake_source_{regn}")),
            (
                "bake_source(착용)",
                sheet,
                format!("bake_source_worn_{regn}"),
            ),
        ] {
            let want = json(&gold.join(format!("{file}.json"))).expect("bake_source json");
            let got = bake::bake_source(source_of, regn).unwrap();
            let lit = got != *source_of;
            check.same(
                &format!("{what} {regn} lit"),
                lit,
                want["lit"].as_bool().unwrap(),
            );
            if lit && want["lit"].as_bool().unwrap() {
                check.image(
                    &format!("{what} {regn}"),
                    &pil::open_rgba(&got).unwrap(),
                    &gold.join(format!("{file}.png")),
                );
            }
        }

        let validate_gold =
            json(&gold.join(format!("validate_{regn}.json"))).expect("validate json");
        let validated = bake::validate(&bare, sheet, regn);
        match (validated, validate_gold.get("error")) {
            (Ok(()), None) => {}
            (Err(error), Some(message)) => {
                check.same(
                    &format!("validate {regn} 오류"),
                    error_text(&error),
                    message.as_str().unwrap().into(),
                );
            }
            (Ok(()), Some(message)) => check.fail(
                &format!("validate {regn}"),
                format!("오류가 나야 함: {message}"),
            ),
            (Err(error), None) => check.fail(&format!("validate {regn}"), format!("오류 {error}")),
        }

        let worn_gold = json(&gold.join(format!("worn_cut_{regn}.json"))).expect("worn_cut json");
        let worn_cut = match engine::stand_front_cut(sheet) {
            Ok(cut) => cut,
            Err(error) => {
                check.same(
                    &format!("worn_cut {regn} 오류"),
                    error_text(&error),
                    worn_gold["error"]
                        .as_str()
                        .unwrap_or("(성공해야 함)")
                        .into(),
                );
                continue;
            }
        };
        check.image(
            &format!("worn_cut {regn}"),
            &worn_cut.image,
            &gold.join(format!("worn_cut_{regn}.png")),
        );
        check.same(
            &format!("worn_cut {regn} left"),
            worn_cut.left as u64,
            worn_gold["left"].as_u64().unwrap(),
        );
        check.same(
            &format!("worn_cut {regn} top"),
            worn_cut.top as u64,
            worn_gold["top"].as_u64().unwrap(),
        );

        let sprite_gold = json(&gold.join(format!("sprite_{regn}.json"))).expect("sprite json");
        let sprite = if regn == bake::REGN_HEAD {
            bake::worn_sprite(&engine::stand_front_cut(&bare).unwrap(), &worn_cut)
        } else {
            bake::worn_sprite(&base.cut, &worn_cut)
        };
        check.same(
            &format!("sprite {regn} x"),
            sprite.x,
            sprite_gold["x"].as_i64().unwrap(),
        );
        check.same(
            &format!("sprite {regn} y"),
            sprite.y,
            sprite_gold["y"].as_i64().unwrap(),
        );
        if regn == bake::REGN_HEAD {
            continue;
        }
        let layer_gold = json(&gold.join(format!("layer_{regn}.json"))).expect("layer json");
        let layer = bake::item_layer(&base.cut.image, &sprite, base.lit);
        match (layer, layer_gold.get("none")) {
            (None, Some(_)) => {}
            (Some(layer), None) => {
                check.image(
                    &format!("layer {regn}"),
                    &layer.image,
                    &gold.join(format!("layer_{regn}.png")),
                );
                check.same(
                    &format!("layer {regn} x"),
                    layer.x,
                    layer_gold["x"].as_i64().unwrap(),
                );
                check.same(
                    &format!("layer {regn} y"),
                    layer.y,
                    layer_gold["y"].as_i64().unwrap(),
                );
            }
            (got, _) => check.fail(
                &format!("layer {regn}"),
                format!("비었는지가 다름: {}", got.is_some()),
            ),
        }
    }

    // 합성
    let mut renders: Vec<(String, Vec<String>)> = vec![("bare".into(), vec![])];
    let mut all: Vec<String> = worn.keys().cloned().collect();
    all.sort();
    if !all.is_empty() {
        renders.push(("all".into(), all.clone()));
    }
    for regn in &all {
        renders.push((regn.clone(), vec![regn.clone()]));
    }
    if worn.contains_key("MYPC0001") && worn.len() > 1 {
        renders.push((
            "head_clothes".into(),
            ["MYPC0001", "MYPC0004"]
                .iter()
                .filter(|r| worn.contains_key(**r))
                .map(|r| r.to_string())
                .collect(),
        ));
    }
    for (name, selected) in renders {
        let Some(want) = json(&gold.join(format!("render_{name}.json"))) else {
            continue;
        };
        let chosen: HashMap<String, Vec<u8>> = selected
            .iter()
            .map(|r| (r.clone(), worn[r].clone()))
            .collect();
        match (compose::render(&bare, &layout, &chosen), want.get("error")) {
            (Ok(bytes), None) => check.image(
                &format!("render {name}"),
                &pil::open_rgba(&bytes).unwrap(),
                &gold.join(format!("render_{name}.png")),
            ),
            (Err(error), Some(message)) => check.same(
                &format!("render {name} 오류"),
                error_text(&error),
                message.as_str().unwrap().into(),
            ),
            (Ok(_), Some(message)) => check.fail(
                &format!("render {name}"),
                format!("오류가 나야 함: {message}"),
            ),
            (Err(error), None) => check.fail(&format!("render {name}"), format!("오류 {error}")),
        }
    }
    check.failures
}

fn report(failures: Vec<String>) {
    assert!(
        failures.is_empty(),
        "정답과 다름 {}건:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn synthetic_sheets_match_python() {
    let mut failures = Vec::new();
    let mut folders: Vec<PathBuf> = std::fs::read_dir(fixtures().join("sheets"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    folders.sort();
    assert!(folders.len() >= 8);
    for folder in folders {
        let label = folder.file_name().unwrap().to_string_lossy().to_string();
        failures.extend(check_sheets(&label, &folder, &folder.join("gold")));
    }
    report(failures);
}

#[test]
fn real_sheets_match_python() {
    let Some(sheets) = std::env::var_os("CHARACTER_SHEETS_DIR") else {
        println!(
            "건너뜀: CHARACTER_SHEETS_DIR(bare.png·worn_<부위코드>.png)가 없어 실제 시트 비교를 하지 않는다 (정답은 sh scripts/gold.sh 가 만든다)"
        );
        return;
    };
    let gold = std::env::var_os("CHARACTER_GOLD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/character-gold"));
    assert!(
        gold.join("cut.json").exists(),
        "정답이 없습니다: sh scripts/gold.sh 로 만드세요 ({})",
        gold.display()
    );
    report(check_sheets("실제", Path::new(&sheets), &gold));
}

#[test]
fn item_cutting_matches_python() {
    let mut failures = Vec::new();
    let mut count = 0;
    for entry in std::fs::read_dir(fixtures().join("items")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let Some(stem) = name.strip_suffix(".png").filter(|s| !s.ends_with(".gold")) else {
            continue;
        };
        count += 1;
        let mut check = Check {
            failures: Vec::new(),
            label: format!("item {stem}"),
        };
        let raw = std::fs::read(&path).unwrap();
        let gold_json = path.with_file_name(format!("{stem}.gold.json"));
        match (engine::cut_item(&raw), json(&gold_json)) {
            (Ok(bytes), None) => check.image(
                "cut_item",
                &pil::open_rgba(&bytes).unwrap(),
                &path.with_file_name(format!("{stem}.gold.png")),
            ),
            (Err(error), Some(want)) => {
                check.same(
                    "오류",
                    error_text(&error),
                    want["error"].as_str().unwrap().into(),
                );
                check.same(
                    "오류 종류",
                    matches!(error, EngineError::ItemImage(_)),
                    want["kind"] == "item_image",
                );
            }
            (Ok(_), Some(want)) => {
                check.fail("cut_item", format!("오류가 나야 함: {}", want["error"]))
            }
            (Err(error), None) => check.fail("cut_item", format!("오류 {error}")),
        }
        failures.extend(check.failures);
    }
    assert!(count >= 6);
    report(failures);
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[test]
fn photo_normalizing_matches_python() {
    let mut failures = Vec::new();
    let mut count = 0;
    for entry in std::fs::read_dir(fixtures().join("photos")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.ends_with(".json") || name.ends_with(".norm.png") || name.ends_with(".normimg.png")
        {
            continue;
        }
        count += 1;
        let raw = std::fs::read(&path).unwrap();
        // JPEG·손실 WebP는 디코더(libjpeg-turbo·libwebp ↔ zune-jpeg·image-webp)가 달라 화소가 조금 다름(알려진 차이) : 크기만 확인
        let pixels_must_match = !(name.ends_with(".jpg") || name.contains("lossy"));
        for (suffix, function) in [
            (
                "norm",
                engine::normalize_photo as fn(&[u8]) -> Result<Vec<u8>, EngineError>,
            ),
            ("normimg", engine::normalize_item_image),
        ] {
            let mut check = Check {
                failures: Vec::new(),
                label: format!("{name} {suffix}"),
            };
            let want =
                json(&path.with_file_name(format!("{name}.{suffix}.json"))).expect("사진 정답");
            match function(&raw) {
                Err(error) => check.fail("정규화", format!("오류 {error}")),
                Ok(bytes) => {
                    let rgb = pil::to_rgb(&pil::open(&bytes).unwrap().image);
                    check.same("너비", rgb.width as u64, want["width"].as_u64().unwrap());
                    check.same("높이", rgb.height as u64, want["height"].as_u64().unwrap());
                    if pixels_must_match {
                        check.same(
                            "화소 CRC32",
                            crc32(&rgb.data) as u64,
                            want["crc32"].as_u64().unwrap(),
                        );
                    }
                }
            }
            failures.extend(check.failures);
        }
    }
    assert!(count >= 12);
    report(failures);
}
