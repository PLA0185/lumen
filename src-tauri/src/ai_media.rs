//! Provider-native image/document inputs. No file is fetched or sent implicitly.
use crate::{
    ai::{ChatMessage, ChatRequest, Provider},
    content_assets::{decode_asset, get_asset, ContentAsset, MAX_ASSET_BYTES},
    db::Db,
    error::{AppError, AppResult},
};
use serde_json::{json, Value};

// Local request guard; DeepSeek's documented image-count ceiling is 600.
const MAX_MEDIA_FILES: usize = 600;

pub fn validate_material_text(text: &str) -> AppResult<()> {
    if text.chars().count() > 80_000 {
        return Err(AppError::validation(
            "原始材料（含图片引用）最多 80000 字，请分段整理",
        ));
    }
    let references = regex::Regex::new(r"!?\[[^\]\n]*\]\(lumen-asset:[0-9a-fA-F-]{36}\)")
        .expect("constant asset token regex");
    if references.replace_all(text, "").chars().count() > 20_000 {
        return Err(AppError::validation("材料文字最多 20000 字"));
    }
    Ok(())
}

pub async fn load_media(
    db: &Db,
    provider: Provider,
    ids: &[String],
) -> AppResult<Vec<ContentAsset>> {
    if ids.len() > MAX_MEDIA_FILES {
        return Err(AppError::validation("一次最多分析 600 个文件"));
    }
    let mut assets = Vec::new();
    let mut total_bytes = 0;
    for id in ids {
        if assets.iter().any(|a: &ContentAsset| &a.id == id) {
            continue;
        }
        let asset = get_asset(db, id).await?;
        total_bytes += asset.byte_size.max(0) as u64;
        if total_bytes > MAX_ASSET_BYTES as u64 {
            return Err(AppError::validation(
                "AI 材料总大小最多 20 MiB，请减少文件或缩小图片",
            ));
        }
        assets.push(asset);
    }
    validate_media(provider, &assets)?;
    Ok(assets)
}

pub fn validate_media(provider: Provider, media: &[ContentAsset]) -> AppResult<()> {
    if media.len() > MAX_MEDIA_FILES {
        return Err(AppError::validation("一次最多分析 600 个文件"));
    }
    if media.iter().map(|a| a.byte_size.max(0) as u64).sum::<u64>() > MAX_ASSET_BYTES as u64 {
        return Err(AppError::validation(
            "AI 材料总大小最多 20 MiB，请减少文件或缩小图片",
        ));
    }
    let mut text_chars = 0;
    for asset in media {
        let bytes = decode_asset(asset)?;
        if asset.mime == "text/plain" {
            let text = std::str::from_utf8(&bytes).map_err(|_| {
                AppError::validation(format!("{} 不是 UTF-8 文本，请转换编码后重试", asset.name))
            })?;
            text_chars += text.chars().count();
            if text_chars > 100_000 {
                return Err(AppError::validation(
                    "文件文字合计超过 100000 字，请分段分析",
                ));
            }
            continue;
        }
        let image = asset.mime.starts_with("image/");
        let pdf = asset.mime == "application/pdf";
        let office = matches!(
            asset.mime.as_str(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
                | "application/msword"
                | "application/vnd.ms-excel"
                | "application/rtf"
                | "application/vnd.oasis.opendocument.text"
        );
        let supported = match provider {
            Provider::OpenAI => image || pdf || office,
            Provider::Claude | Provider::Custom => image || pdf,
            Provider::DeepSeek => image,
        };
        if !supported {
            return Err(AppError::validation(format!("{} 当前接口不能分析 {}（{}）", provider.label(), asset.name, asset.mime)).with_hint("可改用支持该格式的 OpenAI 模型；文档也可以转换为 PDF 或 UTF-8 文本。文件仍保存在本机。"));
        }
        if provider == Provider::Claude && image && asset.data_base64.len() > 10_000_000 {
            return Err(AppError::validation(format!(
                "{} 编码后超过 Claude 单图片 10 MB 上限，请缩小图片",
                asset.name
            )));
        }
    }
    Ok(())
}

pub fn message_content(message: &ChatMessage, req: &ChatRequest) -> Value {
    if message.role != "user" || req.media.is_empty() {
        return json!(message.content);
    }
    let provider = req.config.provider;
    let mut parts = vec![if provider == Provider::OpenAI {
        json!({"type":"input_text", "text":message.content})
    } else {
        json!({"type":"text", "text":message.content})
    }];
    for asset in &req.media {
        if asset.mime == "text/plain" {
            // chat() validated encoding before building this request.
            let bytes = decode_asset(asset).expect("validated media");
            let text = std::str::from_utf8(&bytes).expect("validated UTF-8");
            parts.push(json!({"type": if provider == Provider::OpenAI {"input_text"} else {"text"}, "text":format!("文件：{}\n{}",asset.name,text)}));
        } else if asset.mime.starts_with("image/") {
            parts.push(match provider {
                Provider::OpenAI => json!({"type":"input_image", "image_url":format!("data:{};base64,{}",asset.mime,asset.data_base64)}),
                Provider::Claude => json!({"type":"image", "source":{"type":"base64","media_type":asset.mime,"data":asset.data_base64}}),
                _ => json!({"type":"image_url", "image_url":{"url":format!("data:{};base64,{}",asset.mime,asset.data_base64)}}),
            });
        } else {
            parts.push(match provider {
                Provider::OpenAI => json!({"type":"input_file", "filename":asset.name, "file_data":format!("data:{};base64,{}",asset.mime,asset.data_base64)}),
                Provider::Claude => json!({"type":"document", "title":asset.name,"source":{"type":"base64","media_type":asset.mime,"data":asset.data_base64}}),
                _ => json!({"type":"file", "file":{"filename":asset.name,"file_data":format!("data:{};base64,{}",asset.mime,asset.data_base64)}}),
            });
        }
    }
    json!(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_text_budget_excludes_local_resource_tokens_but_bounds_raw_input() {
        let token = "![业务截图.png](lumen-asset:00000000-0000-7000-8000-000000000001)";
        let material = format!("{}{}", "文".repeat(20_000), token.repeat(600));
        validate_material_text(&material).unwrap();
        assert!(validate_material_text(&"文".repeat(20_001)).is_err());
        assert!(validate_material_text(&token.repeat(2000)).is_err());
        assert!(validate_material_text(&"![外部](https://example.com)".repeat(1000)).is_err());
    }
    use base64::{engine::general_purpose::STANDARD, Engine};
    use sha2::{Digest, Sha256};
    fn asset(name: &str, bytes: &[u8]) -> ContentAsset {
        ContentAsset {
            id: uuid::Uuid::now_v7().to_string(),
            name: name.into(),
            mime: crate::content_assets::mime_for(name, bytes).into(),
            data_base64: STANDARD.encode(bytes),
            byte_size: bytes.len() as i64,
            sha256: hex::encode(Sha256::digest(bytes)),
            created_at: crate::db::now_stamp(),
        }
    }
    #[tokio::test]
    async fn more_than_ten_selected_images_are_loaded_and_transmitted() {
        let dir =
            std::env::temp_dir().join(format!("lumen-ai-many-images-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let bytes = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jB9kAAAAASUVORK5CYII=").unwrap();
        let mut ids = Vec::new();
        for index in 0..15 {
            ids.push(
                crate::content_assets::store_bytes(
                    &db,
                    &format!("chat-{index}.png"),
                    bytes.clone(),
                )
                .await
                .unwrap()
                .id,
            );
        }
        let result = load_media(&db, Provider::DeepSeek, &ids).await;
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
        let media = result.unwrap();
        assert_eq!(
            media
                .iter()
                .map(|asset| asset.id.clone())
                .collect::<Vec<_>>(),
            ids
        );
        let message = ChatMessage {
            role: "user".into(),
            content: "核对全部聊天截图".into(),
        };
        let req = ChatRequest {
            config: crate::ai::ProviderConfig::with_defaults(Provider::DeepSeek),
            system: None,
            messages: vec![message.clone()],
            json_output: false,
            max_output_tokens: None,
            media,
        };
        let parts = message_content(&message, &req);
        assert_eq!(parts.as_array().unwrap().len(), 16);
        for part in parts.as_array().unwrap().iter().skip(1) {
            assert_eq!(
                part["image_url"]["url"],
                format!("data:image/png;base64,{}", STANDARD.encode(&bytes))
            );
        }
    }
    #[test]
    fn many_images_still_have_count_and_total_byte_limits() {
        let png = asset("image.png", b"\x89PNG\r\n\x1a\nimage-bytes");
        validate_media(Provider::DeepSeek, &vec![png.clone(); 600]).unwrap();
        assert!(validate_media(Provider::DeepSeek, &vec![png.clone(); 601])
            .unwrap_err()
            .message
            .contains("600"));
        let mut bytes = vec![0; MAX_ASSET_BYTES / 2 + 1];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        let oversized = vec![asset("large.png", &bytes); 2];
        assert!(validate_media(Provider::DeepSeek, &oversized)
            .unwrap_err()
            .message
            .contains("20 MiB"));
    }
    #[tokio::test]
    async fn loading_stops_at_total_budget_before_reading_more_assets() {
        let dir =
            std::env::temp_dir().join(format!("lumen-ai-media-budget-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let mut bytes = vec![0; MAX_ASSET_BYTES / 2 + 1];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        let mut ids = Vec::new();
        for _ in 0..2 {
            ids.push(
                crate::content_assets::store_bytes(&db, "large.png", bytes.clone())
                    .await
                    .unwrap()
                    .id,
            );
        }
        ids.push(uuid::Uuid::now_v7().to_string());
        let error = load_media(&db, Provider::DeepSeek, &ids).await.unwrap_err();
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
        assert!(
            error.message.contains("20 MiB"),
            "the budget must fail before looking up later files: {error}"
        );
    }
    #[test]
    fn native_parts_contain_real_bytes_and_provider_constraints_are_enforced() {
        let png = asset("image.png", b"\x89PNG\r\n\x1a\nimage-bytes");
        let pdf = asset("manual.pdf", b"%PDF-1.7 real document bytes");
        let text = asset("readme.md", "注意事项：发货前核对订单".as_bytes());
        let word = asset("manual.docx", b"PK\x03\x04document-content");
        for provider in [Provider::OpenAI, Provider::Claude, Provider::Custom] {
            validate_media(provider, &[png.clone(), pdf.clone(), text.clone()]).unwrap();
            let message = ChatMessage {
                role: "user".into(),
                content: "请分析".into(),
            };
            let req = ChatRequest {
                config: crate::ai::ProviderConfig::with_defaults(provider),
                system: None,
                messages: vec![message.clone()],
                json_output: false,
                max_output_tokens: None,
                media: vec![png.clone(), pdf.clone(), text.clone()],
            };
            let parts = message_content(&message, &req);
            assert_eq!(parts.as_array().unwrap().len(), 4);
            assert!(parts.to_string().contains(&png.data_base64));
            assert!(parts.to_string().contains(&pdf.data_base64));
            assert!(parts.to_string().contains("发货前核对订单"));
            assert_eq!(
                parts[1]["type"],
                if provider == Provider::OpenAI {
                    "input_image"
                } else if provider == Provider::Claude {
                    "image"
                } else {
                    "image_url"
                }
            );
            assert_eq!(
                parts[2]["type"],
                if provider == Provider::OpenAI {
                    "input_file"
                } else if provider == Provider::Claude {
                    "document"
                } else {
                    "file"
                }
            );
        }
        validate_media(Provider::OpenAI, std::slice::from_ref(&word)).unwrap();
        assert!(validate_media(Provider::Claude, std::slice::from_ref(&word)).is_err());
        validate_media(Provider::DeepSeek, &[png]).unwrap();
        assert!(validate_media(Provider::DeepSeek, &[pdf]).is_err());
        validate_media(Provider::DeepSeek, &[text]).unwrap();
        assert!(validate_media(Provider::OpenAI, &[asset("unknown.zip", b"PK\x03\x04")]).is_err());
        assert!(
            validate_media(Provider::DeepSeek, &[asset("invalid.txt", &[0xff, 0xfe])]).is_err()
        );
    }
}
