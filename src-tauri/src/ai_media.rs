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

pub(crate) struct ResourceLink {
    pub id: String,
    pub range: std::ops::Range<usize>,
    pub image: bool,
    pub inline: bool,
    pub label: String,
    pub title: String,
}

/// CommonMark resolves reference definitions while excluding code and HTML
/// examples. Offsets refer to the actual use, never the definition line.
pub(crate) fn resource_links(text: &str) -> Vec<ResourceLink> {
    use pulldown_cmark::{Event, LinkType, Parser, Tag, TagEnd};
    let mut links: Vec<ResourceLink> = Vec::new();
    let mut labels = Vec::new();
    for (event, range) in Parser::new(text).into_offset_iter() {
        match event {
            Event::Start(tag @ (Tag::Link { .. } | Tag::Image { .. })) => {
                let image = matches!(tag, Tag::Image { .. });
                let (dest_url, title, link_type) = match tag {
                    Tag::Link {
                        dest_url,
                        title,
                        link_type,
                        ..
                    }
                    | Tag::Image {
                        dest_url,
                        title,
                        link_type,
                        ..
                    } => (dest_url, title, link_type),
                    _ => unreachable!("link or image"),
                };
                let id = dest_url
                    .split_once(':')
                    .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("lumen-asset"))
                    .map(|(_, id)| {
                        uuid::Uuid::parse_str(id)
                            .map(|id| id.to_string())
                            .unwrap_or_else(|_| id.to_owned())
                    });
                labels.push(id.map(|id| {
                    let index = links.len();
                    links.push(ResourceLink {
                        id,
                        range,
                        image,
                        inline: link_type == LinkType::Inline,
                        label: String::new(),
                        title: title.to_string(),
                    });
                    index
                }));
            }
            Event::Text(value) | Event::Code(value) => {
                for index in labels.iter().flatten() {
                    links[*index].label.push_str(&value);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                for index in labels.iter().flatten() {
                    links[*index].label.push('\n');
                }
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                labels.pop();
            }
            _ => (),
        }
    }
    links
}

pub struct FlowMedia {
    pub media: Vec<ContentAsset>,
    pub references: Vec<ContentAsset>,
    pub warnings: Vec<String>,
    pub derived: Vec<ContentAsset>,
    pub source_documents: Vec<(String, String)>,
    pub archived_documents: Vec<ContentAsset>,
}

/// Explicit flow generation may derive images; importing and generic chat do not.
#[cfg(test)]
pub async fn load_flow_media(db: &Db, provider: Provider, ids: &[String]) -> AppResult<FlowMedia> {
    load_flow_media_for_source(db, provider, ids, "").await
}

pub async fn load_flow_media_for_source(
    db: &Db,
    provider: Provider,
    ids: &[String],
    source: &str,
) -> AppResult<FlowMedia> {
    // Actual Markdown pointers are explicit selections too. Put them in source
    // order, including reference links missed by the frontend's token scanner.
    let mut selected_ids = document_asset_ids(source);
    let raw_source = source.to_ascii_lowercase();
    for id in ids {
        let id = uuid::Uuid::parse_str(id)
            .map(|id| id.to_string())
            .unwrap_or_else(|_| id.to_ascii_lowercase());
        if selected_ids.iter().any(|selected| selected == &id) {
            continue;
        }
        // The scanner also sees code literals. They cannot authorize reading
        // an asset; IDs absent from the text remain valid explicit attachments.
        if raw_source.contains(&format!("lumen-asset:{id}")) {
            continue;
        }
        selected_ids.push(id);
    }
    if selected_ids.len() > MAX_MEDIA_FILES {
        return Err(AppError::validation("一次最多分析 600 个文件"));
    }
    let mut result = FlowMedia {
        media: Vec::new(),
        references: Vec::new(),
        warnings: Vec::new(),
        derived: Vec::new(),
        source_documents: Vec::new(),
        archived_documents: Vec::new(),
    };
    let mut originals = Vec::new();
    let mut original_bytes = 0u64;
    for id in &selected_ids {
        if !originals
            .iter()
            .any(|a: &ContentAsset| a.id.eq_ignore_ascii_case(id))
        {
            let original = get_asset(db, &id.to_ascii_lowercase()).await?;
            original_bytes += original.byte_size.max(0) as u64;
            if original_bytes > MAX_ASSET_BYTES as u64 {
                return Err(AppError::validation(
                    "AI 材料总大小最多 20 MiB，请减少文件或缩小图片",
                ));
            }
            originals.push(original);
        }
    }
    let archived = recognized_document_ids(source, &originals)?;
    let mut queued = originals
        .iter()
        .map(|a| a.id.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut pending = std::collections::VecDeque::from(originals);
    let mut budget = Vec::new();
    while let Some(original) = pending.pop_front() {
        budget.push(original.clone());
        check_flow_budget(&budget)?;
        result.references.push(original.clone());
        if archived.contains(&original.id) {
            result.archived_documents.push(original);
            continue;
        }
        let extract = matches!(
            original.mime.as_str(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                | "application/vnd.ms-excel"
        ) || original.mime == "text/plain"
            || (original.mime == "application/pdf" && provider == Provider::DeepSeek);
        if extract {
            let extracted = crate::document_import::prepare_extraction(original.clone()).await?;
            if original.mime == "text/plain" {
                // A selected recognition file explicitly contains its local
                // resource links. Load those links, never other memo content.
                for id in document_asset_ids(&extracted.text) {
                    if queued.insert(id.clone()) {
                        if queued.len() > MAX_MEDIA_FILES {
                            return Err(AppError::validation("一次最多分析 600 个文件"));
                        }
                        let linked = get_asset(db, &id).await?;
                        original_bytes += linked.byte_size.max(0) as u64;
                        if original_bytes > MAX_ASSET_BYTES as u64 {
                            return Err(AppError::validation(
                                "AI 材料总大小最多 20 MiB，请减少文件或缩小图片",
                            ));
                        }
                        pending.push_back(linked);
                    }
                }
            }
            result
                .source_documents
                .push((original.id.clone(), extracted.text.clone()));
            result.warnings.extend(
                extracted
                    .warnings
                    .into_iter()
                    .map(|w| format!("{}：{w}", original.name)),
            );
            use base64::{engine::general_purpose::STANDARD, Engine};
            use sha2::{Digest, Sha256};
            let mut text = original.clone();
            text.name = format!("{}.extracted.txt", original.name);
            text.mime = "text/plain".into();
            text.byte_size = extracted.text.len() as i64;
            text.sha256 = hex::encode(Sha256::digest(extracted.text.as_bytes()));
            text.data_base64 = STANDARD.encode(extracted.text.as_bytes());
            if original.mime != "text/plain" {
                budget.push(text.clone());
            }
            budget.extend(extracted.images.iter().cloned());
            check_flow_budget(&budget)?;
            if provider == Provider::OpenAI {
                result.media.push(original);
            } else {
                result.media.push(text);
            }
            result.references.extend(extracted.images.iter().cloned());
            result.derived.extend(extracted.images.iter().cloned());
            result.media.extend(extracted.images);
        } else {
            result.media.push(original);
        }
    }
    validate_document_cycles(&result.source_documents)?;
    validate_media(Provider::OpenAI, &budget)?;
    validate_media(provider, &result.media)?;
    Ok(result)
}

fn document_asset_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for link in resource_links(text) {
        if !ids.contains(&link.id) {
            ids.push(link.id);
        }
    }
    ids
}

fn validate_document_cycles(documents: &[(String, String)]) -> AppResult<()> {
    let graph = documents
        .iter()
        .map(|(id, text)| (id.as_str(), document_asset_ids(text)))
        .collect::<std::collections::HashMap<_, _>>();
    fn visit<'a>(
        id: &'a str,
        graph: &'a std::collections::HashMap<&str, Vec<String>>,
        active: &mut std::collections::HashSet<&'a str>,
        done: &mut std::collections::HashSet<&'a str>,
    ) -> AppResult<()> {
        if done.contains(id) {
            return Ok(());
        }
        if !active.insert(id) {
            return Err(AppError::validation(
                "所选材料文件存在循环引用，请移除循环链接后重新生成",
            ));
        }
        if let Some(links) = graph.get(id) {
            for linked in links
                .iter()
                .filter(|link| graph.contains_key(link.as_str()))
            {
                visit(linked, graph, active, done)?;
            }
        }
        active.remove(id);
        done.insert(id);
        Ok(())
    }
    let mut active = std::collections::HashSet::new();
    let mut done = std::collections::HashSet::new();
    for id in graph.keys() {
        visit(id, &graph, &mut active, &mut done)?;
    }
    Ok(())
}

/// The editor records an exact source ID; old editor output has an exact
/// filename heading. Never infer recognition from similar business prose.
fn recognized_document_ids(
    source: &str,
    originals: &[ContentAsset],
) -> AppResult<std::collections::HashSet<String>> {
    use pulldown_cmark::{Event, Parser, Tag};
    let mut ids = std::collections::HashSet::new();
    let marker = regex::Regex::new(r"^<!-- lumen-extracted:([0-9a-fA-F-]{36}) -->$")
        .expect("source marker regex");
    let events = Parser::new(source).into_offset_iter().collect::<Vec<_>>();
    for (event, range) in &events {
        if let Event::Html(html) = event {
            if let Some(capture) = marker.captures(html.trim()) {
                if source[range.end..].trim().is_empty() {
                    return Err(AppError::validation(
                        "已识别来源标记后没有正文，请重新识别或移除该标记",
                    ));
                }
                if let Some(original) = originals.iter().find(|a| {
                    a.id.eq_ignore_ascii_case(&capture[1]) && !a.mime.starts_with("image/")
                }) {
                    ids.insert(original.id.clone());
                }
            }
        }
    }
    let explicit_names = originals
        .iter()
        .filter(|a| ids.contains(&a.id))
        .map(|a| a.name.as_str())
        .collect::<std::collections::HashSet<_>>();
    for original in originals {
        if original.mime.starts_with("image/")
            || ids.contains(&original.id)
            || explicit_names.contains(original.name.as_str())
        {
            continue;
        }
        let heading = format!("### {} · 识别内容", original.name);
        if events.iter().any(|(event, range)| {
            matches!(event, Event::Start(Tag::Heading { .. }))
                && source[range.clone()].trim() == heading
                && !source[range.end..].trim().is_empty()
        }) {
            if originals.iter().filter(|a| a.name == original.name).count() > 1 {
                return Err(AppError::validation(
                    "旧识别内容对应多个同名原文件，无法确定来源；请保留需要的原文件并重新识别",
                ));
            }
            ids.insert(original.id.clone());
        }
    }
    Ok(ids)
}

fn check_flow_budget(assets: &[ContentAsset]) -> AppResult<()> {
    // Budget includes originals plus expanded text/images, even if a provider only receives derivatives.
    validate_media_limits(assets)
}

pub fn validate_material_text(text: &str) -> AppResult<()> {
    if text.chars().count() > 80_000 {
        return Err(AppError::validation(
            "原始材料（含图片引用）最多 80000 字，请分段整理",
        ));
    }
    let references = regex::Regex::new(
        r#"!?\[[^\]\n]*\]\(lumen-asset:[0-9a-fA-F-]{36}(?: "(?:\\.|[^"\\\n])*")?\)"#,
    )
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
        let mut asset = get_asset(db, id).await?;
        total_bytes += asset.byte_size.max(0) as u64;
        if total_bytes > MAX_ASSET_BYTES as u64 {
            return Err(AppError::validation(
                "AI 材料总大小最多 20 MiB，请减少文件或缩小图片",
            ));
        }
        let needs_local = (asset.mime == "application/pdf" && provider == Provider::DeepSeek)
            || (matches!(
                asset.mime.as_str(),
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                    | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                    | "application/vnd.ms-excel"
            ) && provider != Provider::OpenAI);
        if needs_local {
            let source = asset.clone();
            let text =
                tokio::task::spawn_blocking(move || crate::document_import::extract_local(&source))
                    .await
                    .map_err(|e| AppError::internal(format!("AI 文件本机识别失败：{e}")))??;
            use base64::{engine::general_purpose::STANDARD, Engine};
            use sha2::{Digest, Sha256};
            asset.name = format!("{}.extracted.txt", asset.name);
            asset.mime = "text/plain".into();
            asset.byte_size = text.len() as i64;
            asset.sha256 = hex::encode(Sha256::digest(text.as_bytes()));
            asset.data_base64 = STANDARD.encode(text.as_bytes());
        }
        assets.push(asset);
    }
    validate_media(provider, &assets)?;
    Ok(assets)
}

fn validate_media_limits(media: &[ContentAsset]) -> AppResult<()> {
    if media.len() > MAX_MEDIA_FILES {
        return Err(AppError::validation("一次最多分析 600 个文件"));
    }
    if media.iter().map(|a| a.byte_size.max(0) as u64).sum::<u64>() > MAX_ASSET_BYTES as u64 {
        return Err(AppError::validation(
            "AI 材料总大小最多 20 MiB，请减少文件或缩小图片",
        ));
    }
    Ok(())
}

pub fn validate_media(provider: Provider, media: &[ContentAsset]) -> AppResult<()> {
    validate_media_limits(media)?;
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

pub fn message_content(message: &ChatMessage, req: &ChatRequest, include_media: bool) -> Value {
    if !include_media || message.role != "user" || req.media.is_empty() {
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

    #[tokio::test]
    async fn flow_attachment_entry_ignores_code_asset_ids_before_database_lookup() {
        let dir =
            std::env::temp_dir().join(format!("lumen-code-attachments-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let known = crate::content_assets::store_bytes(
            &db,
            "example.md",
            "示例不能补到正文。".as_bytes().to_vec(),
        )
        .await
        .unwrap();
        let unknown = uuid::Uuid::now_v7().to_string();
        let source = format!(
            "原操作。\n\n```md\n[示例](lumen-asset:{})\n```\n\n`![示例](lumen-asset:{unknown})`",
            known.id
        );
        let loaded =
            load_flow_media_for_source(&db, Provider::DeepSeek, &[known.id, unknown], &source)
                .await
                .expect("代码示例不能触发任何资源读取，包括不存在的资源");
        assert!(loaded.references.is_empty());
        assert!(loaded.media.is_empty());
        assert!(
            loaded.source_documents.is_empty(),
            "已存示例不能兜底追加到正文"
        );
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn flow_attachment_entry_keeps_explicit_files_without_source_uuid() {
        let dir = std::env::temp_dir().join(format!(
            "lumen-explicit-attachments-{}",
            uuid::Uuid::now_v7()
        ));
        let db = Db::init(&dir).await.unwrap();
        let explicit = crate::content_assets::store_bytes(
            &db,
            "selected.md",
            "附件原操作。".as_bytes().to_vec(),
        )
        .await
        .unwrap();
        let loaded = load_flow_media_for_source(
            &db,
            Provider::DeepSeek,
            std::slice::from_ref(&explicit.id),
            "按附件生成操作流程。",
        )
        .await
        .unwrap();
        assert_eq!(
            loaded.references.len(),
            1,
            "API 明确选中的附件不能因正文没写 UUID 丢失"
        );
        assert_eq!(loaded.references[0].id, explicit.id);
        assert_eq!(loaded.source_documents[0].1, "附件原操作。");
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn flow_attachment_entry_uses_real_reference_order_without_frontend_parser() {
        let dir = std::env::temp_dir().join(format!(
            "lumen-reference-attachments-{}",
            uuid::Uuid::now_v7()
        ));
        let db = Db::init(&dir).await.unwrap();
        let first = crate::content_assets::store_bytes(
            &db,
            "first.png",
            b"\x89PNG\r\n\x1a\nfirst".to_vec(),
        )
        .await
        .unwrap();
        let second = crate::content_assets::store_bytes(
            &db,
            "second.png",
            b"\x89PNG\r\n\x1a\nsecond".to_vec(),
        )
        .await
        .unwrap();
        let source = format!("`![示例](lumen-asset:{})`\n\n![第二图][second]\n\n![第一图][first]\n\n[first]: lumen-asset:{} \"原备注\"\n[second]: lumen-asset:{}", first.id, first.id, second.id);
        for supplied in [vec![first.id.clone(), second.id.clone()], vec![]] {
            let loaded = load_flow_media_for_source(&db, Provider::DeepSeek, &supplied, &source)
                .await
                .unwrap();
            assert_eq!(
                loaded.media.iter().map(|a| &a.id).collect::<Vec<_>>(),
                vec![&second.id, &first.id],
                "参考式原图必须按真实引用顺序加载，不能被代码示例或前端 regex 顺序改变"
            );
        }
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn selected_markdown_cycle_is_rejected_before_generation() {
        let dir =
            std::env::temp_dir().join(format!("lumen-material-cycle-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let id = uuid::Uuid::now_v7().to_string();
        let mut original = crate::content_assets::prepare_bytes(
            "cycle.md",
            format!("原操作。\n[循环材料](lumen-asset:{id})").into_bytes(),
        )
        .unwrap();
        original.id = id.clone();
        crate::content_assets::persist_assets(&db, &[original])
            .await
            .unwrap();
        let error = load_flow_media_for_source(
            &db,
            Provider::DeepSeek,
            std::slice::from_ref(&id),
            &format!("[材料](lumen-asset:{id})"),
        )
        .await
        .err()
        .expect("循环必须失败");
        assert!(error.message.contains("循环引用"));
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn explicit_recognition_source_distinguishes_same_name_files() {
        let first =
            crate::content_assets::prepare_bytes("SOP.docx", crate::document_import::tests::docx())
                .unwrap();
        let second =
            crate::content_assets::prepare_bytes("SOP.docx", crate::document_import::tests::docx())
                .unwrap();
        let source = format!("[原文件](lumen-asset:{})\n<!-- lumen-extracted:{} -->\n\n### SOP.docx · 识别内容\n\n已识别动作。\n\n[另一个原文件](lumen-asset:{})", first.id, first.id, second.id);
        let ids = recognized_document_ids(&source, &[first.clone(), second])
            .expect("明确来源ID不应被同名未识别文件阻断");
        assert_eq!(ids, std::collections::HashSet::from([first.id]));
    }

    #[test]
    fn legacy_recognition_never_guesses_ambiguous_names_or_code_examples() {
        let first =
            crate::content_assets::prepare_bytes("SOP.docx", crate::document_import::tests::docx())
                .unwrap();
        let second =
            crate::content_assets::prepare_bytes("SOP.docx", crate::document_import::tests::docx())
                .unwrap();
        assert!(recognized_document_ids(
            "### SOP.docx · 识别内容\n\n原操作。",
            &[first.clone(), second]
        )
        .is_err());
        assert!(recognized_document_ids(
            "```md\n### SOP.docx · 识别内容\n\n原操作。\n```",
            std::slice::from_ref(&first)
        )
        .unwrap()
        .is_empty());
        assert!(recognized_document_ids("原操作。", &[first])
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn unsupported_native_word_uses_local_text_without_mutating_original() {
        let dir = std::env::temp_dir().join(format!("lumen-local-ai-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let bytes = crate::document_import::tests::docx();
        let original = crate::content_assets::store_bytes(&db, "orders.docx", bytes.clone())
            .await
            .unwrap();
        let loaded = load_media(&db, Provider::DeepSeek, std::slice::from_ref(&original.id)).await;
        let persisted = get_asset(&db, &original.id).await.unwrap();
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!(decode_asset(&persisted).unwrap(), bytes);
        let loaded = loaded.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, original.id);
        assert_eq!(loaded[0].name, "orders.docx.extracted.txt");
        assert_eq!(loaded[0].mime, "text/plain");
        assert!(String::from_utf8(decode_asset(&loaded[0]).unwrap())
            .unwrap()
            .contains("订单核对"));
    }

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
        let parts = message_content(&message, &req, true);
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

    #[test]
    fn expanded_flow_budget_counts_originals_and_extracted_text() {
        let original = asset("orders.docx", b"PK\x03\x04");
        let image = asset("image.png", b"\x89PNG\r\n\x1a\nimage-bytes");
        let mut expanded = vec![image; 600];
        expanded.push(original.clone());
        assert!(check_flow_budget(&expanded)
            .unwrap_err()
            .message
            .contains("600"));
        let text = asset("extracted.txt", "字".repeat(100_001).as_bytes());
        assert!(validate_media(Provider::OpenAI, &[original, text])
            .unwrap_err()
            .message
            .contains("100000"));
        let mut bytes = vec![0; MAX_ASSET_BYTES / 2 + 1];
        bytes[..4].copy_from_slice(b"PK\x03\x04");
        let original = asset("large.docx", &bytes);
        let text = asset("extracted.txt", &vec![b'a'; MAX_ASSET_BYTES / 2]);
        assert!(check_flow_budget(&[original, text])
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
            let parts = message_content(&message, &req, true);
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
