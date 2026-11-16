//! `app/core/config.py` Settings : 환경변수가 `.env.local`보다 우선(키 이름은 대소문자 무시)
//! 2번 서버는 systemd `EnvironmentFile`(시크릿 ENV_FILE)로 넣는다

use std::path::Path;

#[derive(Clone, Debug)]
pub struct Settings {
    /// 러스트→Java 내부 API 주소(인증 확인·작업 보고) : 빈 값이면 /ai/** 전부 502
    pub backend_base_url: String,
    /// 이미지 생성(OpenAI) : 비면 캐릭터 생성 API 502
    pub openai_api_key: String,
    /// OpenAI 주소(없으면 `https://api.openai.com/v1`). Python SDK도 같은 환경변수를 읽는다 : 대조 테스트가 가짜 OpenAI를 가리킨다
    pub openai_base_url: String,
    pub character_image_model: String,
    pub character_vision_model: String,
    /// FN-MYP-010-09 : 외부 호출 한 번의 제한 시간(초)
    pub character_timeout_sec: f64,
}

impl Settings {
    /// 환경변수, 없으면 현재 폴더의 `.env.local`
    pub fn from_env() -> Self {
        let env: Vec<(String, String)> = std::env::vars().collect();
        let file = read_dotenv(Path::new(".env.local"));
        Self::from_lookup(|name| lookup(&env, name).or_else(|| lookup(&file, name)))
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let float = |name: &str, default: f64| match get(name) {
            None => default,
            Some(raw) => raw
                .trim()
                .parse()
                .unwrap_or_else(|_| panic!("설정 {name}가 숫자가 아님")),
        };
        Self {
            backend_base_url: get("BACKEND_BASE_URL").unwrap_or_default(),
            openai_api_key: get("OPENAI_API_KEY").unwrap_or_default(),
            openai_base_url: get("OPENAI_BASE_URL")
                .filter(|url| !url.is_empty())
                .unwrap_or_else(|| "https://api.openai.com/v1".into()),
            character_image_model: get("CHARACTER_IMAGE_MODEL")
                .unwrap_or_else(|| "gpt-image-2".into()),
            character_vision_model: get("CHARACTER_VISION_MODEL")
                .unwrap_or_else(|| "gpt-5-nano".into()),
            character_timeout_sec: float("CHARACTER_TIMEOUT_SEC", 180.0),
        }
    }
}

/// 같은 이름이 여럿이면 마지막 값
fn lookup(pairs: &[(String, String)], name: &str) -> Option<String> {
    pairs
        .iter()
        .rev()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn read_dotenv(path: &Path) -> Vec<(String, String)> {
    std::fs::read_to_string(path)
        .map(|text| parse_dotenv(&text))
        .unwrap_or_default()
}

/// python-dotenv 기본 문법 : `#` 주석 줄, `export ` 앞머리, 따옴표 값, 따옴표 없는 값의 ` #` 뒤 주석
pub fn parse_dotenv(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            if key.is_empty() {
                return None;
            }
            let value = value.trim();
            let value = if let Some(inner) = quoted(value, '"') {
                inner
                    .replace("\\\"", "\"")
                    .replace("\\n", "\n")
                    .replace("\\\\", "\\")
            } else if let Some(inner) = quoted(value, '\'') {
                inner.to_owned()
            } else {
                value.split(" #").next().unwrap_or("").trim_end().to_owned()
            };
            Some((key.to_owned(), value))
        })
        .collect()
}

fn quoted(value: &str, quote: char) -> Option<&str> {
    let inner = value.strip_prefix(quote)?;
    let end = inner.rfind(quote)?;
    Some(&inner[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotenv_rules() {
        let pairs = parse_dotenv(
            "# 주석\nBACKEND_BASE_URL=http://127.0.0.1:8080 # 로컬\nexport OPENAI_API_KEY=\"a b\"\nX='c d'\nEMPTY=\nCHARACTER_TIMEOUT_SEC = 30\n",
        );
        let get = |name: &str| lookup(&pairs, name);
        assert_eq!(
            get("backend_base_url").as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(get("OPENAI_API_KEY").as_deref(), Some("a b"));
        assert_eq!(get("X").as_deref(), Some("c d"));
        assert_eq!(get("EMPTY").as_deref(), Some(""));
        let settings = Settings::from_lookup(get);
        assert_eq!(settings.character_timeout_sec, 30.0);
        assert_eq!(settings.character_image_model, "gpt-image-2");
    }
}
