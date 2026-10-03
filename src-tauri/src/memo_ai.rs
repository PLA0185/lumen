//! Generate an editable flow; only memo_save can persist it after confirmation.
use crate::{
    ai::{self, ChatMessage, ChatRequest, ProviderConfig},
    commands::AppState,
    content_assets::ContentAsset,
    error::{AppError, AppResult},
    memos::{FlowStep, SaveMemoInput},
};
use serde::Deserialize;
use tauri::State;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerateFlowInput {
    pub text: String,
    #[serde(default)]
    pub asset_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ModelFlow {
    title: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    body_md: String,
    steps: Vec<ModelStep>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ModelStep {
    title: String,
    #[serde(default)]
    owner: String,
    #[serde(default)]
    detail: String,
    #[serde(default)]
    asset_ids: Vec<String>,
}

fn without_unsourced_requirements(text: &str, source: &str) -> String {
    const LABELS: &[&str] = &[
        "完成标准",
        "例外情况",
        "待确认问题",
        "待确认",
        "所需材料",
        "材料清单",
        "前置条件",
        "审阅意见",
        "问题清单",
        "补充要求",
    ];
    let mut result = Vec::new();
    let mut suppress_section = false;
    let mut suppress_paragraph = false;
    for line in text.lines() {
        let trimmed = line.trim();
        let heading = trimmed.starts_with('#');
        if heading {
            suppress_section = false;
            suppress_paragraph = false;
        }
        let label = trimmed
            .trim_start_matches('#')
            .trim_start_matches([' ', '*', '-', '•'])
            .trim();
        if LABELS
            .iter()
            .any(|label_text| label.starts_with(label_text) && !source.contains(label_text))
        {
            suppress_section = heading;
            suppress_paragraph = !heading;
            continue;
        }
        if trimmed.is_empty() {
            suppress_paragraph = false;
        }
        if (!suppress_section && !suppress_paragraph) || trimmed.starts_with("![") {
            result.push(line);
        }
    }
    result.join("\n").trim().into()
}

fn validate_step_image_reuse(
    steps: &[FlowStep],
    source: &str,
    media: &[ContentAsset],
) -> AppResult<()> {
    let images = regex::Regex::new(
        r#"!\[[^\]\n]*\]\(lumen-asset:([0-9a-fA-F-]{36})(?: "(?:\\.|[^"\\\n])*")?\)"#,
    )
    .expect("constant image reference regex");
    for asset in media.iter().filter(|a| a.mime.starts_with("image/")) {
        let count_in = |text: &str| {
            images
                .captures_iter(text)
                .filter(|c| c[1].eq_ignore_ascii_case(&asset.id))
                .count()
        };
        let total_count: usize = steps.iter().map(|step| count_in(&step.detail)).sum();
        if total_count > count_in(source).max(1) {
            return Err(AppError::validation(
                "AI 在多个步骤重复使用了原文仅出现一次的图片，未保存错误流程；请重新生成",
            ));
        }
    }
    Ok(())
}

pub(crate) fn parse_flow(
    raw: &str,
    source: &str,
    media: &[ContentAsset],
) -> AppResult<SaveMemoInput> {
    if crate::ai_features::extract_json(raw)?
        .get("schemaVersion")
        .is_some()
    {
        return crate::flow_structure::parse(raw, source, media);
    }
    let model: ModelFlow = serde_json::from_value(crate::ai_features::extract_json(raw)?)
        .map_err(|_| AppError::validation("AI 返回的流程结构不完整，请重新生成"))?;
    if model.steps.is_empty() || model.steps.iter().any(|s| s.title.trim().is_empty()) {
        return Err(AppError::validation(
            "AI 流程必须包含有标题的步骤，请重新生成",
        ));
    }
    let references = regex::Regex::new(r"lumen-asset:([0-9a-fA-F-]{36})")
        .expect("constant asset reference regex");
    let markdown_references = regex::Regex::new(
        r#"(!?)\[[^\]\n]*\]\(lumen-asset:([0-9a-fA-F-]{36})(?: "(?:\\.|[^"\\\n])*")?\)"#,
    )
    .expect("constant asset markdown regex");
    for text in
        std::iter::once(model.body_md.as_str()).chain(model.steps.iter().map(|s| s.detail.as_str()))
    {
        for reference in references.captures_iter(text) {
            if !media
                .iter()
                .any(|asset| asset.id.eq_ignore_ascii_case(&reference[1]))
            {
                return Err(AppError::validation(
                    "AI 引用了未选择的图片或文件，请重新生成",
                ));
            }
        }
    }
    let sections = crate::document_import::source_sections(source)?;
    let mut draft = SaveMemoInput {
        id: None,
        expected_revision: None,
        title: model.title.trim().into(),
        category: model.category.trim().into(),
        kind: "flow".into(),
        body_md: format!(
            "{}\n\n## 原始材料\n\n{}",
            without_unsourced_requirements(&model.body_md, source),
            source
        ),
        steps: model
            .steps
            .into_iter()
            .map(|s| {
                let mut detail = without_unsourced_requirements(&s.detail, source);
                for id in s.asset_ids {
                    let asset = media
                        .iter()
                        .find(|asset| asset.id.eq_ignore_ascii_case(&id))
                        .ok_or_else(|| {
                            AppError::validation("AI 引用了未选择的图片或文件，请重新生成")
                        })?;
                    if !markdown_references.captures_iter(&detail).any(|reference| {
                        reference[2].eq_ignore_ascii_case(&asset.id)
                            && (reference[1] == *"!") == asset.mime.starts_with("image/")
                    }) {
                        let name = asset.name.replace(['[', ']', '\\', '\r', '\n'], "_");
                        detail.push_str(&format!(
                            "\n\n{}[{name}](lumen-asset:{})",
                            if asset.mime.starts_with("image/") {
                                "!"
                            } else {
                                ""
                            },
                            asset.id
                        ));
                    }
                }
                Ok(FlowStep {
                    group: None,
                    layout: None,
                    id: uuid::Uuid::now_v7().to_string(),
                    title: s.title.trim().into(),
                    owner: if source.contains(s.owner.trim()) {
                        s.owner.trim().into()
                    } else {
                        String::new()
                    },
                    detail,
                })
            })
            .collect::<AppResult<Vec<_>>>()?,
    };
    if !sections.is_empty() {
        draft.body_md = format!("## 原始材料\n\n{source}");
        let owner = regex::Regex::new(
            r"(?m)^\s*(?:\*\*)?(?:负责人|责任人)(?:\*\*)?[：:]\s*([^\n]{1,100})$",
        )
        .expect("constant explicit owner regex");
        draft.steps = sections
            .into_iter()
            .map(|section| FlowStep {
                group: None,
                layout: None,
                id: uuid::Uuid::now_v7().to_string(),
                title: section.title,
                owner: owner
                    .captures(&section.detail)
                    .map(|c| c[1].trim().to_owned())
                    .unwrap_or_default(),
                detail: section.detail,
            })
            .collect();
    }
    validate_step_image_reuse(&draft.steps, source, media)?;
    crate::memos::validate(&draft)?;
    Ok(draft)
}

#[tauri::command]
pub async fn ai_generate_flow(
    state: State<'_, AppState>,
    config: ProviderConfig,
    input: GenerateFlowInput,
) -> AppResult<SaveMemoInput> {
    let text = input.text.trim();
    if text.is_empty() && input.asset_ids.is_empty() {
        return Err(AppError::validation("请先粘贴文字、聊天记录或添加截图"));
    }
    crate::ai_media::validate_material_text(&input.text)?;
    let loaded = crate::ai_media::load_flow_media_for_source(
        &state.db,
        config.provider,
        &input.asset_ids,
        &input.text,
    )
    .await?;
    let source = flow_source(&input.text, &loaded);
    // Extracted document text is already in source with original image anchors.
    // Do not send a second copy as a file/text part for the model to split again.
    let media: Vec<_> = loaded
        .media
        .into_iter()
        .filter(|asset| {
            !loaded
                .source_documents
                .iter()
                .any(|(id, _)| id == &asset.id)
        })
        .collect();
    let manifest = media.iter().enumerate().map(|(index, asset)| serde_json::json!({"order":index+1,"id":asset.id,"name":asset.name,"mime":asset.mime})).collect::<Vec<_>>();
    let request = ChatRequest {
            config: config.clone(),
            system: Some(crate::flow_structure::SYSTEM.into()),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: format!("编号原文块（只能引用这些块进行分层）：\n{}\n\n随后提供的图片、文件按以下资源顺序表排列：\n{}", crate::flow_structure::prompt(&source)?, serde_json::to_string(&manifest).map_err(|e| AppError::validation(format!("资源顺序表编码失败：{e}")))?),
            }],
            json_output: true,
            max_output_tokens: None,
            media,
        };
    let (response, _) = analyze_flow(&config, request, &source, &loaded.references).await?;
    finish_flow(
        &state.db,
        &response.text,
        &source,
        &loaded.references,
        &loaded.derived,
        &loaded.warnings,
    )
    .await
}

/// Let the model repair an invalid analysis once; never replace it with local splits.
pub(crate) async fn analyze_flow(
    config: &ProviderConfig,
    mut request: ChatRequest,
    source: &str,
    references: &[ContentAsset],
) -> AppResult<(ai::ChatResponse, usize)> {
    for attempt in 1..=2 {
        let response = ai::chat(config, &request).await?;
        #[cfg(test)]
        if let Ok(folder) = std::env::var("LUMEN_SEMANTIC_FLOW_EVIDENCE") {
            std::fs::write(
                std::path::Path::new(&folder).join(format!("attempt-{attempt}.json")),
                &response.text,
            )
            .map_err(|e| AppError::internal(format!("显式验收证据写入失败：{e}")))?;
        }
        if response.truncated {
            return Err(AppError::validation(
                "AI分析因输出上限中断，未保存流程；请提高输出上限或分段分析",
            ));
        }
        let validation = crate::flow_structure::validate_protocol(&response.text)
            .and_then(|_| parse_flow(&response.text, source, references));
        match validation {
            Ok(_) => return Ok((response, attempt)),
            Err(error)
                if attempt == 1 && matches!(error.code, crate::error::ErrorCode::Validation) =>
            {
                request.messages.push(ChatMessage {
                    role: "assistant".into(),
                    content: response.text,
                });
                request.messages.push(ChatMessage{role:"user".into(),content:format!("刚才结构未通过校验：{}。请重新检查全部范围、原标题祖先和配图，修正整个JSON。目录必须context；step中最多两个hasImage=true的原文块，超过时按实际动作分开并沿用阶段分组，不能改为context藏掉正文。每个step填写简短title，不要输出titleBlock；原章节和小标题用groupPath引用，优先沿用本范围末块的ancestorIds并从chapterId开始。全部原文块仍连续覆盖一次，不补写正文。",error.message)});
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("bounded analysis returns on the second attempt")
}

fn without_recognition_transport(text: &str, loaded: &crate::ai_media::FlowMedia) -> String {
    use pulldown_cmark::{Event, Parser, Tag};
    let transport_ranges = Parser::new(text)
        .into_offset_iter()
        .filter_map(|(event, range)| {
            let is_transport = loaded.archived_documents.iter().any(|asset| match &event {
                Event::Html(html) => html
                    .trim()
                    .eq_ignore_ascii_case(&format!("<!-- lumen-extracted:{} -->", asset.id)),
                Event::Start(Tag::Heading { .. }) => {
                    text[range.clone()].trim() == format!("### {} · 识别内容", asset.name)
                }
                _ => false,
            });
            is_transport.then_some(range)
        })
        .collect::<Vec<_>>();
    let mut material = text.to_owned();
    for range in transport_ranges.into_iter().rev() {
        material.replace_range(range, "");
    }
    material
}

fn expand_source_tokens(
    text: &str,
    loaded: &crate::ai_media::FlowMedia,
    expanded: &mut std::collections::HashSet<String>,
) -> String {
    let mut material = without_recognition_transport(text, loaded);
    let mut replacements = Vec::new();
    let mut consumed = 0;
    // Decide expansions in document order, then replace backwards so byte
    // offsets remain valid. Code examples never enter this list.
    for link in crate::ai_media::resource_links(&material) {
        if link.range.start < consumed {
            continue;
        }
        let replacement = if loaded
            .archived_documents
            .iter()
            .any(|a| a.id.eq_ignore_ascii_case(&link.id))
        {
            Some(String::new())
        } else if let Some((id, document)) = loaded
            .source_documents
            .iter()
            .find(|(id, _)| id.eq_ignore_ascii_case(&link.id))
        {
            Some(if expanded.insert(id.clone()) {
                format!(
                    "\n\n{}\n\n",
                    expand_source_tokens(document, loaded, expanded)
                )
            } else {
                String::new()
            })
        } else if !link.inline {
            let label = link
                .label
                .replace('\\', "\\\\")
                .replace('[', "\\[")
                .replace(']', "\\]");
            let title = if link.title.is_empty() {
                String::new()
            } else {
                format!(
                    " \"{}\"",
                    link.title.replace('\\', "\\\\").replace('"', "\\\"")
                )
            };
            Some(format!(
                "{}[{label}](lumen-asset:{}{title})",
                if link.image { "!" } else { "" },
                link.id
            ))
        } else {
            None
        };
        if let Some(replacement) = replacement {
            consumed = link.range.end;
            replacements.push((link.range, replacement));
        }
    }
    for (range, replacement) in replacements.into_iter().rev() {
        material.replace_range(range, &replacement);
    }
    material
}

fn flow_source(text: &str, loaded: &crate::ai_media::FlowMedia) -> String {
    let mut expanded = loaded
        .archived_documents
        .iter()
        .map(|a| a.id.clone())
        .collect::<std::collections::HashSet<_>>();
    // Expand at the selected file's original position. File transport labels are
    // retained in the archive, not promoted to business steps or fake headings.
    let mut source = expand_source_tokens(text, loaded, &mut expanded);
    for (id, document) in &loaded.source_documents {
        if expanded.insert(id.clone()) {
            source.push_str(&format!(
                "\n\n{}\n",
                expand_source_tokens(document, loaded, &mut expanded)
            ));
        }
    }
    for asset in &loaded.references {
        if !expanded.contains(&asset.id)
            && !crate::ai_media::resource_links(&source)
                .iter()
                .any(|link| link.id == asset.id)
        {
            source.push_str(&format!(
                "\n{}[原始材料](lumen-asset:{})\n",
                if asset.mime.starts_with("image/") {
                    "!"
                } else {
                    ""
                },
                asset.id
            ));
        }
    }
    source
}

async fn finish_flow(
    db: &crate::db::Db,
    raw: &str,
    source: &str,
    references: &[ContentAsset],
    derived: &[ContentAsset],
    warnings: &[String],
) -> AppResult<SaveMemoInput> {
    let mut draft = parse_flow(raw, source, references)?;
    let originals: Vec<_> = references
        .iter()
        .filter(|a| {
            !a.mime.starts_with("image/")
                && !crate::ai_media::resource_links(source)
                    .iter()
                    .any(|link| link.id == a.id)
        })
        .collect();
    if !originals.is_empty() {
        draft.body_md.push_str("\n\n## 原文件\n\n");
        for original in originals {
            let name = original.name.replace(['[', ']', '\\', '\r', '\n'], "_");
            draft
                .body_md
                .push_str(&format!("[{name}](lumen-asset:{})\n", original.id));
        }
        crate::memos::validate(&draft)?;
    }
    if !warnings.is_empty() {
        draft.body_md.push_str("\n\n## 材料识别提示\n\n");
        for warning in warnings {
            draft
                .body_md
                .push_str(&format!("- {}\n", warning.replace(['\r', '\n'], " ")));
        }
        crate::memos::validate(&draft)?;
    }
    crate::content_assets::persist_assets(db, derived).await?;
    Ok(draft)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_image_metadata_and_definition_keep_markdown_anchor() {
        let outer = crate::content_assets::prepare_bytes("outer.md", b"outer".to_vec()).unwrap();
        let image = test_image(&uuid::Uuid::now_v7().to_string());
        let definition = format!("[pic]: lumen-asset:{} \"原备注\"", image.id);
        let text = format!("前操作。\n\n![原图][pic]\n\n后操作。\n\n![原图][pic]\n\n{definition}");
        let loaded = crate::ai_media::FlowMedia {
            media: vec![image.clone()],
            references: vec![outer.clone(), image.clone()],
            warnings: vec![],
            derived: vec![],
            source_documents: vec![(outer.id.clone(), text)],
            archived_documents: vec![],
        };
        let source = flow_source(&format!("[材料](lumen-asset:{})", outer.id), &loaded);
        let inline = format!("![原图](lumen-asset:{} \"原备注\")", image.id);
        assert_eq!(
            source.matches(&inline).count(),
            2,
            "引用式图片应在两个原锚点保留alt/title"
        );
        assert!(source.find("前操作。").unwrap() < source.find(&inline).unwrap());
        assert!(source.find(&inline).unwrap() < source.find("后操作。").unwrap());
        assert!(source.rfind(&inline).unwrap() > source.find("后操作。").unwrap());
        assert!(source.contains(&definition), "原定义行仍保留在全文归档");
        let blocks: serde_json::Value =
            serde_json::from_str(&crate::flow_structure::prompt(&source).unwrap()).unwrap();
        let count = blocks.as_array().unwrap().len();
        let plan = serde_json::json!({"schemaVersion":2,"steps":[{"begin":1,"end":count-1,"kind":"step","title":"对照原图核对"},{"begin":count,"end":count,"kind":"context"}]}).to_string();
        let draft = parse_flow(&plan, &source, &[image]).unwrap();
        assert_eq!(draft.steps[0].detail.matches(&inline).count(), 2);
        assert!(draft.body_md.contains(&definition));
    }

    #[test]
    fn code_only_resource_uuid_is_literal_at_markdown_anchor() {
        let source = "对照语法记录操作。\n\n```md\n![代码示例](lumen-asset:00000000-0000-7000-8000-000000000001)\n```";
        let count = crate::flow_structure::packets(source).len();
        let plan = serde_json::json!({"schemaVersion":2,"steps":[{"begin":1,"end":count,"kind":"step","title":"对照语法记录操作"}]}).to_string();
        let draft = parse_flow(&plan, source, &[]).expect("代码示例UUID不能当未选择附件");
        assert_eq!(draft.steps[0].detail, source);
    }

    #[test]
    fn reference_document_expands_at_real_markdown_anchor() {
        let outer = crate::content_assets::prepare_bytes("outer.md", b"outer".to_vec()).unwrap();
        let inner = crate::content_assets::prepare_bytes("inner.md", b"inner".to_vec()).unwrap();
        let source = format!(
            "前操作。\n\n[内层][m]\n\n后操作。\n\n[m]: lumen-asset:{}",
            inner.id
        );
        let loaded = crate::ai_media::FlowMedia {
            media: vec![],
            references: vec![outer.clone(), inner.clone()],
            warnings: vec![],
            derived: vec![],
            source_documents: vec![(outer.id.clone(), source), (inner.id, "中间操作。".into())],
            archived_documents: vec![],
        };
        let source = flow_source(&format!("[外层](lumen-asset:{})", outer.id), &loaded);
        assert!(source.find("前操作。").unwrap() < source.find("中间操作。").unwrap());
        assert!(
            source.find("中间操作。").unwrap() < source.find("后操作。").unwrap(),
            "引用式资源必须在真实链接位置展开"
        );
    }

    #[test]
    fn code_asset_example_cannot_consume_real_markdown_anchor() {
        let outer = crate::content_assets::prepare_bytes("outer.md", b"outer".to_vec()).unwrap();
        let inner = crate::content_assets::prepare_bytes("inner.md", b"inner".to_vec()).unwrap();
        let example = format!("```md\n[内层](lumen-asset:{})\n```", inner.id);
        let source = format!(
            "{example}\n\n前操作。\n\n[内层](lumen-asset:{})\n\n后操作。",
            inner.id
        );
        let loaded = crate::ai_media::FlowMedia {
            media: vec![],
            references: vec![outer.clone(), inner.clone()],
            warnings: vec![],
            derived: vec![],
            source_documents: vec![(outer.id.clone(), source), (inner.id, "中间操作。".into())],
            archived_documents: vec![],
        };
        let source = flow_source(&format!("[外层](lumen-asset:{})", outer.id), &loaded);
        assert!(
            source.contains(&example),
            "代码字面量必须逐字保留，不展开或消耗业务材料"
        );
        assert!(source.find("前操作。").unwrap() < source.find("中间操作。").unwrap());
        assert!(source.find("中间操作。").unwrap() < source.find("后操作。").unwrap());
    }

    #[tokio::test]
    async fn recognized_markdown_file_reuses_its_local_image_references() {
        let dir = std::env::temp_dir().join(format!(
            "lumen-recognized-markdown-{}",
            uuid::Uuid::now_v7()
        ));
        let db = crate::db::Db::init(&dir).await.unwrap();
        let original = crate::content_assets::store_bytes(
            &db,
            "SOP.docx",
            crate::document_import::tests::ordered_sop(),
        )
        .await
        .unwrap();
        let extraction = crate::document_import::extract_asset(&db, original.clone())
            .await
            .unwrap();
        let markdown = crate::content_assets::store_bytes(
            &db,
            "SOP-识别内容.md",
            extraction.text.as_bytes().to_vec(),
        )
        .await
        .unwrap();
        let source = format!("[SOP.docx](lumen-asset:{})\n\n<!-- lumen-extracted:{} -->\n\n[识别内容.md](lumen-asset:{})", original.id, original.id, markdown.id);
        let loaded = crate::ai_media::load_flow_media_for_source(
            &db,
            crate::ai::Provider::DeepSeek,
            &[original.id.clone(), markdown.id.clone()],
            &source,
        )
        .await
        .unwrap();
        for image in &extraction.images {
            assert!(
                loaded.references.iter().any(|a| a.id == image.id),
                "选中识别MD内部明确引用的原图不能漏载"
            );
            assert!(loaded.media.iter().any(|a| a.id == image.id));
        }
        assert!(loaded.derived.is_empty());
        let material = flow_source(&source, &loaded);
        assert_eq!(material.matches("取得本周表格。").count(), 1);
        assert_eq!(material.matches("lumen-asset:").count(), 2);
        let outer = crate::content_assets::store_bytes(
            &db,
            "outer.md",
            format!(
                "原前言。\n\n[内层材料](lumen-asset:{})\n\n原结尾。",
                markdown.id
            )
            .into_bytes(),
        )
        .await
        .unwrap();
        let loaded = crate::ai_media::load_flow_media_for_source(
            &db,
            crate::ai::Provider::DeepSeek,
            std::slice::from_ref(&outer.id),
            &format!("[材料](lumen-asset:{})", outer.id),
        )
        .await
        .unwrap();
        let nested_source = flow_source(&format!("[材料](lumen-asset:{})", outer.id), &loaded);
        assert!(
            nested_source.find("原前言。").unwrap() < nested_source.find("取得本周表格。").unwrap()
        );
        assert!(
            nested_source.find("按表格生成单据 & 核对。").unwrap()
                < nested_source.find("原结尾。").unwrap()
        );
        assert_eq!(nested_source.matches("lumen-asset:").count(), 2);
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn already_recognized_word_is_not_expanded_twice() {
        let dir =
            std::env::temp_dir().join(format!("lumen-recognized-flow-{}", uuid::Uuid::now_v7()));
        let db = crate::db::Db::init(&dir).await.unwrap();
        let original = crate::content_assets::store_bytes(
            &db,
            "SOP.docx",
            crate::document_import::tests::ordered_sop(),
        )
        .await
        .unwrap();
        let extraction = crate::document_import::extract_asset(&db, original.clone())
            .await
            .unwrap();
        let mut ids = vec![original.id.clone()];
        ids.extend(extraction.images.iter().map(|a| a.id.clone()));
        for marker in [
            format!("<!-- lumen-extracted:{} -->\n", original.id),
            String::new(),
        ] {
            let text = format!(
                "[SOP.docx](lumen-asset:{})\n\n{marker}### SOP.docx · 识别内容\n\n{}",
                original.id, extraction.text
            );
            let loaded = crate::ai_media::load_flow_media_for_source(
                &db,
                crate::ai::Provider::DeepSeek,
                &ids,
                &text,
            )
            .await
            .unwrap();
            let source = flow_source(&text, &loaded);
            assert_eq!(
                source.matches("取得本周表格。").count(),
                1,
                "手动识别正文不能再展开一次"
            );
            assert_eq!(source.matches("按表格生成单据 & 核对。").count(), 1);
            for image in &extraction.images {
                assert_eq!(source.matches(&image.id).count(), 1);
            }
            assert!(
                loaded.derived.is_empty(),
                "复用已有正文图片，不生成第二套资源"
            );
            assert!(!source.contains("lumen-extracted:"));
            assert!(!source.contains(&original.id));
            let blocks: serde_json::Value =
                serde_json::from_str(&crate::flow_structure::prompt(&source).unwrap()).unwrap();
            let blocks = blocks.as_array().unwrap();
            let first = blocks
                .iter()
                .position(|b| b["text"].as_str().unwrap().trim().starts_with("## 1."))
                .unwrap()
                + 1;
            let second = blocks
                .iter()
                .position(|b| b["text"].as_str().unwrap().trim().starts_with("## 2."))
                .unwrap()
                + 1;
            let plan = serde_json::json!({"schemaVersion":2,"steps":[
                {"begin":1,"end":first-1,"kind":"context"},
                {"begin":first,"end":second-1,"kind":"step","groupPath":[first],"title":"下载表格"},
                {"begin":second,"end":blocks.len(),"kind":"step","groupPath":[second],"title":"创建单据"}
            ]});
            let draft = finish_flow(
                &db,
                &plan.to_string(),
                &source,
                &loaded.references,
                &loaded.derived,
                &loaded.warnings,
            )
            .await
            .unwrap();
            assert!(
                draft
                    .body_md
                    .contains(&format!("[SOP.docx](lumen-asset:{})", original.id)),
                "复用识别正文仍保留原文件存档"
            );
            assert_eq!(draft.steps.len(), 2);
            let asset_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM content_assets")
                .fetch_one(db.pool())
                .await
                .unwrap();
            assert_eq!(asset_count, 3, "重复生成不能重新写入一套图片");
        }
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn heading_material_must_not_return_a_local_plan_before_ai_analysis() {
        let source = include_str!("memo_ai.rs");
        let route = source
            .split("pub async fn ai_generate_flow(")
            .nth(1)
            .unwrap()
            .split("fn flow_source(")
            .next()
            .unwrap();
        assert!(route.contains("analyze_flow("));
        assert!(source
            .split("pub(crate) async fn analyze_flow(")
            .nth(1)
            .unwrap()
            .split("fn flow_source(")
            .next()
            .unwrap()
            .contains("ai::chat("));
        assert!(
            !route.contains("explicit_plan("),
            "生成入口必须真的调用 AI 分析，不能遇到标题便提前返回本机计划"
        );
    }

    #[tokio::test]
    #[ignore = "explicit opt-in with an external selected DOCX, configured real model and isolated database"]
    async fn real_selected_word_generates_source_only_draft() {
        assert_eq!(
            std::env::var("LUMEN_RUN_REAL_WORD_FLOW").as_deref(),
            Ok("1")
        );
        let evidence =
            std::path::PathBuf::from(std::env::var("LUMEN_REAL_WORD_FLOW_DIRECTORY").unwrap());
        let config: ProviderConfig = serde_json::from_slice(
            &std::fs::read(evidence.join("real-model-config.json")).unwrap(),
        )
        .unwrap();
        let db = crate::db::Db::init(&evidence.join("real-word-test-data"))
            .await
            .unwrap();
        let original = crate::content_assets::store_bytes(
            &db,
            "selected.docx",
            std::fs::read(evidence.join("original.docx")).unwrap(),
        )
        .await
        .unwrap();
        let loaded = crate::ai_media::load_flow_media(
            &db,
            config.provider,
            std::slice::from_ref(&original.id),
        )
        .await
        .unwrap();
        let source = flow_source(
            &format!("[selected.docx](lumen-asset:{})", original.id),
            &loaded,
        );
        std::fs::write(evidence.join("word-source.md"), &source).unwrap();
        std::fs::write(
            evidence.join("word-packets.json"),
            crate::flow_structure::prompt(&source).unwrap(),
        )
        .unwrap();
        let manifest: Vec<_> = loaded
            .media
            .iter()
            .filter(|a| !loaded.source_documents.iter().any(|(id, _)| id == &a.id))
            .enumerate()
            .map(|(i, a)| serde_json::json!({"order":i+1,"id":a.id,"name":a.name,"mime":a.mime}))
            .collect();
        let request = ChatRequest { config: config.clone(), system: Some(crate::flow_structure::SYSTEM.into()), messages: vec![ChatMessage { role: "user".into(), content: format!("编号原文块（只能引用这些块进行分层）：\n{}\n\n随后提供的图片、文件按以下资源顺序表排列：\n{}", crate::flow_structure::prompt(&source).unwrap(), serde_json::to_string(&manifest).unwrap()) }], json_output: true, max_output_tokens: None, media: loaded.media.iter().filter(|a| !loaded.source_documents.iter().any(|(id, _)| id == &a.id)).cloned().collect() };
        let (response, _) = analyze_flow(&config, request, &source, &loaded.references)
            .await
            .unwrap();
        std::fs::write(evidence.join("word-response.json"), &response.text).unwrap();
        crate::flow_structure::validate_protocol(&response.text).unwrap();
        let draft = finish_flow(
            &db,
            &response.text,
            &source,
            &loaded.references,
            &loaded.derived,
            &loaded.warnings,
        )
        .await;
        std::fs::write(
            evidence.join("word-result.json"),
            serde_json::to_vec_pretty(&draft).unwrap(),
        )
        .unwrap();
        let draft = draft.unwrap();
        assert!(
            !draft
                .steps
                .iter()
                .any(|s| s.detail.trim() == "到这里亚马逊的操作流程基本上已经完成了。"),
            "真实 Word 的阶段收尾不能冒充新操作"
        );
        assert!(
            !draft.steps.iter().any(|s| s
                .detail
                .trim_start()
                .starts_with("例如下图中取得的产品型号为")
                && !s.detail.contains("这步之后根据")),
            "型号示例必须附在对应操作中"
        );
        assert!(
            draft
                .body_md
                .contains(&format!("lumen-asset:{}", original.id)),
            "原文件保留在归档"
        );
        assert!(draft.steps.iter().all(|s| !s.title.contains("文件正文")));
        let memo_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memo_documents")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(memo_count, 0, "生成草稿不能直接保存记录");
        println!(
            "verified source-only draft: {} steps, {} original embedded images",
            draft.steps.len(),
            loaded.derived.len()
        );
        db.pool().close().await;
    }

    #[tokio::test]
    async fn document_expands_at_its_anchor_and_original_file_stays_in_archive() {
        let original = crate::content_assets::prepare_bytes(
            "原文件.docx",
            crate::document_import::tests::docx(),
        )
        .unwrap();
        let image = test_image(&uuid::Uuid::now_v7().to_string());
        let document = format!("## 1. 下载\n\n原文操作。\n![原图](lumen-asset:{})\n图1 原图\n\n## 2. 保存\n\n保存原单号。", image.id);
        let loaded = crate::ai_media::FlowMedia {
            media: vec![],
            references: vec![original.clone(), image.clone()],
            warnings: vec![],
            derived: vec![],
            source_documents: vec![(original.id.clone(), document.clone())],
            archived_documents: vec![],
        };
        let text = format!("用户前言。\n\n[原文件.docx](lumen-asset:{})", original.id);
        let source = flow_source(&text, &loaded);
        assert!(source.starts_with("用户前言。"));
        assert_eq!(source.matches("## 1. 下载").count(), 1);
        assert!(!source.contains("文件正文结束"));
        assert!(source.contains(&document));
        let source = flow_source(
            &format!("[原文件.docx](lumen-asset:{})", original.id),
            &loaded,
        );
        let blocks: serde_json::Value =
            serde_json::from_str(&crate::flow_structure::prompt(&source).unwrap()).unwrap();
        let chapters: Vec<_> = blocks
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["text"].as_str().unwrap().trim_start().starts_with("## "))
            .map(|p| p["id"].as_u64().unwrap())
            .collect();
        let count = blocks.as_array().unwrap().len() as u64;
        let plan = serde_json::json!({"schemaVersion":2,"steps":[{"begin":1,"end":chapters[1]-1,"titleBlock":chapters[0],"groupPath":[chapters[0]]},{"begin":chapters[1],"end":count,"titleBlock":chapters[1],"groupPath":[chapters[1]]}]}).to_string();
        let dir = std::env::temp_dir().join(format!("lumen-word-archive-{}", uuid::Uuid::now_v7()));
        let db = crate::db::Db::init(&dir).await.unwrap();
        let draft = finish_flow(&db, &plan, &source, &loaded.references, &[], &[])
            .await
            .unwrap();
        assert_eq!(draft.steps.len(), 2);
        assert!(draft.steps[0].detail.contains(&image.id));
        assert!(!draft.steps[1].detail.contains(&image.id));
        assert!(draft
            .body_md
            .contains(&format!("[原文件.docx](lumen-asset:{})", original.id)));
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn extracted_word_source_does_not_invent_wrapper_headings() {
        let dir = std::env::temp_dir().join(format!("lumen-word-source-{}", uuid::Uuid::now_v7()));
        let db = crate::db::Db::init(&dir).await.unwrap();
        let original = crate::content_assets::store_bytes(
            &db,
            "中文SOP.docx",
            crate::document_import::tests::docx(),
        )
        .await
        .unwrap();
        let expected = crate::document_import::extract_local(&original).unwrap();
        let loaded = crate::ai_media::load_flow_media(
            &db,
            crate::ai::Provider::DeepSeek,
            std::slice::from_ref(&original.id),
        )
        .await
        .unwrap();
        assert_eq!(
            loaded.source_documents[0].1, expected,
            "文件名和正文结束标记不是用户流程标题"
        );
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn indexed_structure_uses_original_content_and_keeps_images_with_their_text() {
        let image = test_image(&uuid::Uuid::now_v7().to_string());
        let source = format!("## 2. 亚马逊发货\n\n### 2.1 进入货件页面\n\n点击库存中的货件。\n\n![原图](lumen-asset:{})\n\n图 1 货件入口\n\n### 2.2 输入数量\n\n填写发货数量，确认总数。", image.id);
        let plan = r#"{"schemaVersion":2,"steps":[{"begin":1,"end":3,"titleBlock":2,"groupPath":[1]},{"begin":4,"end":5,"titleBlock":4,"groupPath":[1]}]}"#;
        let draft = parse_flow(plan, &source, std::slice::from_ref(&image))
            .expect("只允许引用原文块的结构方案应可生成草稿");
        assert_eq!(draft.steps.len(), 2);
        assert_eq!(draft.steps[0].title, "2.1 进入货件页面");
        assert!(draft.steps[0].detail.contains("点击库存中的货件。"));
        assert!(draft.steps[0]
            .detail
            .contains(&format!("lumen-asset:{}", image.id)));
        assert!(draft.steps[0].detail.contains("图 1 货件入口"));
        assert!(!draft.steps[1].detail.contains("lumen-asset:"));
        assert_eq!(draft.steps[1].detail.trim(), "填写发货数量，确认总数。");
        let group = serde_json::to_value(&draft).unwrap();
        assert_eq!(group["steps"][0]["group"]["title"], "2. 亚马逊发货");
        assert_eq!(group["steps"][0]["group"], group["steps"][1]["group"]);
        assert!(draft.body_md.contains(&source));
    }

    #[test]
    fn indexed_structure_rejects_added_content_gaps_repetition_and_reordering() {
        let source = "打开货件页面。\n\n填写数量。\n\n保存。";
        for plan in [
            r#"{"schemaVersion":2,"steps":[{"begin":1,"end":3,"detail":"新增完成标准"}]}"#,
            r#"{"schemaVersion":2,"steps":[{"begin":1,"end":1},{"begin":3,"end":3}]}"#,
            r#"{"schemaVersion":2,"steps":[{"begin":1,"end":2},{"begin":2,"end":3}]}"#,
            r#"{"schemaVersion":2,"steps":[{"begin":3,"end":3},{"begin":1,"end":2}]}"#,
        ] {
            assert!(
                parse_flow(plan, source, &[]).is_err(),
                "不允许添写、漏段、重复或调换原文"
            );
        }
    }

    #[test]
    fn unsolicited_completion_exception_and_review_sections_are_removed() {
        let image = test_image(&uuid::Uuid::now_v7().to_string());
        let source = "从出货计划处下载本周的出货计划 Excel 表格。";
        let raw = serde_json::json!({"title":"流程","bodyMd":"## 背景\n出货操作\n\n## 待确认问题\n- 文档未提供截图，入口待确认。\n\n## 原文规则\n核对数量。", "steps":[{"title":"下载出货计划", "detail":"从出货计划处下载本周的出货计划 Excel 表格。\n\n**完成标准**：取得表格作为数量基准。\n\n**例外情况**：本章节材料未提供对应截图，下载入口与界面待确认。\n\n**待确认**：具体菜单待确认。", "assetIds":[image.id]}]}).to_string();
        let draft = parse_flow(&raw, source, std::slice::from_ref(&image)).unwrap();
        assert!(!draft.body_md.contains("待确认问题"));
        assert!(draft.body_md.contains("## 原文规则\n核对数量。"));
        assert!(draft.body_md.ends_with(source));
        let detail = &draft.steps[0].detail;
        assert!(detail.starts_with(source));
        for marker in ["完成标准", "例外情况", "待确认", "未提供对应截图"] {
            assert!(!detail.contains(marker), "不得附加 {marker}");
        }
        assert!(
            detail.contains(&format!("lumen-asset:{}", image.id)),
            "原图仍在对应步骤"
        );
    }

    #[test]
    fn original_explicit_completion_or_exception_is_preserved_verbatim() {
        let source = "**完成标准**：原文规定的数量核对。\n\n**例外情况**：退货流程另行处理。";
        let raw = serde_json::json!({"title":"流程","steps":[{"title":"核对","detail":source}]})
            .to_string();
        let draft = parse_flow(&raw, source, &[]).unwrap();
        assert_eq!(draft.steps[0].detail, source);
    }

    #[test]
    fn numbered_source_chapters_override_ai_splitting_reordering_and_image_reuse() {
        let first = test_image(&uuid::Uuid::now_v7().to_string());
        let second = test_image(&uuid::Uuid::now_v7().to_string());
        let source = format!("出货SOP\n概览：下载、创建。\n\n## 1. 下载表格\n取得本周表格。\n![原图](lumen-asset:{})\n\n## 2. 创建单据\n按表格生成单据。\n![原图](lumen-asset:{})", first.id, second.id);
        let raw = serde_json::json!({"title":"出货SOP","bodyMd":"待确认：需补截图", "steps":[{"title":"创建单据","detail":"完成标准：核对完成。","assetIds":[first.id,second.id]},{"title":"填写单据","assetIds":[first.id]},{"title":"下载表格","assetIds":[second.id]}]}).to_string();
        let draft = parse_flow(&raw, &source, &[first.clone(), second.clone()]).unwrap();
        assert_eq!(draft.steps.len(), 2, "已有原文章节不可任意拆成更多步骤");
        assert_eq!(draft.steps[0].title, "1. 下载表格");
        assert_eq!(draft.steps[1].title, "2. 创建单据");
        assert_eq!(draft.steps[0].owner, "");
        assert_eq!(
            draft.steps[0].detail,
            format!("取得本周表格。\n![原图](lumen-asset:{})", first.id)
        );
        assert_eq!(
            draft.steps[1].detail,
            format!("按表格生成单据。\n![原图](lumen-asset:{})", second.id)
        );
        assert!(!draft.body_md.contains("待确认"));
    }

    #[test]
    fn unanchored_ai_duplicate_image_assignment_is_rejected() {
        let image = test_image(&uuid::Uuid::now_v7().to_string());
        let source = format!("准备后核对。\n![原图](lumen-asset:{})", image.id);
        let raw = serde_json::json!({"title":"流程","steps":[{"title":"准备","assetIds":[image.id]},{"title":"核对","assetIds":[image.id]}]}).to_string();
        assert!(
            parse_flow(&raw, &source, &[image]).is_err(),
            "AI 不能把仅出现一次的原图随意放到多个步骤"
        );
    }

    #[test]
    fn image_captions_cannot_bypass_duplicate_reference_guard() {
        let image = test_image(&uuid::Uuid::now_v7().to_string());
        let token = format!("![图片](lumen-asset:{} \"发货单备注\")", image.id);
        let raw = serde_json::json!({"title":"流程","steps":[{"title":"准备","detail":token},{"title":"核对","detail":token}]}).to_string();
        assert!(
            parse_flow(&raw, &token, &[image]).is_err(),
            "图片备注不能绕过原文重复配图校验"
        );
    }

    #[tokio::test]
    async fn later_flow_budget_failure_leaves_only_original_assets() {
        use std::io::{Cursor, Write};
        let mut zip =
            zip::ZipWriter::new_append(Cursor::new(crate::document_import::tests::docx())).unwrap();
        zip.start_file(
            "word/media/image1.png",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"\x89PNG\r\n\x1a\nimage-bytes").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let dir = std::env::temp_dir().join(format!("lumen-flow-reject-{}", uuid::Uuid::now_v7()));
        let db = crate::db::Db::init(&dir).await.unwrap();
        let original = crate::content_assets::store_bytes(&db, "orders.docx", bytes.clone())
            .await
            .unwrap();
        let large = crate::content_assets::store_bytes(
            &db,
            "large.txt",
            vec![b'a'; crate::content_assets::MAX_ASSET_BYTES],
        )
        .await
        .unwrap();
        for _ in 0..2 {
            assert!(crate::ai_media::load_flow_media(
                &db,
                crate::ai::Provider::DeepSeek,
                &[original.id.clone(), large.id.clone()]
            )
            .await
            .is_err());
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM content_assets")
                .fetch_one(db.pool())
                .await
                .unwrap();
            assert_eq!(
                count, 2,
                "rejected generation must not leave embedded images"
            );
        }
        assert_eq!(
            crate::content_assets::decode_asset(
                &crate::content_assets::get_asset(&db, &original.id)
                    .await
                    .unwrap()
            )
            .unwrap(),
            bytes
        );
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn derived_image_insert_failure_rolls_back_all_images() {
        use std::io::{Cursor, Write};
        let mut zip = zip::ZipWriter::new_append(Cursor::new(
            crate::document_import::tests::docx_image_source(&["image1.png", "image2.png"]),
        ))
        .unwrap();
        for name in ["word/media/image1.png", "word/media/image2.png"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"\x89PNG\r\n\x1a\nimage-bytes").unwrap();
        }
        let dir =
            std::env::temp_dir().join(format!("lumen-derived-rollback-{}", uuid::Uuid::now_v7()));
        let db = crate::db::Db::init(&dir).await.unwrap();
        let original = crate::content_assets::store_bytes(
            &db,
            "orders.docx",
            zip.finish().unwrap().into_inner(),
        )
        .await
        .unwrap();
        sqlx::query("CREATE TRIGGER reject_second_image BEFORE INSERT ON content_assets WHEN NEW.name LIKE '%image2%' BEGIN SELECT RAISE(ABORT, 'fixture insert failure'); END").execute(db.pool()).await.unwrap();
        assert!(crate::document_import::extract_asset(&db, original)
            .await
            .is_err());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM content_assets")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(count, 1, "failed second insert must roll back first image");
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn word_generation_exposes_embedded_image_without_saving_memo() {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use std::io::{Cursor, Write};
        let mut zip = zip::ZipWriter::new_append(Cursor::new(
            crate::document_import::tests::docx_image_source(&["image1.png"]),
        ))
        .unwrap();
        zip.start_file(
            "word/media/image1.png",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        let png = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jB9kAAAAASUVORK5CYII=").unwrap();
        zip.write_all(&png).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let dir = std::env::temp_dir().join(format!("lumen-word-flow-{}", uuid::Uuid::now_v7()));
        let db = crate::db::Db::init(&dir).await.unwrap();
        let original = crate::content_assets::store_bytes(&db, "orders.docx", bytes.clone())
            .await
            .unwrap();
        let loaded = crate::ai_media::load_flow_media(
            &db,
            crate::ai::Provider::DeepSeek,
            std::slice::from_ref(&original.id),
        )
        .await
        .unwrap();
        assert!(loaded.references.iter().any(|a| a.id == original.id
            && a.data_base64 == original.data_base64
            && a.mime == original.mime));
        assert!(!loaded.warnings.is_empty());
        let media = loaded.media;
        assert_eq!(
            crate::content_assets::decode_asset(
                &crate::content_assets::get_asset(&db, &original.id)
                    .await
                    .unwrap()
            )
            .unwrap(),
            bytes
        );
        let image = media
            .iter()
            .find(|a| a.mime.starts_with("image/"))
            .expect("Word embedded image must be present in generation manifest");
        assert_eq!(crate::content_assets::decode_asset(image).unwrap(), png);
        assert!(finish_flow(
            &db,
            "invalid model output",
            "",
            &loaded.references,
            &loaded.derived,
            &[]
        )
        .await
        .is_err());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM content_assets")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(count, 1, "model errors must not persist prepared images");
        let raw = serde_json::json!({"title":"流程","steps":[{"title":"准备"},{"title":"核对","assetIds":[image.id]}]}).to_string();
        let draft = finish_flow(
            &db,
            &raw,
            &format!("[原始材料](lumen-asset:{})", original.id),
            &loaded.references,
            &loaded.derived,
            &[],
        )
        .await
        .unwrap();
        assert_eq!(
            crate::content_assets::decode_asset(
                &crate::content_assets::get_asset(&db, &image.id)
                    .await
                    .unwrap()
            )
            .unwrap(),
            png
        );
        assert!(!draft.steps[0].detail.contains("lumen-asset:"));
        assert!(draft.steps[1]
            .detail
            .contains(&format!("![{}](lumen-asset:{})", image.name, image.id)));
        assert!(draft.id.is_none());
        assert!(parse_flow(&serde_json::json!({"title":"流程","steps":[{"title":"核对","assetIds":[uuid::Uuid::now_v7().to_string()]}]}).to_string(), "", &media).is_err());
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM memo_documents")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(count.0, 0);
        let native = crate::ai_media::load_flow_media(
            &db,
            crate::ai::Provider::OpenAI,
            std::slice::from_ref(&original.id),
        )
        .await
        .unwrap();
        assert!(native
            .media
            .iter()
            .any(|a| a.id == original.id && a.data_base64 == original.data_base64));
        assert!(native.media.iter().any(|a| a.mime == "image/png"));
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn test_image(id: &str) -> ContentAsset {
        ContentAsset {
            id: id.into(),
            name: "发货表.png".into(),
            mime: "image/png".into(),
            data_base64: String::new(),
            byte_size: 0,
            sha256: String::new(),
            created_at: String::new(),
        }
    }

    #[test]
    fn model_image_ids_are_attached_to_the_corresponding_steps_and_unknown_ids_fail() {
        let id = uuid::Uuid::now_v7().to_string();
        let source = format!("更新发货表：![发货表](lumen-asset:{id})\n通知仓库时再次使用这张表：![发货表](lumen-asset:{id})");
        let raw = serde_json::json!({"title":"发货流程","steps":[
            {"title":"更新发货表","detail":"核对型号和箱数。","assetIds":[id,id]},
            {"title":"通知仓库","detail":"发送核对后的发货表。","assetIds":[id]}
        ]})
        .to_string();
        let draft = parse_flow(&raw, &source, &[test_image(&id)]).unwrap();
        for step in draft.steps {
            assert_eq!(step.detail.matches(&format!("lumen-asset:{id}")).count(), 1);
            assert!(step.detail.contains("!["));
        }
        assert!(draft.body_md.ends_with(&source));
        assert!(parse_flow(&raw, &source, &[]).is_err());
    }

    #[test]
    fn bare_resource_ids_do_not_count_as_a_visible_step_image() {
        let image = test_image(&uuid::Uuid::now_v7().to_string());
        let raw = serde_json::json!({"title":"流程","steps":[{"title":"核对","detail":format!("请看 lumen-asset:{}",image.id),"assetIds":[image.id]}]}).to_string();
        let draft = parse_flow(&raw, "原文", &[image]).unwrap();
        assert!(draft.steps[0].detail.contains("![发货表.png](lumen-asset:"));
    }

    #[test]
    fn owners_are_optional_and_explicit_owners_are_preserved() {
        let draft = parse_flow(
            r#"{"title":"订单处理","steps":[{"title":"核对"},{"title":"复核","owner":"  "},{"title":"通知","owner":" 仓库 "}]}"#,
            "先核对，再通知仓库。",
            &[],
        )
        .unwrap();
        assert_eq!(draft.steps[0].owner, "");
        assert_eq!(draft.steps[1].owner, "");
        assert_eq!(draft.steps[2].owner, "仓库");
    }

    #[test]
    fn generates_new_flow_preserves_sources_without_adding_missing_detail_requirements() {
        let id = uuid::Uuid::now_v7().to_string();
        let source = format!("张三：先核对订单，然后通知仓库。\n![聊天截图](lumen-asset:{id})");
        let draft = parse_flow(r#"```json
{"title":"订单处理","steps":[{"title":"核对订单"},{"title":"通知仓库","owner":"仓库","detail":"发送已确认订单"}]}
```"#, &source, &[test_image(&id)]).unwrap();
        assert!(draft.id.is_none() && draft.expected_revision.is_none());
        assert_eq!(draft.kind, "flow");
        assert!(draft.body_md.ends_with(&source));
        assert_eq!(draft.steps[0].owner, "");
        assert_eq!(draft.steps[0].detail, "");
        assert_ne!(draft.steps[0].id, draft.steps[1].id);
        assert_eq!(draft.steps[1].title, "通知仓库");
    }

    #[test]
    fn rejects_invalid_empty_oversized_or_overwriting_model_output() {
        for raw in [
            "不是 JSON",
            r#"{"title":"流程","steps":[]}"#,
            r#"{"title":"流程","steps":[{"title":""}]}"#,
            r#"{"id":"old-record","title":"流程","steps":[{"title":"操作"}]}"#,
        ] {
            assert!(parse_flow(raw, "原文", &[]).is_err(), "{raw}");
        }
        let steps = vec![serde_json::json!({"title":"操作"}); 101];
        assert!(parse_flow(
            &serde_json::json!({"title":"流程","steps":steps}).to_string(),
            "原文",
            &[]
        )
        .is_err());
        assert!(parse_flow(&serde_json::json!({"title":"流程","steps":[{"title":"操作","detail":"字".repeat(5001)}]}).to_string(), "原文", &[]).is_err());
    }

    #[test]
    fn rejects_unselected_media_but_keeps_selected_image_links() {
        let id = uuid::Uuid::now_v7().to_string();
        let raw = serde_json::json!({"title":"流程","bodyMd":format!("![说明](lumen-asset:{id})"),"steps":[{"title":"操作"}]}).to_string();
        assert!(parse_flow(&raw, "原文", &[]).is_err());
        assert!(parse_flow(&raw, "原文", &[test_image(&id)]).is_ok());
    }
}
