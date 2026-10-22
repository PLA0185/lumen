use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SceneMatchMode {
    Image,
    Ocr,
    ImageAndOcr,
}

impl SceneMatchMode {
    pub fn as_db_key(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Ocr => "ocr",
            Self::ImageAndOcr => "image_and_ocr",
        }
    }

    pub fn needs_image(self) -> bool {
        matches!(self, Self::Image | Self::ImageAndOcr)
    }

    pub fn needs_ocr(self) -> bool {
        matches!(self, Self::Ocr | Self::ImageAndOcr)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateConfig {
    /// Normalized rectangle inside the captured window, each value in 0..=1.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub source_width: u32,
    pub source_height: u32,
    pub threshold: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneRuleInput {
    pub id: Option<String>,
    pub expected_revision: Option<i64>,
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub program_allowlist: Vec<String>,
    pub window_title_contains: Vec<String>,
    pub match_mode: SceneMatchMode,
    pub template_config: Option<TemplateConfig>,
    /// PNG/JPEG bytes from the one-shot recording flow; never accepts a path.
    pub template_image_base64: Option<String>,
    pub existing_template_id: Option<String>,
    pub required_terms: Vec<String>,
    pub excluded_terms: Vec<String>,
    pub headline: String,
    pub body_md: String,
    pub memo_id: Option<String>,
    pub step_id: Option<String>,
    pub stable_hits: i64,
    pub stable_leave_ms: i64,
    pub default_snooze_minutes: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub program_allowlist: Vec<String>,
    pub window_title_contains: Vec<String>,
    pub match_mode: SceneMatchMode,
    pub template_id: Option<String>,
    pub template_config: Option<TemplateConfig>,
    pub required_terms: Vec<String>,
    pub excluded_terms: Vec<String>,
    pub headline: String,
    pub body_md: String,
    pub memo_id: Option<String>,
    pub step_id: Option<String>,
    pub stable_hits: i64,
    pub stable_leave_ms: i64,
    pub default_snooze_minutes: i64,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneMonitorStatus {
    pub platform_supported: bool,
    pub enabled: bool,
    pub paused: bool,
    pub capture_armed: bool,
    pub allowed_rule_count: usize,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureFramePayload {
    pub capture_id: String,
    pub png_base64: String,
    pub process_name: String,
    pub window_title: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneTestResult {
    pub rule_id: Option<String>,
    pub passed: bool,
    pub window_allowed: bool,
    pub image_score: Option<f32>,
    pub image_threshold: Option<f32>,
    pub ocr_conditions_met: Option<bool>,
    pub reason_code: String,
    pub explanation: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneNoteItem {
    pub rule_id: String,
    pub headline: String,
    pub body_md: String,
    pub memo_id: Option<String>,
    pub step_id: Option<String>,
    pub snoozed_until: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneEvent {
    pub id: String,
    pub rule_id: Option<String>,
    pub occurred_at: String,
    pub event_type: String,
    pub result_code: String,
    pub image_score: Option<f64>,
    pub elapsed_ms: i64,
}
