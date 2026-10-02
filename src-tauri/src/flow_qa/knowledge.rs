use super::*;
use crate::{
    content_assets,
    db::Db,
    document_import,
    error::{AppError, AppResult},
    memos::{self, MemoDocument, MemoSummary},
};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Mutex};

fn step_text(step: &memos::FlowStep) -> String {
    let group = step
        .group
        .as_ref()
        .map(|g| format!("章节：{}\n{}\n", g.title, g.path.join(" > ")))
        .unwrap_or_default();
    format!("{group}{}\n{}\n{}", step.title, step.owner, step.detail)
}

pub struct KnowledgeContext {
    documents: Vec<MemoDocument>,
    draft_hash: Option<String>,
    attachments: HashMap<String, (String, document_import::Extraction)>,
    issued: Mutex<Vec<QaCitation>>,
}
impl KnowledgeContext {
    pub async fn load(db: &Db, scope: QaScope) -> AppResult<Self> {
        let mut documents = Vec::new();
        let mut draft_hash = None;
        match scope {
            QaScope::Current { current } => {
                memos::validate(&current)?;
                if current.kind != "flow" {
                    return Err(AppError::validation("当前范围必须是流程"));
                }
                if let Some(id) = &current.id {
                    let saved = memos::get_impl(db, id).await?;
                    if saved.summary.deleted_at.is_some()
                        || saved.summary.kind != "flow"
                        || Some(saved.summary.revision) != current.expected_revision
                    {
                        return Err(AppError::conflict("流程已经更新或删除，请重新读取"));
                    }
                }
                draft_hash = Some(hex::encode(Sha256::digest(
                    serde_json::to_vec(&current).map_err(|_| AppError::internal("草稿摘要失败"))?,
                )));
                documents.push(MemoDocument {
                    summary: MemoSummary {
                        id: current.id.unwrap_or_default(),
                        title: current.title,
                        category: current.category,
                        kind: current.kind,
                        revision: current.expected_revision.unwrap_or(0),
                        created_at: String::new(),
                        updated_at: String::new(),
                        deleted_at: None,
                    },
                    body_md: current.body_md,
                    steps: current.steps,
                });
            }
            QaScope::All => {
                for summary in memos::list_impl(db, "", false).await? {
                    if summary.kind == "flow" {
                        let doc = memos::get_impl(db, &summary.id).await?;
                        if doc.summary.kind == "flow" && doc.summary.deleted_at.is_none() {
                            documents.push(doc);
                        }
                    }
                }
            }
        }
        Ok(Self {
            documents,
            draft_hash,
            attachments: HashMap::new(),
            issued: Mutex::new(Vec::new()),
        })
    }
    fn document(&self, id: Option<&str>) -> AppResult<&MemoDocument> {
        self.documents
            .iter()
            .find(|doc| {
                if doc.summary.id.is_empty() {
                    id.is_none()
                } else {
                    id == Some(doc.summary.id.as_str())
                }
            })
            .ok_or_else(|| AppError::validation("流程不在授权范围内"))
    }
    fn issue(
        &self,
        doc: &MemoDocument,
        step: Option<&str>,
        attachment: Option<&str>,
        locator: String,
        excerpt: String,
    ) -> AppResult<()> {
        if excerpt.is_empty() {
            return Ok(());
        }
        let chars: Vec<char> = excerpt.chars().collect();
        let mut issued = self
            .issued
            .lock()
            .map_err(|_| AppError::internal("证据锁失效"))?;
        for (index, chunk) in chars.chunks(8000).enumerate() {
            let citation = QaCitation {
                id: uuid::Uuid::now_v7().to_string(),
                flow_id: (!doc.summary.id.is_empty()).then(|| doc.summary.id.clone()),
                step_id: step.map(str::to_owned),
                revision: (!doc.summary.id.is_empty()).then_some(doc.summary.revision),
                draft_hash: self.draft_hash.clone(),
                attachment_id: attachment.map(str::to_owned),
                locator: if chars.len() > 8000 {
                    format!(
                        "{locator}；文字 {}–{}",
                        index * 8000 + 1,
                        index * 8000 + chunk.len()
                    )
                } else {
                    locator.clone()
                },
                excerpt: chunk.iter().collect(),
            };
            issued.push(citation);
        }
        Ok(())
    }
    /// Only these generated IDs may be used by a model as citation IDs.
    pub fn citations(&self) -> AppResult<Vec<QaCitation>> {
        Ok(self
            .issued
            .lock()
            .map_err(|_| AppError::internal("证据锁失效"))?
            .clone())
    }
    pub fn search(&self, query: &str, exact: bool, limit: usize) -> AppResult<Vec<QaCandidate>> {
        if limit > 10 || query.chars().count() > 500 || query.contains('\0') {
            return Err(AppError::validation(
                "搜索最多返回 10 项，关键词最多 500 字",
            ));
        }
        let query = query.trim().to_lowercase();
        let mut result = Vec::new();
        for doc in &self.documents {
            let text = format!(
                "{}\n{}\n{}",
                doc.summary.title, doc.summary.category, doc.body_md
            );
            let mut texts = vec![(None, "流程正文".to_owned(), text)];
            for (index, step) in doc.steps.iter().enumerate() {
                let text = format!("步骤 {}：{}", index + 1, step_text(step));
                texts.push((Some(step.id.as_str()), format!("步骤 {}", index + 1), text));
            }
            for (step, locator, text) in texts {
                let Some(positions) = match_positions(&text, &query, exact) else {
                    continue;
                };
                if result.len() == limit {
                    return Ok(result);
                }
                let chars: Vec<char> = text.chars().collect();
                let ranges = evidence_ranges(positions, chars.len());
                let mut excerpts = Vec::new();
                for range in ranges {
                    let excerpt: String = chars[range.clone()].iter().collect();
                    self.issue(
                        doc,
                        step,
                        None,
                        format!("{locator}；文字 {}–{}", range.start + 1, range.end),
                        excerpt.clone(),
                    )?;
                    excerpts.push(excerpt);
                }
                result.push(QaCandidate {
                    flow_id: (!doc.summary.id.is_empty()).then(|| doc.summary.id.clone()),
                    title: doc.summary.title.clone(),
                    category: doc.summary.category.clone(),
                    step_id: step.map(str::to_owned),
                    evidence: excerpts.join("\n…\n"),
                });
            }
        }
        Ok(result)
    }
    pub fn get_flow(&self, flow: Option<&str>, step: Option<&str>) -> AppResult<MemoDocument> {
        let doc = self.document(flow)?;
        let mut output = doc.clone();
        if let Some(id) = step {
            let (index, selected) = doc
                .steps
                .iter()
                .enumerate()
                .find(|(_, s)| s.id == id)
                .ok_or_else(|| AppError::validation("步骤不属于该流程"))?;
            output.steps = vec![selected.clone()];
            output.body_md.clear();
            self.issue(
                doc,
                Some(id),
                None,
                format!("步骤 {}", index + 1),
                step_text(selected),
            )?;
        } else {
            self.issue(
                doc,
                None,
                None,
                "流程正文".into(),
                format!(
                    "{}\n{}\n{}",
                    doc.summary.title, doc.summary.category, doc.body_md
                ),
            )?;
            for (index, s) in doc.steps.iter().enumerate() {
                self.issue(
                    doc,
                    Some(&s.id),
                    None,
                    format!("步骤 {}", index + 1),
                    step_text(s),
                )?;
            }
        }
        Ok(output)
    }
    pub async fn read_attachment(
        &mut self,
        db: &Db,
        id: &str,
        offset: usize,
    ) -> AppResult<AttachmentEvidence> {
        let owners: Vec<(usize, Option<String>)> = self
            .documents
            .iter()
            .enumerate()
            .flat_map(|(index, doc)| {
                let mut owners = Vec::new();
                if references(&doc.body_md, id) {
                    owners.push((index, None));
                }
                for step in &doc.steps {
                    if references(&step.detail, id) {
                        owners.push((index, Some(step.id.clone())));
                    }
                }
                owners
            })
            .collect();
        if owners.is_empty() {
            return Err(AppError::validation("附件不在授权正文引用内"));
        }
        if !self.attachments.contains_key(id) {
            let asset = content_assets::get_asset(db, id).await?;
            let name = asset.name.clone();
            let extracted = document_import::extract_asset_read_only(asset).await?;
            self.attachments.insert(id.into(), (name, extracted));
        }
        let (name, extracted) = &self.attachments[id];
        let len = extracted.text.chars().count();
        if offset > len || (offset == len && len > 0) {
            return Err(AppError::validation("附件分段位置超出内容范围"));
        }
        let excerpt: String = extracted.text.chars().skip(offset).take(8000).collect();
        let end = offset + excerpt.chars().count();
        // Parser text preserves PDF page and spreadsheet headings; character location never invents a page.
        let locator = format!("{name}；提取文字 {}–{}", offset + 1, end);
        let evidence = AttachmentEvidence {
            attachment_id: id.into(),
            locator: locator.clone(),
            excerpt: excerpt.clone(),
            warnings: extracted.warnings.clone(),
            next_offset: (end < len).then_some(end),
        };
        for (index, step) in owners {
            self.issue(
                &self.documents[index],
                step.as_deref(),
                Some(id),
                locator.clone(),
                excerpt.clone(),
            )?;
        }
        Ok(evidence)
    }
    pub async fn validate_citation(&self, db: &Db, citation: &QaCitation) -> AppResult<()> {
        let issued = self.citations()?;
        if !issued
            .iter()
            .any(|item| serde_json::to_value(item).ok() == serde_json::to_value(citation).ok())
        {
            return Err(AppError::validation("引用编号或证据已被伪造"));
        }
        let doc = self.document(citation.flow_id.as_deref())?;
        if citation.draft_hash != self.draft_hash
            || citation.revision != (!doc.summary.id.is_empty()).then_some(doc.summary.revision)
        {
            return Err(AppError::conflict("流程草稿或版本已更新"));
        }
        if let Some(id) = &citation.flow_id {
            let fresh = memos::get_impl(db, id).await?;
            if fresh.summary.deleted_at.is_some()
                || fresh.summary.kind != "flow"
                || Some(fresh.summary.revision) != citation.revision
            {
                return Err(AppError::conflict("引用流程已经更新或删除，请重新检索"));
            }
        }
        Ok(())
    }
}
/// Match normalized characters while retaining every original Unicode character position.
fn match_positions(text: &str, query: &str, exact: bool) -> Option<Vec<std::ops::Range<usize>>> {
    if query.is_empty() {
        return Some(Vec::new());
    }
    let mut normalized = String::new();
    let mut original_indices = Vec::new();
    for (index, ch) in text.chars().enumerate() {
        for lowered in ch.to_lowercase() {
            normalized.push(lowered);
            original_indices.push(index);
        }
    }
    let words: Vec<&str> = if exact {
        vec![query]
    } else {
        query.split_whitespace().collect()
    };
    let normalized_chars: Vec<char> = normalized.chars().collect();
    let mut positions = Vec::new();
    for word in words {
        if let Some(byte) = normalized.find(word) {
            let start = normalized[..byte].chars().count();
            let end = start + word.chars().count();
            positions.push(original_indices[start]..original_indices[end - 1] + 1);
        } else if exact {
            return None;
        } else {
            let mut cursor = 0;
            for wanted in word.chars() {
                let found = normalized_chars[cursor..]
                    .iter()
                    .position(|ch| *ch == wanted)?
                    + cursor;
                positions.push(original_indices[found]..original_indices[found] + 1);
                cursor = found + 1;
            }
        }
    }
    Some(positions)
}

/// Budget context and separators against the 8,000 character cap, including lowercase expansion.
fn evidence_ranges(
    mut positions: Vec<std::ops::Range<usize>>,
    length: usize,
) -> Vec<std::ops::Range<usize>> {
    if positions.is_empty() {
        return std::iter::once(0..length.min(8000)).collect();
    }
    let matched: usize = positions.iter().map(|range| range.len()).sum();
    let separators = positions.len().saturating_sub(1) * 3;
    let radius = (8000_usize.saturating_sub(matched + separators) / (2 * positions.len())).min(4);
    positions.sort_unstable_by_key(|range| range.start);
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    for position in positions {
        let context = position.start.saturating_sub(radius)..(position.end + radius).min(length);
        if let Some(last) = ranges.last_mut() {
            if context.start <= last.end {
                last.end = last.end.max(context.end);
                continue;
            }
        }
        ranges.push(context);
    }
    ranges
}

fn references(text: &str, id: &str) -> bool {
    use pulldown_cmark::{Event, Parser, Tag};
    Parser::new(text).any(|event| match event {
        Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => dest_url
            .strip_prefix("lumen-asset:")
            .is_some_and(|reference| reference == id && uuid::Uuid::parse_str(reference).is_ok()),
        _ => false,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        content_assets,
        memos::{self, FlowStep, SaveMemoInput},
    };
    fn input() -> SaveMemoInput {
        SaveMemoInput {
            id: None,
            expected_revision: None,
            title: "ERP 发货".into(),
            category: "销售".into(),
            kind: "flow".into(),
            body_md: "核对订单后生成发货单".into(),
            steps: vec![
                FlowStep {
                    group: None,
                    layout: None,
                    id: "z".into(),
                    title: "核对".into(),
                    owner: "".into(),
                    detail: "核对订单".into(),
                },
                FlowStep {
                    group: None,
                    layout: None,
                    id: "a".into(),
                    title: "生成发货单".into(),
                    owner: "".into(),
                    detail: "ERP 菜单".into(),
                },
            ],
        }
    }
    async fn db() -> (Db, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("lumen-qa-{}", uuid::Uuid::now_v7()));
        (Db::init(&dir).await.unwrap(), dir)
    }
    async fn cleanup(db: Db, dir: std::path::PathBuf) {
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn grouped_step_search_and_citations_include_original_chapter_context() {
        let (db, dir) = db().await;
        let mut draft = input();
        draft.steps[0].group = Some(crate::memos::FlowGroup {
            id: "source-1".into(),
            title: "原文发货章节".into(),
            path: vec!["数量核对阶段".into()],
        });
        let ctx = KnowledgeContext::load(&db, QaScope::Current { current: draft })
            .await
            .unwrap();
        let found = ctx.search("原文发货章节", true, 10).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].step_id.as_deref(), Some("z"));
        assert!(ctx.search("数量核对阶段", true, 10).unwrap()[0]
            .evidence
            .contains("数量核对阶段"));
        let step = ctx.get_flow(None, Some("z")).unwrap();
        assert_eq!(step.steps[0].group.as_ref().unwrap().title, "原文发货章节");
        cleanup(db, dir).await;
    }
    #[tokio::test]
    async fn current_draft_search_and_order_are_real() {
        let (db, dir) = db().await;
        let ctx = KnowledgeContext::load(&db, QaScope::Current { current: input() })
            .await
            .unwrap();
        assert_eq!(ctx.search("ERP 发货", true, 10).unwrap().len(), 1);
        assert!(ctx.search("核对 发货单", true, 10).unwrap().is_empty());
        assert!(!ctx.search("核对 发货单", false, 10).unwrap().is_empty());
        assert!(ctx.search("", true, 11).is_err());
        let doc = ctx.get_flow(None, None).unwrap();
        assert_eq!(doc.steps[1].id, "a");
        assert!(ctx.get_flow(Some("foreign"), None).is_err());
        assert!(ctx.get_flow(None, Some("foreign")).is_err());
        cleanup(db, dir).await;
    }
    #[tokio::test]
    async fn all_filters_memo_trash_and_empty_db() {
        let (db, dir) = db().await;
        assert!(KnowledgeContext::load(&db, QaScope::All)
            .await
            .unwrap()
            .search("", false, 10)
            .unwrap()
            .is_empty());
        let live = memos::save_impl(&db, input()).await.unwrap();
        let mut memo = input();
        memo.kind = "memo".into();
        memo.steps.clear();
        memos::save_impl(&db, memo).await.unwrap();
        let deleted = memos::save_impl(&db, input()).await.unwrap();
        memos::set_deleted_impl(&db, &deleted.summary.id, deleted.summary.revision, true)
            .await
            .unwrap();
        let ctx = KnowledgeContext::load(&db, QaScope::All).await.unwrap();
        assert_eq!(ctx.search("ERP 发货", true, 10).unwrap().len(), 1);
        assert!(ctx.get_flow(Some(&live.summary.id), None).is_ok());
        assert!(ctx.get_flow(Some(&deleted.summary.id), None).is_err());
        cleanup(db, dir).await;
    }
    #[tokio::test]
    async fn attachments_are_bounded_authorized_and_immutable() {
        let (db, dir) = db().await;
        let bytes = "中".repeat(8001).into_bytes();
        let asset = content_assets::store_bytes(&db, "original.txt", bytes.clone())
            .await
            .unwrap();
        let hidden = content_assets::store_bytes(&db, "secret.txt", b"secret".to_vec())
            .await
            .unwrap();
        let mut draft = input();
        draft.body_md = format!("[原件](lumen-asset:{})", asset.id);
        let mut ctx = KnowledgeContext::load(&db, QaScope::Current { current: draft })
            .await
            .unwrap();
        assert!(ctx.read_attachment(&db, &hidden.id, 0).await.is_err());
        assert!(ctx.read_attachment(&db, "C:/secret.txt", 0).await.is_err());
        let first = ctx.read_attachment(&db, &asset.id, 0).await.unwrap();
        assert_eq!(first.excerpt.chars().count(), 8000);
        assert_eq!(first.next_offset, Some(8000));
        assert_eq!(
            ctx.read_attachment(&db, &asset.id, 8000)
                .await
                .unwrap()
                .excerpt,
            "中"
        );
        assert!(ctx.read_attachment(&db, &asset.id, 8002).await.is_err());
        assert_eq!(
            content_assets::decode_asset(&content_assets::get_asset(&db, &asset.id).await.unwrap())
                .unwrap(),
            bytes
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM content_assets")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(count, 2);
        cleanup(db, dir).await;
    }
    #[tokio::test]
    async fn forged_citation_is_rejected() {
        let (db, dir) = db().await;
        let ctx = KnowledgeContext::load(&db, QaScope::Current { current: input() })
            .await
            .unwrap();
        let fake = QaCitation {
            id: "fake".into(),
            flow_id: None,
            step_id: None,
            revision: None,
            draft_hash: None,
            attachment_id: None,
            locator: "正文".into(),
            excerpt: "核对订单后生成发货单".into(),
        };
        assert!(ctx.validate_citation(&db, &fake).await.is_err());
        cleanup(db, dir).await;
    }
}

#[cfg(test)]
mod citation_tests {
    use super::*;
    use crate::memos::{FlowStep, SaveMemoInput};
    fn draft() -> SaveMemoInput {
        SaveMemoInput {
            id: None,
            expected_revision: None,
            title: "交付".into(),
            category: "".into(),
            kind: "flow".into(),
            body_md: "已授权操作".into(),
            steps: vec![FlowStep {
                group: None,
                layout: None,
                id: "s".into(),
                title: "检查".into(),
                owner: "".into(),
                detail: "核对".into(),
            }],
        }
    }
    #[tokio::test]
    async fn citations_require_issued_id_exact_fields_live_revision_and_draft_hash() {
        let dir = std::env::temp_dir().join(format!("lumen-qa-citation-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let saved = memos::save_impl(&db, draft()).await.unwrap();
        let ctx = KnowledgeContext::load(&db, QaScope::All).await.unwrap();
        ctx.get_flow(Some(&saved.summary.id), None).unwrap();
        let source = ctx.citations().unwrap().remove(0);
        ctx.validate_citation(&db, &source).await.unwrap();
        for field in [
            "id",
            "revision",
            "excerpt",
            "locator",
            "stepId",
            "attachmentId",
        ] {
            let mut json = serde_json::to_value(&source).unwrap();
            json[field] = if field == "revision" {
                serde_json::json!(99)
            } else {
                serde_json::json!("forged")
            };
            let fake: QaCitation = serde_json::from_value(json).unwrap();
            assert!(ctx.validate_citation(&db, &fake).await.is_err(), "{field}");
        }
        let mut edit = draft();
        edit.id = Some(saved.summary.id.clone());
        edit.expected_revision = Some(saved.summary.revision);
        edit.body_md = "更改".into();
        let updated = memos::save_impl(&db, edit).await.unwrap();
        assert!(ctx.validate_citation(&db, &source).await.is_err());
        let ctx2 = KnowledgeContext::load(&db, QaScope::All).await.unwrap();
        ctx2.get_flow(Some(&saved.summary.id), None).unwrap();
        let source2 = ctx2.citations().unwrap().remove(0);
        memos::set_deleted_impl(&db, &saved.summary.id, updated.summary.revision, true)
            .await
            .unwrap();
        assert!(ctx2.validate_citation(&db, &source2).await.is_err());
        let current = KnowledgeContext::load(&db, QaScope::Current { current: draft() })
            .await
            .unwrap();
        current.get_flow(None, None).unwrap();
        let mut source = current.citations().unwrap().remove(0);
        assert!(source.draft_hash.is_some());
        current.validate_citation(&db, &source).await.unwrap();
        source.draft_hash = Some("changed".into());
        assert!(current.validate_citation(&db, &source).await.is_err());
        let mut changed = draft();
        changed.body_md = "changed".into();
        let rebuilt = KnowledgeContext::load(&db, QaScope::Current { current: changed })
            .await
            .unwrap();
        assert!(rebuilt.validate_citation(&db, &source).await.is_err());
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn office_embedded_images_never_create_business_rows() {
        use std::io::{Cursor, Write};
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        archive
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(b"<w:document xmlns:w='urn:word'><w:p><w:r><w:t>Order 42</w:t></w:r></w:p></w:document>").unwrap();
        archive
            .start_file(
                "word/media/image1.png",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
        let bytes = archive.finish().unwrap().into_inner();
        let dir = std::env::temp_dir().join(format!("lumen-qa-office-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let asset = content_assets::store_bytes(&db, "order.docx", bytes.clone())
            .await
            .unwrap();
        let mut current = draft();
        current.steps[0].detail = format!("[附件](lumen-asset:{})", asset.id);
        let saved = memos::save_impl(&db, current).await.unwrap();
        let mut ctx = KnowledgeContext::load(&db, QaScope::All).await.unwrap();
        assert!(ctx
            .read_attachment(&db, &asset.id, 0)
            .await
            .unwrap()
            .excerpt
            .contains("Order 42"));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM content_assets")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            content_assets::decode_asset(&content_assets::get_asset(&db, &asset.id).await.unwrap())
                .unwrap(),
            bytes
        );
        assert_eq!(
            memos::get_impl(&db, &saved.summary.id)
                .await
                .unwrap()
                .summary
                .revision,
            saved.summary.revision
        );
        let citation = ctx.citations().unwrap().remove(0);
        assert_eq!(citation.step_id.as_deref(), Some("s"));
        ctx.validate_citation(&db, &citation).await.unwrap();
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn scanned_image_ocr_warning_survives_evidence() {
        let (bytes, _, _, _) = document_import::tests::printed_fixture();
        let dir = std::env::temp_dir().join(format!("lumen-qa-ocr-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let asset = content_assets::store_bytes(&db, "scan.png", bytes)
            .await
            .unwrap();
        let mut current = draft();
        current.body_md = format!("[扫描件](lumen-asset:{})", asset.id);
        let mut ctx = KnowledgeContext::load(&db, QaScope::Current { current })
            .await
            .unwrap();
        let evidence = ctx.read_attachment(&db, &asset.id, 0).await.unwrap();
        assert!(evidence.excerpt.contains("ORDER"));
        assert!(evidence.warnings.iter().any(|w| w.contains("OCR")));
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[cfg(test)]
mod boundary_tests {
    use super::*;
    #[tokio::test]
    async fn long_flow_citations_are_bounded_and_bare_attachment_ids_are_not_authority() {
        let dir = std::env::temp_dir().join(format!("lumen-qa-bounds-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let asset = content_assets::store_bytes(&db, "file.txt", b"private".to_vec())
            .await
            .unwrap();
        let draft = crate::memos::SaveMemoInput {
            id: None,
            expected_revision: None,
            title: "long".into(),
            category: "".into(),
            kind: "flow".into(),
            body_md: format!(
                "{} 实际操作关键词 lumen-asset:{}",
                "中".repeat(9000),
                asset.id
            ),
            steps: vec![],
        };
        let mut ctx = KnowledgeContext::load(&db, QaScope::Current { current: draft })
            .await
            .unwrap();
        ctx.get_flow(None, None).unwrap();
        assert!(ctx.search("实际操作关键词", true, 1).unwrap()[0]
            .evidence
            .contains("实际操作关键词"));
        assert!(ctx
            .citations()
            .unwrap()
            .iter()
            .all(|c| c.excerpt.chars().count() <= 8000));
        assert!(ctx.read_attachment(&db, &asset.id, 0).await.is_err());
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod review_regressions {
    use super::*;
    fn draft(body: String) -> crate::memos::SaveMemoInput {
        crate::memos::SaveMemoInput {
            id: None,
            expected_revision: None,
            title: "交付".into(),
            category: "".into(),
            kind: "flow".into(),
            body_md: body,
            steps: vec![],
        }
    }
    async fn setup() -> (Db, std::path::PathBuf, crate::content_assets::ContentAsset) {
        let dir = std::env::temp_dir().join(format!("lumen-qa-review-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let asset = content_assets::store_bytes(&db, "order.txt", b"ORDER 42".to_vec())
            .await
            .unwrap();
        (db, dir, asset)
    }
    #[tokio::test]
    async fn literal_markdown_examples_do_not_grant_attachment_access() {
        let (db, dir, asset) = setup().await;
        let link = format!("[file](lumen-asset:{})", asset.id);
        for text in [
            format!("`{link}`"),
            format!("```md\n{link}\n```"),
            format!("    {link}"),
            format!("\\{link}"),
            format!("<!-- {link} -->"),
            format!("[unused]: lumen-asset:{}", asset.id),
            format!("[file](lumen-asset:{}-extra)", asset.id),
        ] {
            let mut ctx = KnowledgeContext::load(
                &db,
                QaScope::Current {
                    current: draft(text.clone()),
                },
            )
            .await
            .unwrap();
            assert!(
                ctx.read_attachment(&db, &asset.id, 0).await.is_err(),
                "must reject literal {text}"
            );
        }
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn real_markdown_destination_variants_grant_attachment_access() {
        let (db, dir, asset) = setup().await;
        for text in [
            format!("[file](<lumen-asset:{}>)", asset.id),
            format!("[file](lumen-asset:{} \"Order title\")", asset.id),
            format!("![scan](<lumen-asset:{}> 'title')", asset.id),
            format!(
                "[file][asset]\n\n[asset]: lumen-asset:{} \"title\"",
                asset.id
            ),
            format!("[file](lumen-asset:{})", asset.id),
        ] {
            let mut ctx = KnowledgeContext::load(
                &db,
                QaScope::Current {
                    current: draft(text.clone()),
                },
            )
            .await
            .unwrap();
            assert_eq!(
                ctx.read_attachment(&db, &asset.id, 0)
                    .await
                    .unwrap_or_else(|e| panic!("{text}: {e}"))
                    .excerpt,
                "ORDER 42"
            );
        }
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn fuzzy_subsequence_after_long_prefix_keeps_all_matching_evidence() {
        let (db, dir, _) = setup().await;
        let ctx = KnowledgeContext::load(
            &db,
            QaScope::Current {
                current: draft(format!("{}核……对……发……货", "中".repeat(9000))),
            },
        )
        .await
        .unwrap();
        let candidates = ctx.search("核对发货", false, 10).unwrap();
        assert_eq!(candidates.len(), 1);
        for ch in ['核', '对', '发', '货'] {
            assert!(candidates[0].evidence.contains(ch), "missing {ch}");
            assert!(ctx
                .citations()
                .unwrap()
                .iter()
                .any(|c| c.excerpt.contains(ch)));
        }
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn distant_multiword_fuzzy_hits_have_bounded_separate_sources() {
        let (db, dir, _) = setup().await;
        let ctx = KnowledgeContext::load(
            &db,
            QaScope::Current {
                current: draft(format!("核对{}发货", "中".repeat(9000))),
            },
        )
        .await
        .unwrap();
        let candidate = ctx.search("核对 发货", false, 1).unwrap().remove(0);
        assert!(candidate.evidence.contains("核对"));
        assert!(candidate.evidence.contains("发货"));
        assert!(candidate.evidence.chars().count() <= 8000);
        let sources = ctx.citations().unwrap();
        assert!(sources.iter().any(|c| c.excerpt.contains("核对")));
        assert!(sources.iter().any(|c| c.excerpt.contains("发货")));
        for source in sources {
            ctx.validate_citation(&db, &source).await.unwrap();
            assert!(source.excerpt.chars().count() <= 8000);
        }
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn step_search_source_uses_real_one_based_index() {
        let (db, dir, _) = setup().await;
        let mut current = draft("".into());
        current.steps = vec![
            crate::memos::FlowStep {
                group: None,
                layout: None,
                id: "z".into(),
                title: "核对".into(),
                owner: "".into(),
                detail: "".into(),
            },
            crate::memos::FlowStep {
                group: None,
                layout: None,
                id: "a".into(),
                title: "发货".into(),
                owner: "".into(),
                detail: "".into(),
            },
        ];
        let ctx = KnowledgeContext::load(&db, QaScope::Current { current })
            .await
            .unwrap();
        ctx.search("发货", true, 1).unwrap();
        assert!(ctx.citations().unwrap()[0].locator.starts_with("步骤 2"));
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[cfg(test)]
mod unicode_evidence_regression {
    use super::*;
    #[tokio::test]
    async fn lowercase_expansion_of_maximum_query_still_bounds_evidence() {
        let dir = std::env::temp_dir().join(format!("lumen-qa-unicode-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let body = format!("i{}\u{0307}{}", "中".repeat(12), "中".repeat(12)).repeat(500);
        let draft = crate::memos::SaveMemoInput {
            id: None,
            expected_revision: None,
            title: "交付".into(),
            category: "".into(),
            kind: "flow".into(),
            body_md: body,
            steps: vec![],
        };
        let ctx = KnowledgeContext::load(&db, QaScope::Current { current: draft })
            .await
            .unwrap();
        let candidate = ctx.search(&"İ".repeat(500), false, 1).unwrap().remove(0);
        assert!(
            candidate.evidence.chars().count() <= 8000,
            "{}",
            candidate.evidence.chars().count()
        );
        assert!(candidate.evidence.contains('i') && candidate.evidence.contains('\u{0307}'));
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
}
