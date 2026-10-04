//! Offline PaddleOCR fallback used when multimodal recognition is unavailable.
use crate::error::{AppError, AppResult};
use std::path::Path;

#[cfg(windows)]
pub(crate) fn recognize(bytes: &[u8], model_dir: &Path) -> AppResult<String> {
    use image::{ImageReader, Limits};
    use paddle_ocr_rs::ocr_lite::OcrLite;
    use std::{
        io::Cursor,
        sync::{Mutex, OnceLock},
    };

    static ENGINE: OnceLock<Result<Mutex<OcrLite>, String>> = OnceLock::new();
    let engine = ENGINE
        .get_or_init(|| {
            let path = |name: &str| model_dir.join(name).to_string_lossy().into_owned();
            let mut engine = OcrLite::new();
            engine
                .init_models(
                    &path("ch_PP-OCRv4_det_infer.onnx"),
                    &path("ch_ppocr_mobile_v2.0_cls_infer.onnx"),
                    &path("ch_PP-OCRv4_rec_infer.onnx"),
                    std::thread::available_parallelism().map_or(2, |count| count.get().min(4)),
                )
                .map_err(|error| format!("PaddleOCR 模型加载失败：{error}"))?;
            Ok(Mutex::new(engine))
        })
        .as_ref()
        .map_err(|error| AppError::internal(error.clone()))?;

    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| AppError::validation(format!("无法读取 OCR 图片：{error}")))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(10_000);
    limits.max_image_height = Some(10_000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|error| AppError::validation(format!("无法解码 OCR 图片：{error}")))?
        .to_rgb8();
    if u64::from(image.width()) * u64::from(image.height()) > 64_000_000 {
        return Err(AppError::validation("OCR 图片超过 6400 万像素限制"));
    }
    let result = engine
        .lock()
        .map_err(|_| AppError::internal("PaddleOCR 引擎状态损坏"))?
        .detect(&image, 50, 960, 0.5, 0.3, 1.6, true, false)
        .map_err(|error| AppError::internal(format!("PaddleOCR 识别失败：{error}")))?;
    let text = result
        .text_blocks
        .iter()
        .map(|block| block.text.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        return Err(AppError::validation(
            "PaddleOCR 未识别到文字；请检查图片清晰度或原文是否包含文字",
        ));
    }
    Ok(text)
}

#[cfg(not(windows))]
pub(crate) fn recognize(_bytes: &[u8], _model_dir: &Path) -> AppResult<String> {
    Err(AppError::validation(
        "本机 PaddleOCR 当前仅随 Windows 版本提供",
    ))
}

pub(crate) fn bundled_model_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/ocr")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    #[test]
    fn model_bundle_contains_fixed_paddle_assets() {
        let dir = bundled_model_dir();
        for (name, expected_sha256) in [
            (
                "ch_PP-OCRv4_det_infer.onnx",
                "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9",
            ),
            (
                "ch_PP-OCRv4_rec_infer.onnx",
                "48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b",
            ),
            (
                "ch_ppocr_mobile_v2.0_cls_infer.onnx",
                "e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c",
            ),
        ] {
            let bytes = std::fs::read(dir.join(name))
                .unwrap_or_else(|error| panic!("missing PaddleOCR asset {name}: {error}"));
            let digest = hex::encode(sha2::Sha256::digest(bytes));
            assert_eq!(digest, expected_sha256, "PaddleOCR asset changed: {name}");
        }
    }
}
