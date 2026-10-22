use crate::error::{AppError, AppResult};

use super::models::TemplateConfig;

const MATCH_EDGE: u32 = 64;

/// Return the best normalized cross-correlation score in [0, 1]. The search is
/// constrained to a small area around the normalized ROI saved by the user.
pub fn image_score(
    frame_png: &[u8],
    template_png: &[u8],
    config: &TemplateConfig,
) -> AppResult<f32> {
    if frame_png.is_empty() || template_png.is_empty() {
        return Err(AppError::validation("当前画面或参考图为空"));
    }
    let frame = image::load_from_memory(frame_png)
        .map_err(|_| AppError::validation("当前窗口画面无法解码"))?
        .to_luma8();
    let template = image::load_from_memory(template_png)
        .map_err(|_| AppError::validation("参考图无法解码"))?
        .to_luma8();
    if frame.width() == 0 || frame.height() == 0 || template.width() < 8 || template.height() < 8 {
        return Err(AppError::validation("参考图尺寸太小或当前画面无效"));
    }
    if u64::from(frame.width()) * u64::from(frame.height()) > 64_000_000 {
        return Err(AppError::validation("当前窗口画面超过 6400 万像素限制"));
    }

    let expected_x = (config.x * frame.width() as f32).round() as i32;
    let expected_y = (config.y * frame.height() as f32).round() as i32;
    let expected_w = (config.width * frame.width() as f32).round().max(1.0) as u32;
    let expected_h = (config.height * frame.height() as f32).round().max(1.0) as u32;
    let mut best = f32::NEG_INFINITY;

    // A few controlled scales accommodate DPI and zoom changes without
    // searching the whole window. The selected region itself is never widened.
    for scale in [0.90_f32, 0.95, 1.0, 1.05, 1.10] {
        let target_w = ((expected_w as f32 * scale).round() as u32).clamp(8, frame.width());
        let target_h = ((expected_h as f32 * scale).round() as u32).clamp(8, frame.height());
        let (tpl_w, tpl_h) = bounded_size(template.width(), template.height(), target_w, target_h);
        let resized_template = image::imageops::resize(
            &template,
            tpl_w,
            tpl_h,
            image::imageops::FilterType::Triangle,
        );
        let search_x = ((expected_w as f32 * 0.12).round() as i32).max(2);
        let search_y = ((expected_h as f32 * 0.12).round() as i32).max(2);
        let step_x = (tpl_w / 12).max(1) as i32;
        let step_y = (tpl_h / 12).max(1) as i32;
        let min_x = (expected_x - search_x).clamp(0, frame.width() as i32 - tpl_w as i32);
        let min_y = (expected_y - search_y).clamp(0, frame.height() as i32 - tpl_h as i32);
        let max_x = (expected_x + search_x).clamp(min_x, frame.width() as i32 - tpl_w as i32);
        let max_y = (expected_y + search_y).clamp(min_y, frame.height() as i32 - tpl_h as i32);
        let mut y = min_y;
        while y <= max_y {
            let mut x = min_x;
            while x <= max_x {
                best = best.max(ncc(&frame, &resized_template, x as u32, y as u32));
                x += step_x;
            }
            y += step_y;
        }
    }
    if !best.is_finite() {
        return Err(AppError::validation("参考图没有可用于比较的视觉特征"));
    }
    Ok(((best + 1.0) / 2.0).clamp(0.0, 1.0))
}

fn bounded_size(source_w: u32, source_h: u32, target_w: u32, target_h: u32) -> (u32, u32) {
    let scale = (MATCH_EDGE as f32 / source_w.max(source_h) as f32)
        .min(MATCH_EDGE as f32 / target_w.max(target_h) as f32)
        .min(1.0);
    (
        ((target_w as f32 * scale).round() as u32).max(8),
        ((target_h as f32 * scale).round() as u32).max(8),
    )
}

fn ncc(frame: &image::GrayImage, template: &image::GrayImage, x: u32, y: u32) -> f32 {
    let count = (template.width() * template.height()) as f64;
    if count == 0.0 {
        return -1.0;
    }
    let mut frame_sum = 0.0_f64;
    let mut template_sum = 0.0_f64;
    let mut frame_sq = 0.0_f64;
    let mut template_sq = 0.0_f64;
    let mut cross = 0.0_f64;
    for ty in 0..template.height() {
        for tx in 0..template.width() {
            let a = f64::from(frame.get_pixel(x + tx, y + ty)[0]);
            let b = f64::from(template.get_pixel(tx, ty)[0]);
            frame_sum += a;
            template_sum += b;
            frame_sq += a * a;
            template_sq += b * b;
            cross += a * b;
        }
    }
    let numerator = cross - frame_sum * template_sum / count;
    let frame_variance = (frame_sq - frame_sum * frame_sum / count).max(0.0);
    let template_variance = (template_sq - template_sum * template_sum / count).max(0.0);
    let denominator = (frame_variance * template_variance).sqrt();
    if denominator <= f64::EPSILON {
        return -1.0;
    }
    (numerator / denominator).clamp(-1.0, 1.0) as f32
}

pub fn normalize_text(text: &str) -> String {
    text.chars()
        .map(|ch| {
            let code = ch as u32;
            if code == 0x3000 {
                ' '
            } else if (0xFF01..=0xFF5E).contains(&code) {
                char::from_u32(code - 0xFEE0)
                    .unwrap_or(ch)
                    .to_ascii_lowercase()
            } else {
                ch.to_lowercase().next().unwrap_or(ch)
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn ocr_terms_match(text: &str, required: &[String], excluded: &[String]) -> bool {
    let text = normalize_text(text);
    required
        .iter()
        .all(|term| text.contains(&normalize_text(term)))
        && excluded
            .iter()
            .all(|term| !text.contains(&normalize_text(term)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, GrayImage, ImageFormat, Luma};
    use std::io::Cursor;

    fn png(image: &GrayImage) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageLuma8(image.clone())
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    #[test]
    fn matching_finds_nearby_template_and_rejects_different_pattern() {
        let mut frame = GrayImage::from_pixel(240, 160, Luma([16]));
        let mut pattern = GrayImage::new(24, 20);
        for y in 0..20 {
            for x in 0..24 {
                pattern.put_pixel(
                    x,
                    y,
                    Luma([((x * 17 + y * 31 + x * y * 3) % 220 + 20) as u8]),
                );
            }
        }
        image::imageops::replace(&mut frame, &pattern, 124, 84);
        let config = TemplateConfig {
            x: 0.5125,
            y: 0.51875,
            width: 0.10,
            height: 0.125,
            source_width: 240,
            source_height: 160,
            threshold: 0.82,
        };
        let exact = image_score(&png(&frame), &png(&pattern), &config).unwrap();
        assert!(
            exact >= config.threshold,
            "expected a strong match, got {exact}"
        );
        let different = GrayImage::from_fn(24, 20, |x, y| Luma([((x * 9 + y * 5) % 255) as u8]));
        let score = image_score(&png(&frame), &png(&different), &config).unwrap();
        assert!(
            score < config.threshold,
            "negative sample unexpectedly matched: {score}"
        );
    }

    #[test]
    fn ocr_match_normalizes_fullwidth_ascii_and_excluded_terms() {
        assert!(ocr_terms_match(
            "  ＦＮＳＫＵ　 Scan   now ",
            &["FNSKU".into()],
            &[]
        ));
        assert!(!ocr_terms_match(
            "FNSKU scan",
            &["FNSKU".into()],
            &["scan".into()]
        ));
    }
}
