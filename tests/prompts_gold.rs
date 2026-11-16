//! 프롬프트·눈 판독이 원본 Python(`engine.py`)과 문자열까지 같은지 비교한다.
//! 정답: `sh scripts/gold.sh prompts`가 만든 `tests/fixtures/prompts.json`

use nyangmeong_character::engine::{self, BakeItem, Eyes};
use serde_json::Value;

fn gold() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/prompts.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("정답 파일")).expect("정답 JSON")
}

fn text(value: &Value) -> Option<&str> {
    value.as_str()
}

fn opt(value: &Value) -> Option<&str> {
    value.as_str()
}

#[test]
fn build_prompt_matches_python() {
    let gold = gold();
    let cases = gold["build_prompt"].as_array().unwrap();
    assert_eq!(cases.len(), 2 * 4 * 8 * 6);
    for case in cases {
        let eyes = Eyes {
            shape: case["shape"].as_str().unwrap().into(),
            color: opt(&case["color"]).map(Into::into),
        };
        let got = engine::build_prompt(
            case["species"].as_str().unwrap(),
            &eyes,
            opt(&case["vrty"]),
            opt(&case["colr"]),
            opt(&case["chrc"]),
        );
        assert_eq!(got, case["prompt"].as_str().unwrap(), "{case}");
    }
}

#[test]
fn eye_question_matches_python() {
    let gold = gold();
    for species in ["dog", "cat"] {
        assert_eq!(
            engine::eye_question(species),
            gold["eye_question"][species].as_str().unwrap()
        );
    }
}

#[test]
fn build_wear_prompt_matches_python() {
    let gold = gold();
    let cases = gold["build_wear_prompt"].as_array().unwrap();
    assert!(!cases.is_empty());
    for case in cases {
        let regn = case["regn"].as_str().unwrap();
        let item = BakeItem {
            item_sn: 1,
            regn_cd: regn.into(),
            bake_hint: opt(&case["hint"]).map(Into::into),
            by_image: case["by_image"].as_bool().unwrap(),
        };
        let got = engine::build_wear_prompt(
            case["species"].as_str().unwrap(),
            &item,
            case["by_image"].as_bool().unwrap(),
        );
        assert_eq!(got, case["prompt"].as_str().unwrap(), "{case}");
    }
}

#[test]
fn parse_eyes_matches_python() {
    let gold = gold();
    for case in gold["parse_eyes"].as_array().unwrap() {
        let got = engine::parse_eyes(case["answer"].as_str().unwrap());
        assert_eq!(got.shape, case["shape"].as_str().unwrap(), "{case}");
        assert_eq!(got.color.as_deref(), opt(&case["color"]), "{case}");
    }
}

#[test]
fn parse_bake_hint_matches_python() {
    let gold = gold();
    for case in gold["parse_bake_hint"].as_array().unwrap() {
        let got = engine::parse_bake_hint(case["answer"].as_str().unwrap());
        assert_eq!(got.as_deref(), opt(&case["hint"]), "{case}");
    }
}

#[test]
fn item_texts_match_python() {
    let gold = gold();
    for case in gold["item_texts"].as_array().unwrap() {
        let regn = case["regn"].as_str().unwrap();
        let name = case["name"].as_str().unwrap();
        assert_eq!(
            engine::item_hint_question(regn, name),
            text(&case["hint"]).unwrap()
        );
        assert_eq!(
            engine::item_pixel_prompt(regn, name),
            text(&case["pixel"]).unwrap()
        );
    }
}

#[test]
fn species_and_style_ref_match_python() {
    let gold = gold();
    for code in ["PETC0001", "PETC0002"] {
        assert_eq!(engine::species_of(code), gold["species"][code].as_str());
    }
    assert_eq!(engine::species_of("PETC0003"), None);
    assert_eq!(
        engine::STYLE_REF.len() as u64,
        gold["style_ref_bytes"].as_u64().unwrap()
    );
}
