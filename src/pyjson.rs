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

/// `int()` 자릿수 제한(Python 3.11+ `sys.int_info.default_max_str_digits`) : 넘으면 `json.loads`가 `ValueError`
const MAX_INT_DIGITS: usize = 4300;

/// `json.loads(str)` 중 Python만 받는 것도 받는 읽기 : `NaN`·`Infinity`·`-Infinity` 상수,
/// f64를 넘치는 숫자(`1e400`은 `inf`), 4300자리까지의 정수. 4300자리를 넘는 정수는 Python처럼 거절(None).
/// 상수·넘치는 숫자는 `null`로, 실수는 f64로 다시 써서 읽는다 : 숫자 값은 쓰지 않고 문자열 칸만 보는 곳에서만 쓴다
pub fn loads_str_lenient(text: &str) -> Option<Value> {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
        } else if let Some(constant) = ["-Infinity", "Infinity", "NaN"]
            .into_iter()
            .find(|constant| rest.starts_with(constant))
        {
            out.push_str("null");
            rest = &rest[constant.len()..];
            continue;
        } else if c == '-' || c.is_ascii_digit() {
            let len = number_len(rest);
            if len > 0 {
                push_number(&mut out, &rest[..len])?;
                rest = &rest[len..];
                continue;
            }
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    serde_json::from_str(&out).ok()
}

/// `json.decoder.NUMBER_RE` `(-?(?:0|[1-9]\d*))(\.\d+)?([eE][-+]?\d+)?`가 맨 앞에서 맞는 바이트 수 (맞지 않으면 0)
fn number_len(text: &str) -> usize {
    let bytes = text.as_bytes();
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut end = usize::from(bytes[0] == b'-');
    match digits(end) {
        0 => return 0,
        _ if bytes[end] == b'0' => end += 1,
        n => end += n,
    }
    if bytes.get(end) == Some(&b'.') && digits(end + 1) > 0 {
        end += 1 + digits(end + 1);
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        if digits(end + 1 + sign) > 0 {
            end += 1 + sign + digits(end + 1 + sign);
        }
    }
    end
}

/// 숫자 토큰 하나를 serde가 받는 모양으로 쓴다. 정수가 4300자리를 넘으면 None
fn push_number(out: &mut String, token: &str) -> Option<()> {
    let is_float = token.contains(['.', 'e', 'E']);
    let digits = token.bytes().filter(u8::is_ascii_digit).count();
    if !is_float {
        if digits > MAX_INT_DIGITS {
            return None;
        }
        if digits <= 18 {
            out.push_str(token);
            return Some(());
        }
    }
    // serde는 f64를 넘치는 수와 아주 큰 지수를 거절하지만 Python은 `inf`·0으로 읽는다
    match token.parse::<f64>() {
        Ok(value) if value.is_finite() => out.push_str(&format!("{value:?}")),
        _ => out.push_str("null"),
    }
    Some(())
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
