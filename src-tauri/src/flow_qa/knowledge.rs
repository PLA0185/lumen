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
        let matches = |text: &str| {
            let text = text.to_lowercase();
            if exact {
                text.contains(&query)
            } else {
                query.split_whitespace().all(|word| {
                    if text.contains(word) {
                        true
                    } else {
                        let mut chars = text.chars();
                        word.chars()
                            .all(|needle| chars.by_ref().any(|ch| ch == needle))
                    }
                })
            }
        };
        let mut result = Vec::new();
        for doc in &self.documents {
            let text = format!(
                "{}\n{}\n{}",
                doc.summary.title, doc.summary.category, doc.body_md
            );
            let mut hits: Vec<(Option<&str>, String)> = Vec::new();
            if matches(&text) {
                hits.push((None, text));
            }
            for (index, step) in doc.steps.iter().enumerate() {
                let text = format!(
                    "步骤 {}：{}\n{}\n{}",
                    index + 1,
                    step.title,
                    step.owner,
                    step.detail
                );
                if matches(&text) {
                    hits.push((Some(&step.id), text));
                }
            }
            for (step, text) in hits {
                if result.len() == limit {
                    return Ok(result);
                }
                let normalized = text.to_lowercase();
                let match_at = if exact {
                    normalized.find(&query)
                } else {
                    query
                        .split_whitespace()
                        .filter_map(|word| normalized.find(word))
                        .min()
                };
                let normalized_offset = match_at
                    .map(|byte| normalized[..byte].chars().count())
                    .unwrap_or(0);
                let mut lowered_chars = 0;
                let original_offset = text
                    .chars()
                    .take_while(|ch| {
                        let before = lowered_chars;
                        lowered_chars += ch.to_lowercase().count();
                        before < normalized_offset
                    })
                    .count();
                let evidence: String = text
                    .chars()
                    .skip(original_offset.saturating_sub(200))
                    .take(8000)
                    .collect();
                self.issue(
                    doc,
                    step,
                    None,
                    step.map(|id| format!("步骤 {id}"))
                        .unwrap_or_else(|| "流程正文".into()),
                    evidence.clone(),
                )?;
                result.push(QaCandidate {
                    flow_id: (!doc.summary.id.is_empty()).then(|| doc.summary.id.clone()),
                    title: doc.summary.title.clone(),
                    category: doc.summary.category.clone(),
                    step_id: step.map(str::to_owned),
                    evidence,
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
                format!(
                    "{}\n{}\n{}",
                    selected.title, selected.owner, selected.detail
                ),
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
                    format!("{}\n{}\n{}", s.title, s.owner, s.detail),
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
fn references(text: &str, id: &str) -> bool {
    // Match actual markdown URLs, never a bare ID or arbitrary filesystem path.
    regex::Regex::new(r"!?\[[^\]\n]*\]\(lumen-asset:([0-9a-fA-F-]{36})\)")
        .expect("constant regex")
        .captures_iter(text)
        .any(|m| m[1] == *id && uuid::Uuid::parse_str(&m[1]).is_ok())
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
                    id: "z".into(),
                    title: "核对".into(),
                    owner: "".into(),
                    detail: "核对订单".into(),
                },
                FlowStep {
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
