pub mod knowledge;
use crate::memos::SaveMemoInput;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum QaScope {
    Current { current: SaveMemoInput },
    All,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QaCitation {
    pub id: String,
    pub flow_id: Option<String>,
    pub step_id: Option<String>,
    pub revision: Option<i64>,
    pub draft_hash: Option<String>,
    pub attachment_id: Option<String>,
    pub locator: String,
    pub excerpt: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaCandidate {
    pub flow_id: Option<String>,
    pub title: String,
    pub category: String,
    pub step_id: Option<String>,
    pub evidence: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentEvidence {
    pub attachment_id: String,
    pub locator: String,
    pub excerpt: String,
    pub warnings: Vec<String>,
    pub next_offset: Option<usize>,
}
