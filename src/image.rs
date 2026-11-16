//! `app/core/image.py` : 업로드 제약(REQ-NFR-003) 캐릭터 사진(FN-MYP-010-08)

use crate::error::ApiError;
use crate::response::AppError;

pub const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// 파일 시그니처 기준 jpg·png·webp 판별
pub fn detect_image_type(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\xff\xd8\xff") {
        return Some("jpeg");
    }
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return Some("webp");
    }
    None
}

fn extension_type(extension: &str) -> Option<&'static str> {
    match extension.to_lowercase().as_str() {
        "jpg" | "jpeg" => Some("jpeg"),
        "png" => Some("png"),
        "webp" => Some("webp"),
        _ => None,
    }
}

/// 확장자 + 실제 형식 검증, 10MB 이하 (PDF 불가)
pub fn validate_upload(image: &[u8], file_name: Option<&str>) -> Result<(), ApiError> {
    if image.len() > MAX_IMAGE_BYTES {
        return Err(AppError::new(
            "FILE_SIZE_EXCEEDED",
            400,
            "파일 용량은 10MB 이하여야 합니다.",
        )
        .into());
    }
    // Python str.rpartition(".") : 점이 없으면 확장자 없음
    let expected = file_name
        .unwrap_or("")
        .rsplit_once('.')
        .and_then(|(_, extension)| extension_type(extension));
    if expected.is_none() || detect_image_type(image) != expected {
        return Err(AppError::new(
            "FILE_TYPE_NOT_ALLOWED",
            400,
            "jpg·jpeg·png·webp 이미지만 올릴 수 있습니다.",
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";

    fn code(error: ApiError) -> String {
        match error {
            ApiError::App(e) => e.code,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn accepts_matching_extension_and_signature() {
        assert!(validate_upload(PNG, Some("a.PNG")).is_ok());
        assert!(validate_upload(b"\xff\xd8\xffx", Some("a.jpg")).is_ok());
        assert!(validate_upload(b"RIFF\0\0\0\0WEBPx", Some("x.y.webp")).is_ok());
    }

    #[test]
    fn rejects_mismatch_missing_extension_and_size() {
        assert_eq!(
            validate_upload(PNG, Some("a.jpg"))
                .map_err(code)
                .unwrap_err(),
            "FILE_TYPE_NOT_ALLOWED"
        );
        assert_eq!(
            validate_upload(PNG, Some("png")).map_err(code).unwrap_err(),
            "FILE_TYPE_NOT_ALLOWED"
        );
        assert_eq!(
            validate_upload(PNG, None).map_err(code).unwrap_err(),
            "FILE_TYPE_NOT_ALLOWED"
        );
        let big = vec![0u8; MAX_IMAGE_BYTES + 1];
        assert_eq!(
            validate_upload(&big, Some("a.png"))
                .map_err(code)
                .unwrap_err(),
            "FILE_SIZE_EXCEEDED"
        );
    }
}
