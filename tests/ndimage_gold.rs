//! scipy.ndimage 정답 비교 : `contract/gold_images.py`가 원본 가상환경(scipy 1.18.1)에서 만든 `tests/fixtures/image/ndimage.json`

use std::path::PathBuf;

use nyangmeong_character::ndimage::{self, Structure};
use serde_json::Value;

fn rows(value: &Value) -> (Vec<bool>, usize, usize) {
    let lines: Vec<&str> = value
        .as_array()
        .expect("행 목록")
        .iter()
        .map(|row| row.as_str().expect("행 문자열"))
        .collect();
    let width = lines[0].len();
    let cells = lines
        .iter()
        .flat_map(|line| line.bytes().map(|c| c == b'1'))
        .collect();
    (cells, width, lines.len())
}

#[test]
fn matches_scipy_on_random_masks() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/image/ndimage.json");
    let cases: Vec<Value> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!cases.is_empty());
    for (index, case) in cases.iter().enumerate() {
        let (mask, w, h) = rows(&case["mask"]);
        let (allowed, _, _) = rows(&case["allowed"]);
        let (seeds, _, _) = rows(&case["seeds"]);
        let expect = |name: &str| rows(&case[name]).0;
        let at = format!("사례 {index} ({w}×{h})");

        let (labels, count) = ndimage::label(&mask, w, h);
        let want_labels: Vec<u32> = case["labels"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|row| {
                row.as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u32)
            })
            .collect();
        assert_eq!(
            count as u64,
            case["count"].as_u64().unwrap(),
            "{at} label 개수"
        );
        assert_eq!(labels, want_labels, "{at} label 번호");

        let checks: Vec<(&str, Vec<bool>)> = vec![
            ("fill_holes", ndimage::binary_fill_holes(&mask, w, h)),
            (
                "erosion_cross_1",
                ndimage::binary_erosion(&mask, w, h, Structure::Cross, 1),
            ),
            (
                "erosion_square_2",
                ndimage::binary_erosion(&mask, w, h, Structure::Square, 2),
            ),
            (
                "dilation_cross_1",
                ndimage::binary_dilation(&mask, w, h, Structure::Cross, 1),
            ),
            (
                "dilation_cross_3",
                ndimage::binary_dilation(&mask, w, h, Structure::Cross, 3),
            ),
            (
                "dilation_square_2",
                ndimage::binary_dilation(&mask, w, h, Structure::Square, 2),
            ),
            (
                "opening_cross_2",
                ndimage::binary_opening(&mask, w, h, Structure::Cross, 2),
            ),
            (
                "opening_square_1",
                ndimage::binary_opening(&mask, w, h, Structure::Square, 1),
            ),
            (
                "propagation",
                ndimage::binary_propagation(&seeds, &allowed, w, h),
            ),
        ];
        for (name, got) in checks {
            assert_eq!(got, expect(name), "{at} {name}");
        }
    }
}
