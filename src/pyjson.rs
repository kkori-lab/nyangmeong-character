//! Python `json.loads`·`json.dumps(ensure_ascii=False)`와 같은 결과를 내는 도우미
//! `app/core/errors.py` `_hide_internal_path`가 Java 오류 본문을 다시 쓸 때 쓴다

use serde::Serialize;
use serde_json::Value;
use std::io;

/// `json.loads(bytes)` : UTF-8(BOM 허용)만 받는다. 실패면 None
pub fn loads(body: &[u8]) -> Option<Value> {
    let body = body.strip_prefix(b"\xef\xbb\xbf").unwrap_or(body);
    let text = std::str::from_utf8(body).ok()?;
    serde_json::from_str(text).ok()
}

/// `json.dumps(value, ensure_ascii=False)` : 구분자 `", "`·`": "`, 실수는 `repr` 모양
pub fn dumps(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, PythonFormatter);
    value.serialize(&mut ser).expect("Vec 쓰기는 실패하지 않음");
    out
}

struct PythonFormatter;

impl serde_json::ser::Formatter for PythonFormatter {
    fn begin_array_value<W: ?Sized + io::Write>(
        &mut self,
        w: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if first { Ok(()) } else { w.write_all(b", ") }
    }

    fn begin_object_key<W: ?Sized + io::Write>(
        &mut self,
        w: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if first { Ok(()) } else { w.write_all(b", ") }
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        w.write_all(b": ")
    }

    fn write_f64<W: ?Sized + io::Write>(&mut self, w: &mut W, value: f64) -> io::Result<()> {
        w.write_all(float_repr(value).as_bytes())
    }

    fn write_f32<W: ?Sized + io::Write>(&mut self, w: &mut W, value: f32) -> io::Result<()> {
        self.write_f64(w, f64::from(value))
    }
}

/// Python `repr(float)` : 가장 짧은 자릿수, 지수 -4 이상 16 미만은 소수점 표기
pub fn float_repr(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    let sci = format!("{value:e}");
    let (mantissa, exponent) = sci.split_once('e').expect("{:e}에는 e가 있음");
    let exponent: i32 = exponent.parse().expect("지수는 정수");
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let sign = if negative { "-" } else { "" };
    if (-4..16).contains(&exponent) {
        let point = exponent + 1;
        let body = if point <= 0 {
            format!("0.{}{digits}", "0".repeat((-point) as usize))
        } else if point as usize >= digits.len() {
            format!("{digits}{}.0", "0".repeat(point as usize - digits.len()))
        } else {
            let (int, frac) = digits.split_at(point as usize);
            format!("{int}.{frac}")
        };
        format!("{sign}{body}")
    } else {
        let (first, rest) = digits.split_at(1);
        let rest = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        let exp_sign = if exponent < 0 { '-' } else { '+' };
        format!("{sign}{first}{rest}e{exp_sign}{:02}", exponent.abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_repr_matches_python() {
        let cases = [
            (1.0, "1.0"),
            (1.5, "1.5"),
            (-0.0, "-0.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1e16, "1e+16"),
            (1.5e16, "1.5e+16"),
            (123456789012345.6, "123456789012345.6"),
            (0.1, "0.1"),
            (1e22, "1e+22"),
            (2.5e-7, "2.5e-07"),
        ];
        for (value, expected) in cases {
            assert_eq!(float_repr(value), expected, "{value}");
        }
    }

    #[test]
    fn dumps_uses_python_separators_and_keeps_order() {
        let value = loads(r#"{"b":1,"a":[1,2.0,"\uac00\n"],"path":null}"#.as_bytes()).unwrap();
        assert_eq!(
            String::from_utf8(dumps(&value)).unwrap(),
            "{\"b\": 1, \"a\": [1, 2.0, \"가\\n\"], \"path\": null}"
        );
    }

    #[test]
    fn loads_strips_bom_and_rejects_bad_utf8() {
        assert!(loads(b"\xef\xbb\xbf{}").is_some());
        assert!(loads(b"{\"a\":\"\xff\"}").is_none());
        assert!(loads(b"{} x").is_none());
    }
}
