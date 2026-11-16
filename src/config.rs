//! `app/core/config.py` Settings : 환경변수(대소문자 무시)에서 읽는다
//! Python은 `.env.local`도 읽지만 2번 서버는 systemd `EnvironmentFile`로 넣는다

#[derive(Clone, Debug)]
pub struct Settings {
    /// 러스트→Java 내부 API 주소(인증 확인·작업 보고) : 빈 값이면 /ai/** 전부 502
    pub backend_base_url: String,
    /// 이미지 생성(OpenAI) : 비면 캐릭터 생성 API 502
    pub openai_api_key: String,
    pub character_image_model: String,
    pub character_vision_model: String,
    /// FN-MYP-010-09 : 외부 호출 한 번의 제한 시간(초)
    pub character_timeout_sec: f64,
}

impl Settings {
    pub fn from_env() -> Self {
        let vars: Vec<(String, String)> = std::env::vars().collect();
        Self::from_lookup(|name| {
            vars.iter()
                .rev()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.clone())
        })
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let float = |name: &str, default: f64| match get(name) {
            None => default,
            Some(raw) => raw
                .trim()
                .parse()
                .unwrap_or_else(|_| panic!("환경변수 {name}가 숫자가 아님")),
        };
        Self {
            backend_base_url: get("BACKEND_BASE_URL").unwrap_or_default(),
            openai_api_key: get("OPENAI_API_KEY").unwrap_or_default(),
            character_image_model: get("CHARACTER_IMAGE_MODEL")
                .unwrap_or_else(|| "gpt-image-2".into()),
            character_vision_model: get("CHARACTER_VISION_MODEL")
                .unwrap_or_else(|| "gpt-5-nano".into()),
            character_timeout_sec: float("CHARACTER_TIMEOUT_SEC", 180.0),
        }
    }
}
