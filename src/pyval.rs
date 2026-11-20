//! pydantic(2.14, lax)·Python 내장 `int()`의 값 변환을 그대로 옮김
//! 규칙은 `nyangmeong-ai` 가상환경에서 직접 확인한 결과(2026-11-16)
//!
//! 알려진 차이: Python 정수는 크기 제한이 없지만 여기는 i64까지
//! i64 밖 값은 Python이면 Java로 넘어가 Java가 4xx로 막고, 여기서는 검증 오류(400)

use serde_json::Value;

/// pydantic `int` 필드(lax) : 정수, 소수부 0인 실수, bool, 정수 모양 문자열
pub fn pydantic_int(value: &Value) -> Option<i64> {
    match value {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i)
            } else if n.is_u64() {
                None
            } else {
                float_to_int(n.as_f64()?)
            }
        }
        Value::String(s) => pydantic_int_str(s),
        _ => None,
    }
}

fn float_to_int(f: f64) -> Option<i64> {
    if f.is_finite() && f.fract() == 0.0 && f >= i64::MIN as f64 && f < i64::MAX as f64 {
        Some(f as i64)
    } else {
        None
    }
}

/// pydantic 문자열 → 정수 : 앞뒤 공백, 부호, 숫자 사이 밑줄 하나, 끝의 `.0…`(0만) 허용
/// `"1.0"`·`"1_0.0"`·`" +1 "` 허용, `"1."`·`"1.5"`·`"1e3"`·`"1__0"`·`"0x10"` 거절
pub fn pydantic_int_str(raw: &str) -> Option<i64> {
    let text = raw.trim();
    let (int_part, frac) = match text.split_once('.') {
        Some((int_part, frac)) => (int_part, Some(frac)),
        None => (text, None),
    };
    if let Some(frac) = frac
        && (frac.is_empty() || !frac.bytes().all(|b| b == b'0'))
    {
        return None;
    }
    parse_int_literal(int_part)
}

/// Python 정수 리터럴 모양(부호, 숫자 사이 밑줄 하나) : ASCII 숫자만
fn parse_int_literal(text: &str) -> Option<i64> {
    let (negative, digits) = match text.as_bytes().first()? {
        b'+' => (false, &text[1..]),
        b'-' => (true, &text[1..]),
        _ => (false, text),
    };
    let bytes = digits.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_digit() || !bytes[bytes.len() - 1].is_ascii_digit() {
        return None;
    }
    let mut clean = String::with_capacity(bytes.len() + 1);
    if negative {
        clean.push('-');
    }
    let mut previous_underscore = false;
    for &b in bytes {
        match b {
            b'0'..=b'9' => {
                clean.push(b as char);
                previous_underscore = false;
            }
            b'_' if !previous_underscore => previous_underscore = true,
            _ => return None,
        }
    }
    clean.parse().ok()
}

/// Python 내장 `int(x)` (`app/core/auth.py`의 `int(data["mbrSn"])`)
/// 실수는 버림, 문자열은 공백·부호·밑줄만 허용(소수점 불가)
pub fn builtin_int(value: &Value) -> Option<i64> {
    match value {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i)
            } else if n.is_u64() {
                None
            } else {
                let f = n.as_f64()?.trunc();
                (f.is_finite() && f >= i64::MIN as f64 && f < i64::MAX as f64).then_some(f as i64)
            }
        }
        Value::String(s) => parse_int_literal(s.trim()),
        _ => None,
    }
}

/// pydantic `str` 필드 : 문자열만(숫자를 문자열로 바꾸지 않음)
pub fn pydantic_str(value: &Value) -> Option<&str> {
    value.as_str()
}

/// pydantic `float` 필드(lax, 2026-11-16 원본 가상환경으로 확인) : 숫자, bool, 실수 모양 문자열
/// 문자열은 앞뒤 공백·부호·숫자 사이 밑줄 하나·`inf`·`infinity`·`nan`(대소문자 무시) 허용,
/// `""`·`"1,5"`·`"0x10"` 거절
pub fn pydantic_float(value: &Value) -> Option<f64> {
    match value {
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        Value::Number(n) => n.as_f64(),
        Value::String(s) => float_str(s),
        _ => None,
    }
}

fn float_str(raw: &str) -> Option<f64> {
    let text = raw.trim();
    if !text.is_ascii() {
        return None;
    }
    let bytes = text.as_bytes();
    let mut clean = String::with_capacity(text.len());
    for (at, &b) in bytes.iter().enumerate() {
        if b == b'_' {
            let between_digits = at > 0
                && bytes[at - 1].is_ascii_digit()
                && bytes.get(at + 1).is_some_and(u8::is_ascii_digit);
            if !between_digits {
                return None;
            }
        } else {
            clean.push(b as char);
        }
    }
    clean.parse().ok()
}

/// pydantic `bool` 필드(lax) : bool, 0·1(정수·실수), `true/false/yes/no/on/off/t/f/y/n/1/0`(대소문자 무시, 공백 불가)
pub fn pydantic_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match n.as_f64()? {
            0.0 => Some(false),
            1.0 => Some(true),
            _ => None,
        },
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "t" | "y" | "1" => Some(true),
            "false" | "no" | "off" | "f" | "n" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Python `len(str)` : 코드 포인트 수
pub fn py_len(text: &str) -> usize {
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pydantic_int_str_matches_probe() {
        let ok = [
            ("1", 1),
            (" 1 ", 1),
            ("+1", 1),
            ("-1", -1),
            ("1.0", 1),
            ("1.00", 1),
            ("1_000", 1000),
            ("01", 1),
            ("007", 7),
            ("-0", 0),
            ("1_0.0", 10),
            (" +1 ", 1),
            ("\t1\n", 1),
            ("\u{a0}1", 1),
            ("\u{3000}1", 1),
        ];
        for (text, expected) in ok {
            assert_eq!(pydantic_int_str(text), Some(expected), "{text:?}");
        }
        let bad = [
            "1.5", "1.", ".0", "1__0", "_1", "1_", "1.0e0", "1.10", "1 0", "- 1", "1.0_0", "+-1",
            "", "abc", "0x10", "1e3", "\u{661}", "\u{ff11}",
        ];
        for text in bad {
            assert_eq!(pydantic_int_str(text), None, "{text:?}");
        }
    }

    #[test]
    fn pydantic_int_value_matches_probe() {
        assert_eq!(pydantic_int(&json!(1)), Some(1));
        assert_eq!(pydantic_int(&json!(1.0)), Some(1));
        assert_eq!(pydantic_int(&json!(1.5)), None);
        assert_eq!(pydantic_int(&json!(true)), Some(1));
        assert_eq!(pydantic_int(&json!(false)), Some(0));
        assert_eq!(pydantic_int(&json!(null)), None);
        assert_eq!(pydantic_int(&json!([1])), None);
        assert_eq!(pydantic_int(&json!("2")), Some(2));
    }

    #[test]
    fn pydantic_float_and_bool_match_probe() {
        let floats = [
            (json!(1), 1.0),
            (json!(1.5), 1.5),
            (json!(true), 1.0),
            (json!(false), 0.0),
            (json!("1.5"), 1.5),
            (json!(" 1.5 "), 1.5),
            (json!("1_0"), 10.0),
            (json!("+1"), 1.0),
            (json!("1e3"), 1000.0),
            (json!(".5"), 0.5),
            (json!("5."), 5.0),
            (json!("inf"), f64::INFINITY),
            (json!("-inf"), f64::NEG_INFINITY),
            (json!("infinity"), f64::INFINITY),
        ];
        for (value, expected) in floats {
            assert_eq!(pydantic_float(&value), Some(expected), "{value}");
        }
        assert!(pydantic_float(&json!("NaN")).is_some_and(f64::is_nan));
        for value in [
            json!(""),
            json!("a"),
            json!(null),
            json!([]),
            json!({}),
            json!("1,5"),
            json!("0x10"),
            json!("\u{ff11}\u{ff12}"),
            json!("1__0"),
            json!("_1"),
        ] {
            assert_eq!(pydantic_float(&value), None, "{value}");
        }
        let yes = ["true", "TRUE", "yes", "on", "t", "y", "1"];
        let no = ["false", "no", "off", "f", "n", "0"];
        for text in yes {
            assert_eq!(pydantic_bool(&json!(text)), Some(true), "{text}");
        }
        for text in no {
            assert_eq!(pydantic_bool(&json!(text)), Some(false), "{text}");
        }
        for value in [json!(true), json!(1), json!(1.0)] {
            assert_eq!(pydantic_bool(&value), Some(true), "{value}");
        }
        for value in [json!(false), json!(0), json!(0.0)] {
            assert_eq!(pydantic_bool(&value), Some(false), "{value}");
        }
        for value in [
            json!(2),
            json!(-1),
            json!(0.5),
            json!("2"),
            json!(""),
            json!(null),
            json!(" true "),
            json!("tru"),
        ] {
            assert_eq!(pydantic_bool(&value), None, "{value}");
        }
    }

    #[test]
    fn builtin_int_truncates_float_and_rejects_decimal_string() {
        assert_eq!(builtin_int(&json!(12.9)), Some(12));
        assert_eq!(builtin_int(&json!(" 12 ")), Some(12));
        assert_eq!(builtin_int(&json!("1_2")), Some(12));
        assert_eq!(builtin_int(&json!("12.0")), None);
        assert_eq!(builtin_int(&json!(true)), Some(1));
        assert_eq!(builtin_int(&json!(null)), None);
    }
}
