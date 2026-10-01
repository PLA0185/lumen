//! AI 功能层（任务书 §6）。
//!
//! ## 本模块的核心约束
//!
//! > 所有 AI 输出先显示差异预览，用户选择接受、修改或放弃后才写入数据库。
//! > AI 不得擅自删除、完成或改写用户任务。
//!
//! 落实方式：所有会改动数据的 AI 能力都遵循同一套三段式流程：
//!
//! ```text
//! generate  →  返回 DiffPreview（只读，不碰数据库）
//! validate  →  结构校验 / 日期时区校验 / 任务 ID 核验 / 重复项检测
//! apply     →  用户带着 preview_id 明确确认后才写入
//! ```
//!
//! 关键设计：
//! - **预览不落库**：`generate` 阶段只读取必要数据并返回差异，数据库零改动。
//! - **确认需回传预览 id**：`apply` 必须带上 generate 时给出的 id，
//!   因此不可能出现"没看过预览就写入"的情况。
//! - **校验不合格就拒绝**：无效输出返回可读错误并允许重新生成，
//!   **绝不**"尽力而为"地写一半进去。
//! - **发送范围可控**：默认只发送标题、截止时间、优先级等必要字段；
//!   备注正文（可能含敏感信息）默认不发送（§6 明确要求）。
//!
//! ## 预览的生命周期
//!
//! 预览保存在内存中（`AppState` 不持有，改用一个进程内的注册表），
//! 带过期时间。这样即使用户放着不管也不会永久占用内存，
//! 且程序重启后旧预览自然失效——用户必须重新生成，避免"确认一份过期数据"。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::ai::{self, ChatMessage, ChatRequest, ProviderConfig};
use crate::commands::AppState;
use crate::error::{AppError, AppResult};

/// 预览有效期。过期后必须重新生成（避免用户确认一份陈旧的数据）
const PREVIEW_TTL: Duration = Duration::from_secs(30 * 60);

/// 单次可生成的最大条目数，防止模型返回超长列表把界面撑爆
const MAX_ITEMS: usize = 50;

/// 备注正文是否默认发送给模型。
///
/// §6 要求"敏感备注、附件内容默认不发送"，附件内容从未发送，
/// 备注也默认不发；用户可在设置中显式开启。
pub const DEFAULT_SEND_NOTES: bool = false;

// =============================================================================
// 差异预览的数据结构
// =============================================================================

/// 一条差异项的动作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffAction {
    /// 新增任务
    Create,
    /// 修改已有任务的字段
    Update,
    /// 修改排程（只改计划时间）
    Reschedule,
}

/// 单个字段的改动
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldChange {
    /// 字段名（用界面能显示的中文标签也在 `label` 里给出）
    pub field: String,
    pub label: String,
    /// 改动前（新增时为 None）
    pub before: Option<String>,
    /// 改动后
    pub after: Option<String>,
}

/// 一条差异项
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffItem {
    /// 动作
    pub action: DiffAction,
    /// 目标任务 ID（新增时为 None）
    pub task_id: Option<String>,
    /// 目标任务标题（新增时是拟创建的标题）
    pub title: String,
    /// 字段级改动列表（界面逐条展示，用户可据此判断）
    pub changes: Vec<FieldChange>,
    /// 该条要写入的完整字段（`apply` 时使用）
    #[serde(skip_serializing)]
    pub payload: serde_json::Value,
    /// 备注：模型给出的理由或需要用户注意的点
    #[serde(default)]
    pub note: Option<String>,
}

/// 校验发现的问题
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    /// 严重级别：error 会阻止整体写入；warning 只提示
    pub level: String,
    /// 涉及第几条（从 1 开始；0 表示整体问题）
    pub index: usize,
    pub message: String,
}

/// 差异预览（返回给界面，**不改动数据库**）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffPreview {
    /// 确认写入时必须回传这个 id。
    /// Rust 侧用 snake_case，序列化给前端时由 serde 转成 `previewId`。
    pub preview_id: String,
    /// 能力名称（界面用于显示"AI 整理"之类的标题）
    pub capability: String,
    /// 人类可读的概要
    pub summary: String,
    /// 差异项
    pub items: Vec<DiffItem>,
    /// 校验问题（error 级存在时界面应禁用"接受"按钮）
    pub issues: Vec<ValidationIssue>,
    /// 是否可接受（无 error 级问题且至少有一条差异）
    pub acceptable: bool,
    /// 模型原始返回的正文（供"查看原始输出"用，便于排查）
    pub raw: String,
    /// token 用量
    pub usage: Option<ai::TokenUsage>,
    /// 数据发送范围说明（§6 要求说明发送给模型的数据范围）
    pub data_scope_note: String,
}

/// 应用结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    /// 实际创建的任务数
    pub created: usize,
    /// 实际修改的任务数
    pub updated: usize,
    /// 被跳过的条数（用户在预览里取消勾选）
    pub skipped: usize,
    pub warnings: Vec<String>,
}

// =============================================================================
// 预览注册表（进程内、带过期）
// =============================================================================

struct Registry {
    map: HashMap<String, (Instant, PendingPreview)>,
}

/// 待确认的预览内容（与 DiffPreview 相比多了 apply 所需的数据）
struct PendingPreview {
    capability: String,
    items: Vec<DiffItem>,
}

static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();

fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| {
        Mutex::new(Registry {
            map: HashMap::new(),
        })
    })
}

/// 登记一份预览，返回 id
fn register(capability: &str, items: Vec<DiffItem>) -> String {
    let id = uuid::Uuid::now_v7().to_string();
    let now = Instant::now();
    if let Ok(mut r) = registry().lock() {
        // 顺手清理过期项，避免长期运行后无限增长
        r.map
            .retain(|_, (t, _)| now.duration_since(*t) < PREVIEW_TTL);
        r.map.insert(
            id.clone(),
            (
                now,
                PendingPreview {
                    capability: capability.to_string(),
                    items,
                },
            ),
        );
    }
    id
}

/// 取出并移除预览（**一次性**：确认后即失效，避免重复写入）
fn take(id: &str) -> AppResult<(String, Vec<DiffItem>)> {
    let mut r = registry()
        .lock()
        .map_err(|_| AppError::internal("预览注册表不可用"))?;
    match r.map.remove(id) {
        Some((t, p)) => {
            if Instant::now().duration_since(t) >= PREVIEW_TTL {
                return Err(AppError::conflict("这份预览已过期")
                    .with_hint("为避免确认一份陈旧的数据，请重新生成"));
            }
            Ok((p.capability, p.items))
        }
        None => Err(AppError::not_found("预览", id)
            .with_hint("预览可能已过期、已被应用，或程序重启过。请重新生成后再确认")),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewEdit {
    pub index: usize,
    pub field: String,
    pub value: Option<String>,
}

fn edit_items(items: &mut [DiffItem], edits: Vec<PreviewEdit>) -> AppResult<()> {
    for edit in edits {
        let item = items
            .get_mut(edit.index)
            .ok_or_else(|| AppError::validation("预览条目不存在"))?;
        let is_subtask = item
            .payload
            .get("asSubtask")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if (item.action != DiffAction::Create && edit.field != "plannedAt")
            || (is_subtask && edit.field != "title")
        {
            return Err(AppError::validation("此条目不允许修改该字段"));
        }
        let raw = edit.value.as_deref().unwrap_or("").trim();
        let (value, label, display) = match edit.field.as_str() {
            "title" => {
                if raw.is_empty() || raw.chars().count() > 500 {
                    return Err(AppError::validation("标题必须为 1–500 字"));
                }
                item.title = raw.to_string();
                (serde_json::json!(raw), "标题", Some(raw.to_string()))
            }
            "plannedAt" | "dueAt" => {
                if item.payload.get("rrule").is_some_and(|v| !v.is_null()) {
                    return Err(AppError::validation(
                        "重复任务请在创建后用“修改重复规则”调整首次日期和规则",
                    ));
                }
                let label = if edit.field == "plannedAt" {
                    "计划时间"
                } else {
                    "截止时间"
                };
                let (date, has_time) = normalize_model_datetime(label, raw)?;
                if item.action != DiffAction::Create && date.is_none() {
                    return Err(AppError::validation("排程日期不能为空"));
                }
                item.payload[if edit.field == "plannedAt" {
                    "hasPlannedTime"
                } else {
                    "hasDueTime"
                }] = serde_json::json!(has_time);
                let display = date
                    .as_ref()
                    .map(|d| format!("{d}{}", if has_time { "" } else { "（仅日期）" }));
                (serde_json::json!(date), label, display)
            }
            "priority" => {
                let p = raw
                    .parse::<i64>()
                    .map_err(|_| AppError::validation("优先级应为 0–3"))?;
                if !(0..=3).contains(&p) {
                    return Err(AppError::validation("优先级应为 0–3"));
                }
                (
                    serde_json::json!(p),
                    "优先级",
                    Some(["无", "低", "中", "高"][p as usize].to_string()),
                )
            }
            _ => return Err(AppError::validation("不能编辑此字段")),
        };
        item.payload[&edit.field] = value;
        if let Some(change) = item.changes.iter_mut().find(|c| c.field == edit.field) {
            change.after = display;
        } else {
            item.changes.push(FieldChange {
                field: edit.field,
                label: label.into(),
                before: None,
                after: display,
            });
        }
    }
    Ok(())
}

/// 用户调整预览：只改内存候选，原始动作、目标 ID 与附件均不可改。
#[tauri::command]
pub fn ai_preview_edit(preview_id: String, edits: Vec<PreviewEdit>) -> AppResult<Vec<DiffItem>> {
    let mut r = registry()
        .lock()
        .map_err(|_| AppError::internal("预览注册表不可用"))?;
    let (time, pending) = r
        .map
        .get_mut(&preview_id)
        .ok_or_else(|| AppError::not_found("预览", &preview_id))?;
    if time.elapsed() >= PREVIEW_TTL {
        return Err(AppError::conflict("预览已过期，请重新生成"));
    }
    let mut items = pending.items.clone();
    edit_items(&mut items, edits)?;
    pending.items = items.clone();
    Ok(items)
}

// =============================================================================
// JSON 解析与校验
// =============================================================================

/// 从模型返回的文本里提取 JSON。
///
/// 即使要求了 JSON 输出，模型仍可能包上 ```json 代码块或前后加解释文字
/// （DeepSeek 的 json_object 模式尤其如此，官方也提示需要自行在 prompt 中
/// 强调 json）。因此这里做一次宽松提取，而不是直接 `from_str` 失败。
pub(crate) fn extract_json(text: &str) -> AppResult<serde_json::Value> {
    let t = text.trim();

    // 1) 直接就是 JSON
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
        return Ok(v);
    }

    // 2) 去掉 ```json ... ``` 围栏
    let unfenced = t
        .trim_start_matches("```json")
        .trim_start_matches("```JSON")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(unfenced) {
        return Ok(v);
    }

    // 3) 取第一段平衡的 {...} 或 [...]
    for (open, close) in [('{', '}'), ('[', ']')] {
        if let Some(start) = t.find(open) {
            let mut depth = 0i32;
            let mut in_str = false;
            let mut escaped = false;
            for (i, ch) in t[start..].char_indices() {
                if escaped {
                    escaped = false;
                    continue;
                }
                match ch {
                    '\\' if in_str => escaped = true,
                    '"' => in_str = !in_str,
                    c if c == open && !in_str => depth += 1,
                    c if c == close && !in_str => {
                        depth -= 1;
                        if depth == 0 {
                            let candidate = &t[start..start + i + ch.len_utf8()];
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(candidate) {
                                return Ok(v);
                            }
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Err(AppError::validation("模型返回的内容不是有效的 JSON")
        .with_hint("可点「重新生成」，或改用能力更强的模型（较小的模型更容易返回非 JSON 文本）"))
}

/// 校验并把一个日期字符串规范化为 UTC ISO-8601。
///
/// §6 要求"日期与时区校验"：模型经常返回 `2026-09-25` 这样的纯日期，
/// 或相对描述（"明天"）。纯日期按**当天本地零点**解释并明确标注为"仅日期"，
/// 而不是当成凌晨到期（§4.3 明确禁止把全天任务当作凌晨到期）。
fn normalize_model_datetime(field: &str, raw: &str) -> AppResult<(Option<String>, bool)> {
    let s = raw.trim();
    // 模型返回"空值"的写法不统一：空串、null、NULL、"None" 都出现过。
    // 大小写敏感地只认 "null" 会让大写形式被当成非法日期而报错。
    if s.is_empty() || s.eq_ignore_ascii_case("null") || s.eq_ignore_ascii_case("none") || s == "-"
    {
        return Ok((None, false));
    }

    // 带时区的完整时间
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok((
            Some(crate::db::to_db_time(dt.with_timezone(&chrono::Utc))),
            true,
        ));
    }

    // 纯日期 `YYYY-MM-DD` → 本地当天零点，标记为"仅日期"
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        use chrono::TimeZone;
        let local = d
            .and_hms_opt(0, 0, 0)
            .and_then(|ndt| chrono::Local.from_local_datetime(&ndt).single())
            .ok_or_else(|| AppError::validation(format!("{field}无法转换为本地时间：{s}")))?;
        return Ok((
            Some(crate::db::to_db_time(local.with_timezone(&chrono::Utc))),
            false,
        ));
    }

    // 本地日期时间（无时区）
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        use chrono::TimeZone;
        let local = chrono::Local
            .from_local_datetime(&ndt)
            .single()
            .ok_or_else(|| AppError::validation(format!("{field}无法转换为本地时间：{s}")))?;
        return Ok((
            Some(crate::db::to_db_time(local.with_timezone(&chrono::Utc))),
            true,
        ));
    }

    Err(
        AppError::validation(format!("{field}的日期格式无法识别：{s}"))
            .with_hint("AI 应返回 YYYY-MM-DD 或带时区的 ISO-8601。可重新生成，或手动修正该条"),
    )
}

/// 校验一条 `create` 条目，产出 DiffItem
fn build_create_item(
    idx: usize,
    obj: &serde_json::Map<String, serde_json::Value>,
    issues: &mut Vec<ValidationIssue>,
) -> Option<DiffItem> {
    let title = obj
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    if title.is_empty() {
        issues.push(ValidationIssue {
            level: "error".into(),
            index: idx + 1,
            message: "该条缺少标题，无法创建".into(),
        });
        return None;
    }
    if title.chars().count() > 500 {
        issues.push(ValidationIssue {
            level: "error".into(),
            index: idx + 1,
            message: format!("标题过长（{} 字符），上限 500", title.chars().count()),
        });
        return None;
    }

    let mut changes = vec![FieldChange {
        field: "title".into(),
        label: "标题".into(),
        before: None,
        after: Some(title.clone()),
    }];

    // 截止时间
    let mut due_at: Option<String> = None;
    let mut has_due_time = false;
    if let Some(raw) = obj.get("dueAt").and_then(|v| v.as_str()) {
        match normalize_model_datetime("截止时间", raw) {
            Ok((dt, h)) => {
                due_at = dt.clone();
                has_due_time = h;
                if let Some(d) = dt {
                    changes.push(FieldChange {
                        field: "dueAt".into(),
                        label: "截止时间".into(),
                        before: None,
                        after: Some(format!("{d}{}", if h { "" } else { "（仅日期）" })),
                    });
                }
            }
            Err(e) => issues.push(ValidationIssue {
                level: "error".into(),
                index: idx + 1,
                message: e.message,
            }),
        }
    }

    // 计划时间
    let mut planned_at: Option<String> = None;
    let mut has_planned_time = false;
    for key in ["plannedAt", "scheduledAt"] {
        if let Some(raw) = obj.get(key).and_then(|v| v.as_str()) {
            match normalize_model_datetime("计划时间", raw) {
                Ok((dt, h)) => {
                    planned_at = dt.clone();
                    has_planned_time = h;
                    if let Some(d) = dt {
                        changes.push(FieldChange {
                            field: "plannedAt".into(),
                            label: "计划时间".into(),
                            before: None,
                            after: Some(format!("{d}{}", if h { "" } else { "（仅日期）" })),
                        });
                    }
                }
                Err(e) => issues.push(ValidationIssue {
                    level: "error".into(),
                    index: idx + 1,
                    message: e.message,
                }),
            }
            break;
        }
    }

    // 优先级：0–3，越界收敛为 0 并给出 warning 而不是直接失败
    let mut priority = 0i64;
    if let Some(p) = obj.get("priority").and_then(|v| v.as_i64()) {
        if (0..=3).contains(&p) {
            priority = p;
            if p > 0 {
                changes.push(FieldChange {
                    field: "priority".into(),
                    label: "优先级".into(),
                    before: None,
                    after: Some(
                        ["无", "低", "中", "高"]
                            .get(p as usize)
                            .unwrap_or(&"无")
                            .to_string(),
                    ),
                });
            }
        } else {
            issues.push(ValidationIssue {
                level: "warning".into(),
                index: idx + 1,
                message: format!("优先级 {p} 超出 0–3，已按「无」处理"),
            });
        }
    }

    let estimated = obj
        .get("estimatedMinutes")
        .and_then(|v| v.as_i64())
        .filter(|v| *v > 0 && *v <= 600_000);
    if let Some(m) = estimated {
        changes.push(FieldChange {
            field: "estimatedMinutes".into(),
            label: "预计耗时".into(),
            before: None,
            after: Some(format!("{m} 分钟")),
        });
    }

    let project_id = obj
        .get("projectId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let category_id = obj
        .get("categoryId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut payload = serde_json::json!({
        "title": title,
        "description": obj.get("description").and_then(|v| v.as_str()).unwrap_or(""),
        "priority": priority,
        "plannedAt": planned_at,
        "hasPlannedTime": has_planned_time,
        "dueAt": due_at,
        "hasDueTime": has_due_time,
        "estimatedMinutes": estimated,
        "projectId": project_id,
        "categoryId": category_id,
    });

    if let Some(raw) = obj.get("rrule").filter(|v| !v.is_null()) {
        let result = (|| -> AppResult<crate::recurrence::RecurrenceRule> {
            let rrule = raw
                .as_str()
                .ok_or_else(|| AppError::validation("重复规则必须是字符串"))?;
            crate::recurrence::RecurrenceRule::from_rrule_string(
                rrule,
                obj.get("tzid").and_then(|v| v.as_str()).unwrap_or("UTC"),
                obj.get("dtstartLocal")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
                obj.get("hasStartTime")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            )
        })();
        match result {
            Ok(rule) => {
                changes.push(FieldChange {
                    field: "rrule".into(),
                    label: "重复规则".into(),
                    before: None,
                    after: Some(rule.describe()),
                });
                for key in ["rrule", "dtstartLocal", "tzid"] {
                    payload[key] = obj.get(key).cloned().unwrap_or(serde_json::Value::Null);
                }
                payload["hasStartTime"] = serde_json::json!(obj
                    .get("hasStartTime")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false));
            }
            Err(e) => {
                issues.push(ValidationIssue {
                    level: "error".into(),
                    index: idx + 1,
                    message: e.message,
                });
                return None;
            }
        }
    }

    Some(DiffItem {
        action: DiffAction::Create,
        task_id: None,
        title,
        changes,
        payload,
        note: obj
            .get("reason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}

/// 校验一条 `reschedule` 条目
fn build_reschedule_item(
    idx: usize,
    obj: &serde_json::Map<String, serde_json::Value>,
    existing: &HashMap<String, (String, Option<String>)>,
    issues: &mut Vec<ValidationIssue>,
) -> Option<DiffItem> {
    let id = obj
        .get("taskId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    if id.is_empty() {
        issues.push(ValidationIssue {
            level: "error".into(),
            index: idx + 1,
            message: "该条缺少 taskId，无法定位要改期的任务".into(),
        });
        return None;
    }

    // **任务 ID 核验**（§6 明确要求）：模型可能编造不存在的 id
    let Some((title, old_planned)) = existing.get(&id) else {
        issues.push(ValidationIssue {
            level: "error".into(),
            index: idx + 1,
            message: format!("任务 ID 不存在，已忽略这一条（{id}）"),
        });
        return None;
    };

    let raw = obj
        .get("plannedAt")
        .or_else(|| obj.get("newDate"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let (planned_at, has_time) = match normalize_model_datetime("计划时间", raw) {
        Ok(v) => v,
        Err(e) => {
            issues.push(ValidationIssue {
                level: "error".into(),
                index: idx + 1,
                message: e.message,
            });
            return None;
        }
    };
    let Some(new_at) = planned_at else {
        issues.push(ValidationIssue {
            level: "error".into(),
            index: idx + 1,
            message: "缺少新的计划时间".into(),
        });
        return None;
    };

    // 改期到同一个时刻没有意义，提示但不阻止
    if old_planned.as_deref() == Some(new_at.as_str()) {
        issues.push(ValidationIssue {
            level: "warning".into(),
            index: idx + 1,
            message: format!("「{title}」的计划时间没有变化"),
        });
    }

    Some(DiffItem {
        action: DiffAction::Reschedule,
        task_id: Some(id.clone()),
        title: title.clone(),
        changes: vec![FieldChange {
            field: "plannedAt".into(),
            label: "计划时间".into(),
            before: old_planned.clone(),
            after: Some(new_at.clone()),
        }],
        payload: serde_json::json!({
            "taskId": id,
            "plannedAt": new_at,
            "hasPlannedTime": has_time,
        }),
        note: obj
            .get("reason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}

/// 重复项检测（§6 明确要求）
fn detect_duplicates(items: &[DiffItem], issues: &mut Vec<ValidationIssue>) {
    // 同批次内标题+时间都相同的 create 视为重复
    let mut seen: HashMap<String, usize> = HashMap::new();
    for (i, it) in items.iter().enumerate() {
        if it.action != DiffAction::Create {
            continue;
        }
        let key = format!(
            "{}|{}",
            it.title.trim().to_lowercase(),
            it.payload
                .get("plannedAt")
                .and_then(|v| v.as_str())
                .unwrap_or("")
        );
        if let Some(prev) = seen.get(&key) {
            issues.push(ValidationIssue {
                level: "warning".into(),
                index: i + 1,
                message: format!("与第 {} 条内容重复（标题与计划时间相同）", prev + 1),
            });
        } else {
            seen.insert(key, i);
        }
    }

    // 同一任务被多次改期时只有最后一次生效，提前说明
    let mut planned: HashMap<String, usize> = HashMap::new();
    for (i, it) in items.iter().enumerate() {
        if it.action != DiffAction::Reschedule {
            continue;
        }
        if let Some(id) = it.task_id.as_ref() {
            if let Some(prev) = planned.get(id) {
                issues.push(ValidationIssue {
                    level: "warning".into(),
                    index: i + 1,
                    message: format!("同一任务在第 {} 条已改期，将以本条的最终时间为准", prev + 1),
                });
            } else {
                planned.insert(id.clone(), i);
            }
        }
    }
}

// =============================================================================
// 上下文收集（控制发送给模型的数据范围）
// =============================================================================

/// 收集待整理的任务摘要。
///
/// **默认不发送备注正文**（§6 要求"敏感备注、附件内容默认不发送"），
/// 只发标题、截止时间、优先级等排程必需字段。
async fn collect_task_brief(
    state: &AppState,
    ids: Option<&[String]>,
    limit: i64,
    send_notes: bool,
) -> AppResult<(
    Vec<serde_json::Value>,
    HashMap<String, (String, Option<String>)>,
)> {
    let rows = if let Some(id_list) = ids.filter(|l| !l.is_empty()) {
        // 只取用户选中的任务
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT id, title, description, note_md, status, priority, planned_at, due_at, estimated_minutes
             FROM tasks WHERE deleted_at IS NULL AND id IN (",
        );
        let mut sep = qb.separated(", ");
        for id in id_list {
            sep.push_bind(id.clone());
        }
        sep.push_unseparated(") LIMIT 200");
        qb.build().fetch_all(state.db.pool()).await?
    } else {
        sqlx::query(
            "SELECT id, title, description, note_md, status, priority, planned_at, due_at, estimated_minutes
             FROM tasks WHERE deleted_at IS NULL AND status NOT IN ('done','archived')
             ORDER BY (due_at IS NULL), due_at ASC, priority DESC LIMIT ?1",
        )
        .bind(limit.clamp(1, 200))
        .fetch_all(state.db.pool())
        .await?
    };

    let mut brief = Vec::with_capacity(rows.len());
    let mut index = HashMap::new();

    for r in rows {
        let id: String = r.try_get("id")?;
        let title: String = r.try_get("title")?;
        let planned: Option<String> = r.try_get("planned_at")?;
        index.insert(id.clone(), (title.clone(), planned.clone()));

        let mut o = serde_json::json!({
            "id": id,
            "title": title,
            "status": r.try_get::<String, _>("status")?,
            "priority": r.try_get::<i64, _>("priority")?,
            "plannedAt": planned,
            "dueAt": r.try_get::<Option<String>, _>("due_at")?,
            "estimatedMinutes": r.try_get::<Option<i64>, _>("estimated_minutes")?,
        });

        // 描述通常是一句话的概要，默认发送；备注可能含敏感细节，默认不发
        let desc: String = r.try_get("description")?;
        if !desc.trim().is_empty() {
            o["description"] = serde_json::json!(desc);
        }
        if send_notes {
            let notes: String = r.try_get("note_md")?;
            if !notes.trim().is_empty() {
                o["notes"] = serde_json::json!(notes);
            }
        }

        brief.push(o);
    }

    Ok((brief, index))
}

/// 数据范围说明（§6 要求让用户知道发了什么）
fn scope_note(send_notes: bool, count: usize) -> String {
    let base = format!(
        "本次向模型发送了 {count} 个任务的标题、状态、优先级、计划与截止时间（均为排程所必需）。"
    );
    if send_notes {
        format!("{base}另外，按你的设置，Markdown 备注正文也一并发送了。附件内容从不发送。")
    } else {
        format!("{base}备注正文与附件内容**未发送**（可在 AI 设置中开启备注，附件始终不发送）。")
    }
}

// =============================================================================
// 能力：自由输入整理成候选任务
// =============================================================================

/// 请求输入
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeInput {
    #[serde(default)]
    pub asset_ids: Vec<String>,
    /// 用户自由输入的文本（如一段会议记录）
    pub text: String,
    /// 是否发送已有任务的备注（默认否）
    #[serde(default)]
    pub send_notes: bool,
    #[serde(default)]
    pub defaults: Option<serde_json::Value>,
    #[serde(default)]
    pub tzid: Option<String>,
}

const ORGANIZE_SYSTEM: &str = r#"你是一个任务整理助手。用户会给你一段自由文本，请把它整理成结构化的任务候选。
只输出 JSON，不要任何解释文字。JSON 格式：
{"tasks":[{"title":"任务标题","description":"要求和注意事项","dueAt":"YYYY-MM-DD 或带时区的 ISO-8601 或 null","plannedAt":"同上或 null","priority":0,"estimatedMinutes":30,"rrule":null,"dtstartLocal":null,"hasStartTime":false,"reason":"为什么这样安排"}]}
要求：
- title 必填，简洁明确，不超过 100 字。
- priority 取 0（无）、1（低）、2（中）、3（高）。
- 日期无法从原文推断时填 null，不要编造日期。
- 不要创建与已有任务重复的条目。
- 用户说每周一、每周几天、每天、每月时，必须生成重复规则，不能转换成一个单次任务。每周一用 FREQ=WEEKLY;BYDAY=MO，周三和周五用 FREQ=WEEKLY;BYDAY=WE,FR。每天默认双休，用 FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR；用户明确每天含周末时用 FREQ=DAILY。法定节假日不执行加 ;X-LUMEN-HOLIDAYS=CN。不要擅自设置调休补班。
- 重复任务必须填写 dtstartLocal（YYYY-MM-DDTHH:MM:SS，本地首次日期；未指定开始日期从今天开始），有具体时间时 hasStartTime=true。普通任务 rrule=null。
- 最多 20 条。"#;

#[tauri::command]
pub async fn ai_organize(
    state: State<'_, AppState>,
    config: ProviderConfig,
    input: OrganizeInput,
) -> AppResult<DiffPreview> {
    let text = input.text.trim();
    if text.is_empty() && input.asset_ids.is_empty() {
        return Err(AppError::validation(
            "请先输入文本或添加需要分析的图片、文件",
        ));
    }
    if text.chars().count() > 20_000 {
        return Err(AppError::validation(
            "输入过长，请分段整理（上限 20000 字符）",
        ));
    }

    // 已有任务摘要用于去重判断，但**不发送备注**（除用户显式开启）
    let (brief, _index) = collect_task_brief(&state, None, 80, input.send_notes).await?;

    let user = format!(
        "当前本地日期与时区：{}。原文中的今天、明天、本周以此为准。\n已有任务（用于避免重复）：\n{}\n\n需要整理的文本：\n{}",
        chrono::Local::now().format("%Y-%m-%d %H:%M %:z"),
        serde_json::to_string(&brief).unwrap_or_else(|_| "[]".into()),
        text
    );

    let media = crate::ai_media::load_media(&state.db, config.provider, &input.asset_ids).await?;
    let resp = ai::chat(
        &config,
        &ChatRequest {
            config: config.clone(),
            system: Some(ORGANIZE_SYSTEM.to_string()),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: user,
            }],
            json_output: true,
            max_output_tokens: None,
            media,
        },
    )
    .await?;

    let mut issues = Vec::new();
    let value = extract_json(&resp.text)?;
    let arr = value
        .get("tasks")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            AppError::validation("模型返回的 JSON 缺少 tasks 数组")
                .with_hint("可点「重新生成」再试一次")
        })?;

    if arr.len() > MAX_ITEMS {
        issues.push(ValidationIssue {
            level: "warning".into(),
            index: 0,
            message: format!("模型返回了 {} 条，仅采用前 {MAX_ITEMS} 条", arr.len()),
        });
    }

    let mut items = Vec::new();
    for (i, v) in arr.iter().take(MAX_ITEMS).enumerate() {
        let Some(obj) = v.as_object() else {
            issues.push(ValidationIssue {
                level: "error".into(),
                index: i + 1,
                message: "该条不是对象，已忽略".into(),
            });
            continue;
        };
        let mut obj = obj.clone();
        obj.insert(
            "tzid".into(),
            serde_json::json!(input.tzid.as_deref().unwrap_or("UTC")),
        );
        if let Some(defaults) = &input.defaults {
            for key in ["projectId", "categoryId", "tagIds", "periodType"] {
                if let Some(value) = defaults.get(key) {
                    obj.insert(key.into(), value.clone());
                }
            }
            if obj.get("rrule").is_none_or(|v| v.is_null())
                && obj.get("plannedAt").is_none_or(|v| v.is_null())
            {
                if let Some(date) = defaults
                    .get("plannedDate")
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.is_empty())
                {
                    obj.insert("plannedAt".into(), serde_json::json!(date));
                }
            }
        }
        if let Some(mut it) = build_create_item(i, &obj, &mut issues) {
            for key in ["tagIds", "periodType"] {
                if let Some(value) = obj.get(key) {
                    it.payload[key] = value.clone();
                }
            }
            items.push(it);
        }
    }

    detect_duplicates(&items, &mut issues);

    finish_preview(
        "整理任务",
        format!("从输入中整理出 {} 个候选任务", items.len()),
        items,
        issues,
        resp.text,
        resp.usage,
        format!(
            "{}；另发送当前输入选择的 {} 个图片 / 文件内容。",
            scope_note(input.send_notes, brief.len()),
            input.asset_ids.len()
        ),
    )
}

// =============================================================================
// 能力：大任务拆解为子任务
// =============================================================================

/// 拆解输入
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakdownInput {
    pub task_id: String,
    /// 是否把结果同时创建为子任务（false 时只返回建议）
    #[serde(default)]
    pub as_subtasks: bool,
}

const BREAKDOWN_SYSTEM: &str = r#"你是一个任务拆解助手。用户给你一个任务，请把它拆成 3–8 个可执行的具体步骤。
只输出 JSON，不要任何解释文字。JSON 格式：
{"steps":[{"title":"步骤标题","estimatedMinutes":30,"reason":"这一步要做什么"}]}
要求：
- 每个步骤应当是可以独立完成的小动作，而不是抽象阶段。
- estimatedMinutes 是完成该步骤的预计分钟数。
- 最多 8 步，最少 3 步。"#;

/// 拆解结果（子任务建议也可走预览确认）
#[tauri::command]
pub async fn ai_breakdown(
    state: State<'_, AppState>,
    config: ProviderConfig,
    input: BreakdownInput,
) -> AppResult<DiffPreview> {
    let row = sqlx::query(
        "SELECT id, title, description, note_md, estimated_minutes FROM tasks
         WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(&input.task_id)
    .fetch_optional(state.db.pool())
    .await?;

    let Some(row) = row else {
        return Err(AppError::not_found("任务", &input.task_id));
    };

    let title: String = row.try_get("title")?;
    let desc: String = row.try_get("description")?;
    let est: Option<i64> = row.try_get("estimated_minutes")?;

    // 拆解需要理解任务内容，因此这里发送描述；备注仍按默认不发送
    let mut payload = serde_json::json!({
        "title": title,
        "description": desc,
        "estimatedMinutes": est,
    });
    if input.as_subtasks {
        payload["note"] = serde_json::json!("结果将以子任务形式提交，需要你确认后才会写入");
    }

    let resp = ai::chat(
        &config,
        &ChatRequest {
            config: config.clone(),
            system: Some(BREAKDOWN_SYSTEM.to_string()),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: serde_json::to_string(&payload).unwrap_or_default(),
            }],
            json_output: true,
            max_output_tokens: None,
            media: vec![],
        },
    )
    .await?;

    let mut issues = Vec::new();
    let value = extract_json(&resp.text)?;
    let arr = value
        .get("steps")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AppError::validation("模型返回的 JSON 缺少 steps 数组"))?;

    let mut items = Vec::new();
    for (i, v) in arr.iter().take(20).enumerate() {
        let Some(obj) = v.as_object() else { continue };
        let step_title = obj
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if step_title.is_empty() {
            issues.push(ValidationIssue {
                level: "error".into(),
                index: i + 1,
                message: "该步骤缺少标题".into(),
            });
            continue;
        }
        let minutes = obj
            .get("estimatedMinutes")
            .and_then(|v| v.as_i64())
            .filter(|m| *m > 0 && *m <= 60 * 24);

        let mut changes = vec![FieldChange {
            field: "title".into(),
            label: "子任务".into(),
            before: None,
            after: Some(step_title.clone()),
        }];
        if let Some(m) = minutes {
            changes.push(FieldChange {
                field: "estimatedMinutes".into(),
                label: "预计耗时".into(),
                before: None,
                after: Some(format!("{m} 分钟")),
            });
        }

        items.push(DiffItem {
            action: DiffAction::Create,
            task_id: Some(input.task_id.clone()),
            title: step_title,
            changes,
            payload: serde_json::json!({
                "parentTaskId": input.task_id,
                "title": obj.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                "estimatedMinutes": minutes,
                "asSubtask": input.as_subtasks,
            }),
            note: obj
                .get("reason")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        });
    }

    // 拆解结果不应互相重复
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut unique = Vec::new();
    for (i, it) in items.into_iter().enumerate() {
        let k = it.title.trim().to_lowercase();
        if let Some(prev) = seen.get(&k) {
            issues.push(ValidationIssue {
                level: "warning".into(),
                index: i + 1,
                message: format!("与第 {} 步重复，已忽略", prev + 1),
            });
        } else {
            seen.insert(k, i);
            unique.push(it);
        }
    }

    if unique.len() < 2 {
        issues.push(ValidationIssue {
            level: "warning".into(),
            index: 0,
            message: "拆解结果过少，可换用能力更强的模型重新生成".into(),
        });
    }

    finish_preview(
        "拆解任务",
        format!("把「{title}」拆成 {} 个步骤", unique.len()),
        unique,
        issues,
        resp.text,
        resp.usage,
        "本次向模型发送了该任务的标题与描述。Markdown 备注与附件内容未发送。".to_string(),
    )
}

// =============================================================================
// 能力：生成每日/每周计划
// =============================================================================

/// 排程输入
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInput {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub asset_ids: Vec<String>,
    /// daily = 一天；weekly = 一周
    pub horizon: String,
    /// 每天可用的分钟数（用于约束总量）
    pub minutes_per_day: i64,
    /// 起始日期（本地 `YYYY-MM-DD`），空表示今天
    #[serde(default)]
    pub start_date: Option<String>,
    /// 只排这些任务（空表示全部未完成）
    #[serde(default)]
    pub task_ids: Option<Vec<String>>,
    #[serde(default)]
    pub send_notes: bool,
}

const PLAN_SYSTEM: &str = r#"你是一个排程助手。给你一批任务和每天可用的时间，请把它们安排到具体日期。
只输出 JSON，不要任何解释文字。JSON 格式：
{"schedule":[{"taskId":"任务id","plannedAt":"YYYY-MM-DD 或带时区 ISO-8601","reason":"为什么排在这一天"}],"conflicts":[{"taskIds":["id1","id2"],"reason":"冲突原因"}],"unscheduled":[{"taskId":"id","reason":"为什么排不下"}]}
要求：
- 严格遵守截止时间：不能把任务排到它的截止时间之后。
- 遵守依赖关系：被依赖的任务要排在依赖它的任务之前。
- 每天安排的总预计耗时不要超过给定的每日可用时间。
- 若确实排不下，放进 unscheduled 并说明原因，不要硬塞。
- plannedAt 使用 YYYY-MM-DD 格式（表示仅日期）。"#;

#[tauri::command]
pub async fn ai_plan(
    state: State<'_, AppState>,
    config: ProviderConfig,
    input: PlanInput,
) -> AppResult<DiffPreview> {
    let days = match input.horizon.as_str() {
        "daily" | "day" => 1,
        "weekly" | "week" => 7,
        other => {
            return Err(AppError::validation(format!("不支持的排程范围：{other}"))
                .with_hint("允许值：daily / weekly"))
        }
    };
    if input.minutes_per_day <= 0 || input.minutes_per_day > 24 * 60 {
        return Err(AppError::validation("每日可用时间应在 1–1440 分钟之间"));
    }

    let (brief, index) =
        collect_task_brief(&state, input.task_ids.as_deref(), 120, input.send_notes).await?;

    if brief.is_empty() {
        return Err(AppError::validation("没有可安排的任务")
            .with_hint("请先创建一些未完成的任务，或检查筛选条件"));
    }

    // 依赖关系一并发送，让模型能遵守先后顺序
    let deps = sqlx::query(
        "SELECT d.task_id, d.depends_on_id FROM task_dependencies d
         JOIN tasks a ON a.id = d.task_id AND a.deleted_at IS NULL
         JOIN tasks b ON b.id = d.depends_on_id AND b.deleted_at IS NULL",
    )
    .fetch_all(state.db.pool())
    .await?;
    let dep_list: Vec<serde_json::Value> = deps
        .iter()
        .map(|r| {
            serde_json::json!({
                "taskId": r.try_get::<String, _>("task_id").unwrap_or_default(),
                "dependsOn": r.try_get::<String, _>("depends_on_id").unwrap_or_default(),
            })
        })
        .collect();

    let start = input
        .start_date
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());

    let user = serde_json::json!({
        "horizonDays": days,
        "startDate": start,
        "minutesPerDay": input.minutes_per_day,
        "tasks": brief,
        "dependencies": dep_list,
        "currentDateTime": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
    });

    let extra = input.text.as_deref().unwrap_or("");
    if extra.chars().count() > 20_000 {
        return Err(AppError::validation("补充说明超过 20000 字"));
    }
    let user = format!("{user}\n用户补充说明：{extra}");
    let media = crate::ai_media::load_media(&state.db, config.provider, &input.asset_ids).await?;
    let resp = ai::chat(
        &config,
        &ChatRequest {
            config: config.clone(),
            system: Some(PLAN_SYSTEM.to_string()),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: serde_json::to_string(&user).unwrap_or_default(),
            }],
            json_output: true,
            max_output_tokens: None,
            media,
        },
    )
    .await?;

    let mut issues = Vec::new();
    let value = extract_json(&resp.text)?;
    let arr = value
        .get("schedule")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut items = Vec::new();
    for (i, v) in arr.iter().take(MAX_ITEMS).enumerate() {
        let Some(obj) = v.as_object() else { continue };
        if let Some(it) = build_reschedule_item(i, obj, &index, &mut issues) {
            items.push(it);
        }
    }

    // 模型报告的冲突与排不下的任务，转成提示信息让用户看到
    if let Some(conflicts) = value.get("conflicts").and_then(|v| v.as_array()) {
        for c in conflicts.iter().take(10) {
            let reason = c
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("存在时间冲突");
            issues.push(ValidationIssue {
                level: "warning".into(),
                index: 0,
                message: format!("模型提示冲突：{reason}"),
            });
        }
    }
    if let Some(unsched) = value.get("unscheduled").and_then(|v| v.as_array()) {
        for u in unsched.iter().take(10) {
            let reason = u.get("reason").and_then(|v| v.as_str()).unwrap_or("排不下");
            let tid = u.get("taskId").and_then(|v| v.as_str()).unwrap_or("");
            let name = index.get(tid).map(|(t, _)| t.as_str()).unwrap_or(tid);
            issues.push(ValidationIssue {
                level: "warning".into(),
                index: 0,
                message: format!("「{name}」未能安排：{reason}"),
            });
        }
    }

    detect_duplicates(&items, &mut issues);

    // 排程本身的合理性检查：不能把任务排到自己的截止时间之后
    let mut late = 0;
    for (i, it) in items.iter().enumerate() {
        let Some(tid) = it.task_id.as_ref() else {
            continue;
        };
        let due: Option<String> = sqlx::query("SELECT due_at FROM tasks WHERE id = ?1")
            .bind(tid)
            .fetch_optional(state.db.pool())
            .await?
            .and_then(|r| r.try_get("due_at").ok().flatten());
        let new_at = it.payload.get("plannedAt").and_then(|v| v.as_str());
        if let (Some(d), Some(p)) = (due, new_at) {
            if p > d.as_str() {
                late += 1;
                issues.push(ValidationIssue {
                    level: "warning".into(),
                    index: i + 1,
                    message: format!("「{}」被排到截止时间之后，建议调整", it.title),
                });
            }
        }
    }
    if late > 0 {
        log::warn!("AI 排程中有 {late} 条晚于截止时间，已作为警告呈现给用户");
    }

    finish_preview(
        "AI 排程",
        format!(
            "为 {} 个任务生成{}安排",
            items.len(),
            if days == 1 { "今日" } else { "本周" }
        ),
        items,
        issues,
        resp.text,
        resp.usage,
        format!(
            "{}；另发送当前输入选择的 {} 个图片 / 文件内容。",
            scope_note(input.send_notes, brief.len()),
            input.asset_ids.len()
        ),
    )
}

// =============================================================================
// 能力：每日/每周复盘
// =============================================================================

/// 复盘输入
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewInput {
    #[serde(default)]
    pub asset_ids: Vec<String>,
    /// daily / weekly / monthly / yearly
    pub horizon: String,
    #[serde(default)]
    pub anchor_date: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
}

fn review_range(
    horizon: &str,
    anchor: chrono::NaiveDate,
) -> AppResult<(chrono::NaiveDate, chrono::NaiveDate)> {
    use chrono::Datelike;
    let range = match horizon {
        "daily" | "day" => (anchor, anchor.succ_opt()),
        "weekly" | "week" => {
            let start =
                anchor - chrono::Duration::days(anchor.weekday().num_days_from_monday() as i64);
            (start, start.checked_add_signed(chrono::Duration::days(7)))
        }
        "monthly" | "month" => {
            let start = anchor
                .with_day(1)
                .ok_or_else(|| AppError::validation("无效月份"))?;
            (start, start.checked_add_months(chrono::Months::new(1)))
        }
        "yearly" | "year" => {
            let start = chrono::NaiveDate::from_ymd_opt(anchor.year(), 1, 1)
                .ok_or_else(|| AppError::validation("无效年份"))?;
            (
                start,
                chrono::NaiveDate::from_ymd_opt(anchor.year() + 1, 1, 1),
            )
        }
        other => {
            return Err(AppError::validation(format!("不支持的复盘范围：{other}"))
                .with_hint("允许 daily / weekly / monthly / yearly"))
        }
    };
    Ok((
        range.0,
        range
            .1
            .ok_or_else(|| AppError::validation("日期超出范围"))?,
    ))
}

/// 复盘结果（**纯只读**，不产生任何写入）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResult {
    pub text: String,
    pub summary: String,
    pub stats: serde_json::Value,
    pub usage: Option<ai::TokenUsage>,
    pub data_scope_note: String,
}

const REVIEW_SYSTEM: &str = r#"你是一个务实的复盘助手。用户会给你真实的完成记录数据，请写一份简短的复盘。
要求：
- 用中文，根据日、周、月、年范围给出工作成果、未完成项与下一步建议；有用户文本时结合文本总结。
- 只基于给出的数据说话，**绝对不要编造**没有出现在数据里的事实。
- 不要夸大成绩，也不要说教。若完成率低，直接指出并给一条具体可行的建议。
- 不要使用"继续保持"这类空话。"#;

#[tauri::command]
pub async fn ai_review(
    state: State<'_, AppState>,
    config: ProviderConfig,
    input: ReviewInput,
) -> AppResult<ReviewResult> {
    let anchor = match input.anchor_date.as_deref() {
        Some(s) => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|_| AppError::validation("总结日期格式应为 YYYY-MM-DD"))?,
        None => chrono::Local::now().date_naive(),
    };
    let (start, end) = review_range(&input.horizon, anchor)?;
    let text = input.text.as_deref().unwrap_or("").trim();
    if text.chars().count() > 20_000 {
        return Err(AppError::validation("总结文本超过 20000 字，请分段处理"));
    }

    // 统计口径与统计页保持一致，避免"复盘说的和界面显示的不一样"
    let stats = crate::stats::collect_calendar_stats(&state, start, end).await?;
    let range = format!("{} 至 {}", start, end - chrono::Duration::days(1));

    let user = serde_json::json!({
        "range": range,
        "today": chrono::Local::now().date_naive().to_string(),
        "userText": text,
        "data": stats,
    });

    let media = crate::ai_media::load_media(&state.db, config.provider, &input.asset_ids).await?;
    let resp = ai::chat(
        &config,
        &ChatRequest {
            config: config.clone(),
            system: Some(REVIEW_SYSTEM.to_string()),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: serde_json::to_string(&user).unwrap_or_default(),
            }],
            json_output: false,
            max_output_tokens: None,
            media,
        },
    )
    .await?;

    Ok(ReviewResult {
        text: resp.text.clone(),
        summary: format!("{range} 工作总结"),
        stats,
        usage: resp.usage,
        data_scope_note: format!(
            "发送 {range} 的聚合统计（数量、完成率、耗时等）{}，以及当前输入选择的 {} 个图片 / 文件；不自动发送其他任务正文或附件。",
            if text.is_empty() {
                ""
            } else {
                "及用户提供的文本"
            },
            input.asset_ids.len()
        ),
    })
}

// =============================================================================
// 预览收尾与应用
// =============================================================================

/// 组装预览并登记
fn finish_preview(
    capability: &str,
    summary: String,
    items: Vec<DiffItem>,
    issues: Vec<ValidationIssue>,
    raw: String,
    usage: Option<ai::TokenUsage>,
    data_scope_note: String,
) -> AppResult<DiffPreview> {
    let has_error = issues.iter().any(|i| i.level == "error");
    let acceptable = !has_error && !items.is_empty();

    let preview_id = register(capability, items.clone());

    Ok(DiffPreview {
        preview_id,
        capability: capability.to_string(),
        summary,
        items,
        issues,
        acceptable,
        raw,
        usage,
        data_scope_note,
    })
}

/// 用户确认后写入。
///
/// `accept_indices` 是用户勾选接受的条目下标（从 0 开始）；
/// 传 None 表示全部接受。**未勾选的条目一律不写入**。
#[tauri::command]
pub async fn ai_apply(
    state: State<'_, AppState>,
    preview_id: String,
    accept_indices: Option<Vec<usize>>,
) -> AppResult<ApplyResult> {
    ai_apply_impl(&state, preview_id, accept_indices).await
}

pub(crate) async fn ai_apply_impl(
    state: &AppState,
    preview_id: String,
    accept_indices: Option<Vec<usize>>,
) -> AppResult<ApplyResult> {
    let (capability, items) = take(&preview_id)?;

    let chosen: Vec<DiffItem> = match accept_indices {
        Some(idx) => {
            let set: std::collections::HashSet<usize> = idx.into_iter().collect();
            items
                .into_iter()
                .enumerate()
                .filter(|(i, _)| set.contains(i))
                .map(|(_, it)| it)
                .collect()
        }
        None => items,
    };

    let total = chosen.len();
    if total == 0 {
        return Ok(ApplyResult {
            created: 0,
            updated: 0,
            skipped: 0,
            warnings: vec![],
        });
    }

    let mut created = 0usize;
    let mut updated = 0usize;
    let mut recurring = vec![];
    let now = crate::db::to_db_time(crate::db::utc_now());

    let mut tx = state.db.pool().begin().await?;

    for it in &chosen {
        match it.action {
            DiffAction::Create => {
                if it.payload.get("rrule").is_some_and(|v| !v.is_null()) {
                    let input: crate::recurrence_service::CreateRecurringInput =
                        serde_json::from_value(it.payload.clone())
                            .map_err(|e| AppError::validation(format!("重复任务参数无效：{e}")))?;
                    let (series, first, _) =
                        crate::recurrence_service::create_recurring_seed_tx(&mut tx, &input)
                            .await?;
                    recurring.push((series, first));
                    created += 1;
                    continue;
                }
                // 子任务形式
                if let Some(parent) = it.payload.get("parentTaskId").and_then(|v| v.as_str()) {
                    let as_sub = it
                        .payload
                        .get("asSubtask")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if as_sub {
                        let id = uuid::Uuid::now_v7().to_string();
                        let row = sqlx::query(
                            "SELECT CAST(COALESCE(MAX(sort_order), 0) + 1 AS REAL) AS n
                             FROM subtasks WHERE task_id = ?1",
                        )
                        .bind(parent)
                        .fetch_one(&mut *tx)
                        .await?;
                        let ord: f64 = row.try_get("n")?;
                        sqlx::query(
                            "INSERT INTO subtasks (id, task_id, title, is_done, sort_order, created_at, updated_at)
                             VALUES (?1, ?2, ?3, 0, ?4, ?5, ?5)",
                        )
                        .bind(&id)
                        .bind(parent)
                        .bind(&it.title)
                        .bind(ord)
                        .bind(&now)
                        .execute(&mut *tx)
                        .await?;
                        created += 1;
                        continue;
                    }
                }

                // 普通任务
                let id = uuid::Uuid::now_v7().to_string();
                sqlx::query(
                    "INSERT INTO tasks (
                        id, title, description, status, priority, project_id, category_id,
                        planned_at, has_planned_time, due_at, has_due_time,
                        estimated_minutes, created_at, updated_at, sort_order,
                        occurrence_kind, is_exception, sync_rev, sync_state
                     ) VALUES (
                        ?1, ?2, ?3, 'todo', ?4, ?5, ?6,
                        ?7, ?8, ?9, ?10,
                        ?11, ?12, ?12, 0,
                        'single', 0, 0, 'local'
                     )",
                )
                .bind(&id)
                .bind(
                    it.payload
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&it.title),
                )
                .bind(
                    it.payload
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                )
                .bind(
                    it.payload
                        .get("priority")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                )
                .bind(it.payload.get("projectId").and_then(|v| v.as_str()))
                .bind(it.payload.get("categoryId").and_then(|v| v.as_str()))
                .bind(it.payload.get("plannedAt").and_then(|v| v.as_str()))
                .bind(
                    it.payload
                        .get("hasPlannedTime")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false) as i64,
                )
                .bind(it.payload.get("dueAt").and_then(|v| v.as_str()))
                .bind(
                    it.payload
                        .get("hasDueTime")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false) as i64,
                )
                .bind(it.payload.get("estimatedMinutes").and_then(|v| v.as_i64()))
                .bind(&now)
                .execute(&mut *tx)
                .await?;
                sqlx::query("UPDATE tasks SET period_type = ?1 WHERE id = ?2")
                    .bind(
                        it.payload
                            .get("periodType")
                            .and_then(|v| v.as_str())
                            .unwrap_or("none"),
                    )
                    .bind(&id)
                    .execute(&mut *tx)
                    .await?;
                if let Some(tags) = it.payload.get("tagIds").and_then(|v| v.as_array()) {
                    for tag in tags.iter().filter_map(|v| v.as_str()) {
                        sqlx::query("INSERT INTO task_tags(task_id,tag_id) VALUES(?1,?2)")
                            .bind(&id)
                            .bind(tag)
                            .execute(&mut *tx)
                            .await?;
                    }
                }
                created += 1;
            }

            DiffAction::Reschedule | DiffAction::Update => {
                let Some(tid) = it.task_id.as_ref() else {
                    continue;
                };
                let planned = it.payload.get("plannedAt").and_then(|v| v.as_str());
                if let Some(p) = planned {
                    let has_time = it
                        .payload
                        .get("hasPlannedTime")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let n = sqlx::query(
                        "UPDATE tasks SET planned_at = ?1, has_planned_time = ?2, updated_at = ?3,
                         occurrence_kind = CASE WHEN series_id IS NOT NULL THEN 'exception' ELSE occurrence_kind END,
                         is_exception = CASE WHEN series_id IS NOT NULL THEN 1 ELSE is_exception END
                         WHERE id = ?4 AND deleted_at IS NULL",
                    )
                    .bind(p)
                    .bind(has_time as i64)
                    .bind(&now)
                    .bind(tid)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                    if n > 0 {
                        // AI scheduling is explicitly a this-only change for recurring
                        // occurrences; keep occurrence_key stable and reminders atomic.
                        crate::reminders::recompute_task_reminders_tx(&mut tx, tid).await?;
                        updated += 1;
                    }
                }
            }
        }
    }

    tx.commit().await?;

    let mut warnings = vec![];
    for (series, first) in recurring {
        let end = chrono::DateTime::parse_from_rfc3339(&first)
            .map_err(|e| AppError::internal(format!("首次发生日期无效：{e}")))?
            + chrono::Duration::days(90);
        if let Err(e) = crate::recurrence_service::recurring_materialize_inner(
            state,
            series,
            first,
            crate::db::to_db_time(end.with_timezone(&chrono::Utc)),
        )
        .await
        {
            warnings.push(format!(
                "系列已保存，后续发生生成失败：{e}；后台维护会重试。"
            ));
        }
    }

    log::info!(
        "应用 AI「{capability}」预览：新增 {created}、修改 {updated}（共 {total} 条被接受）"
    );

    Ok(ApplyResult {
        created,
        updated,
        skipped: 0,
        warnings,
    })
}

/// 放弃预览（显式释放，不必等过期）
#[tauri::command]
pub async fn ai_discard(preview_id: String) -> AppResult<bool> {
    let mut r = registry()
        .lock()
        .map_err(|_| AppError::internal("预览注册表不可用"))?;
    Ok(r.map.remove(&preview_id).is_some())
}

/// 冲突检测（不依赖 AI，纯规则计算；§6 要求"识别明显排程冲突并建议改期"）
#[tauri::command]
pub async fn schedule_conflicts(state: State<'_, AppState>) -> AppResult<Vec<serde_json::Value>> {
    // 同一天内被安排了过多耗时，或截止时间早于计划时间
    let rows = sqlx::query(
        "SELECT
            substr(planned_at, 1, 10) AS day,
            COUNT(*) AS cnt,
            COALESCE(SUM(COALESCE(estimated_minutes, 0)), 0) AS mins,
            (SELECT COUNT(*) FROM tasks t2
              WHERE t2.deleted_at IS NULL
                AND t2.status NOT IN ('done','archived')
                AND t2.due_at IS NOT NULL
                AND t2.planned_at IS NOT NULL
                AND t2.planned_at > t2.due_at
                AND substr(t2.planned_at, 1, 10) = substr(tasks.planned_at, 1, 10)) AS late_cnt
         FROM tasks
         WHERE deleted_at IS NULL
           AND status NOT IN ('done','archived')
           AND planned_at IS NOT NULL
         GROUP BY day
         ORDER BY day ASC",
    )
    .fetch_all(state.db.pool())
    .await?;

    let mut out = Vec::new();
    for r in rows {
        let day: String = r.try_get("day")?;
        let cnt: i64 = r.try_get("cnt")?;
        let mins: i64 = r.try_get("mins")?;
        let late: i64 = r.try_get("late_cnt")?;

        // 超过 8 小时的工作量在一天里通常不现实
        if mins > 8 * 60 {
            out.push(serde_json::json!({
                "day": day,
                "kind": "overload",
                "message": format!("{day} 安排了 {cnt} 项任务、共约 {:.1} 小时，可能超出实际可用时间", mins as f64 / 60.0),
            }));
        }
        if late > 0 {
            out.push(serde_json::json!({
                "day": day,
                "kind": "after_due",
                "message": format!("{day} 有 {late} 项任务的计划时间晚于其截止时间"),
            }));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn ai_weekly_candidates_create_real_series_only_after_confirmation() {
        let dir = std::env::temp_dir().join(format!("lumen-ai-weekly-{}", uuid::Uuid::now_v7()));
        let state = AppState::new(crate::db::Db::init(&dir).await.unwrap());
        let obj = serde_json::json!({"title":"每周运营表", "rrule":"FREQ=WEEKLY;BYDAY=MO,WE", "dtstartLocal":"2026-09-30T00:00:00", "tzid":"Asia/Shanghai", "hasStartTime":false});
        let mut issues = vec![];
        let item = build_create_item(0, obj.as_object().unwrap(), &mut issues).unwrap();
        assert!(issues.is_empty());
        assert!(item.changes.iter().any(|c| c.field == "rrule"));
        let id = register("整理任务", vec![item]);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM task_series")
                .fetch_one(state.db.pool())
                .await
                .unwrap(),
            0
        );
        assert_eq!(ai_apply_impl(&state, id, None).await.unwrap().created, 1);
        let rule: String = sqlx::query_scalar("SELECT rrule FROM task_series")
            .fetch_one(state.db.pool())
            .await
            .unwrap();
        assert_eq!(rule, "FREQ=WEEKLY;BYDAY=MO,WE");
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT planned_at, series_id FROM tasks ORDER BY planned_at")
                .fetch_all(state.db.pool())
                .await
                .unwrap();
        assert!(rows.len() > 4);
        assert!(rows.iter().all(|(_, s)| s == &rows[0].1));
        for (at, _) in rows {
            use chrono::Datelike;
            let date = chrono::DateTime::parse_from_rfc3339(&at)
                .unwrap()
                .with_timezone(&chrono_tz::Asia::Shanghai);
            assert!(matches!(
                date.weekday(),
                chrono::Weekday::Mon | chrono::Weekday::Wed
            ));
        }
        let invalid = serde_json::json!({"title":"坏规则", "rrule":"FREQ=INVALID", "dtstartLocal":"2026-09-30T00:00:00"});
        let mut issues = vec![];
        build_create_item(0, invalid.as_object().unwrap(), &mut issues);
        assert!(issues.iter().any(|i| i.level == "error"));
        state.db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn edited_preview_is_validated_before_writing_and_applies_selected_items() {
        let dir = std::env::temp_dir().join(format!("lumen-ai-edit-{}", uuid::Uuid::now_v7()));
        let state = AppState::new(crate::db::Db::init(&dir).await.unwrap());
        let item = |title: &str| {
            let obj = serde_json::json!({ "title": title, "priority": 0 });
            build_create_item(0, obj.as_object().unwrap(), &mut vec![]).unwrap()
        };
        let id = register("整理任务", vec![item("原始标题"), item("不接受这条")]);
        let edits = vec![
            PreviewEdit {
                index: 0,
                field: "title".into(),
                value: Some("修改后标题".into()),
            },
            PreviewEdit {
                index: 0,
                field: "plannedAt".into(),
                value: Some("2026-02-30".into()),
            },
        ];
        assert!(ai_preview_edit(id.clone(), edits).is_err());
        assert_eq!(
            registry().lock().unwrap().map[&id].1.items[0].title,
            "原始标题"
        );
        let edited = ai_preview_edit(
            id.clone(),
            vec![
                PreviewEdit {
                    index: 0,
                    field: "title".into(),
                    value: Some("修改后标题".into()),
                },
                PreviewEdit {
                    index: 0,
                    field: "plannedAt".into(),
                    value: Some("2026-09-29".into()),
                },
                PreviewEdit {
                    index: 0,
                    field: "priority".into(),
                    value: Some("3".into()),
                },
            ],
        )
        .unwrap();
        assert_eq!(edited[0].title, "修改后标题");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tasks")
                .fetch_one(state.db.pool())
                .await
                .unwrap(),
            0
        );
        let result = ai_apply_impl(&state, id.clone(), Some(vec![0]))
            .await
            .unwrap();
        assert_eq!(result.created, 1);
        let (title, priority, has_time): (String, i64, i64) =
            sqlx::query_as("SELECT title, priority, has_planned_time FROM tasks")
                .fetch_one(state.db.pool())
                .await
                .unwrap();
        assert_eq!((title.as_str(), priority, has_time), ("修改后标题", 3, 0));
        assert!(ai_apply_impl(&state, id, None).await.is_err());
        let id = register("整理任务", vec![item("保留")]);
        for (field, value) in [("title", ""), ("priority", "4"), ("taskId", "other")] {
            assert!(ai_preview_edit(
                id.clone(),
                vec![PreviewEdit {
                    index: 0,
                    field: field.into(),
                    value: Some(value.into())
                }]
            )
            .is_err());
        }
        use chrono::TimeZone;
        for date in ["2024-02-01", "2024-02-29", "2024-03-01"] {
            let day = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
            let local = chrono::Local
                .from_local_datetime(&day.and_hms_opt(12, 0, 0).unwrap())
                .single()
                .unwrap();
            let at = crate::db::to_db_time(local.with_timezone(&chrono::Utc));
            sqlx::query("INSERT INTO tasks (id,title,status,completed_at,created_at,updated_at,actual_minutes) VALUES (?1,'总结测试','done',?2,?2,?2,20)").bind(date).bind(at).execute(state.db.pool()).await.unwrap();
        }
        let anchor = chrono::NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        let (s, e) = review_range("monthly", anchor).unwrap();
        let stats = crate::stats::collect_calendar_stats(&state, s, e)
            .await
            .unwrap();
        assert_eq!(stats["completedTotal"], 2);
        assert_eq!(stats["actualMinutes"], 40);
        assert_eq!(stats["days"], 29);
        let (s, e) = review_range("yearly", anchor).unwrap();
        let stats = crate::stats::collect_calendar_stats(&state, s, e)
            .await
            .unwrap();
        assert_eq!(stats["completedTotal"], 3);
        assert_eq!(stats["days"], 366);
        state.db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn review_supports_calendar_month_year_and_monday_week() {
        use chrono::NaiveDate;
        let day = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        let (s, e) = review_range("monthly", day).unwrap();
        assert_eq!(s, NaiveDate::from_ymd_opt(2024, 2, 1).unwrap());
        assert_eq!(e, NaiveDate::from_ymd_opt(2024, 3, 1).unwrap());
        assert_eq!((e - s).num_days(), 29);
        let (s, e) = review_range("yearly", day).unwrap();
        assert_eq!(s, NaiveDate::from_ymd_opt(2024, 1, 1).unwrap());
        assert_eq!(e, NaiveDate::from_ymd_opt(2025, 1, 1).unwrap());
        assert_eq!((e - s).num_days(), 366);
        let day = NaiveDate::from_ymd_opt(2026, 9, 28).unwrap();
        let (s, e) = review_range("weekly", day).unwrap();
        assert_eq!(s, day);
        assert_eq!(e, NaiveDate::from_ymd_opt(2026, 10, 5).unwrap());
        assert!(review_range("quarter", day).is_err());
    }
    use super::*;

    #[tokio::test]
    async fn apply_first_ai_subtask_to_empty_parent() {
        let dir = std::env::temp_dir().join(format!("lumen-ai-apply-{}", uuid::Uuid::now_v7()));
        let db = crate::db::Db::init(&dir).await.unwrap();
        let state = AppState::new(db);
        let now = crate::db::to_db_time(crate::db::utc_now());
        sqlx::query(
            "INSERT INTO tasks (id, title, status, created_at, updated_at, sort_order,
             occurrence_kind, is_exception) VALUES ('ai-parent', '父任务', 'todo', ?1, ?1, 0, 'single', 0)",
        )
        .bind(&now)
        .execute(state.db.pool())
        .await
        .unwrap();
        let preview_id = register(
            "拆分",
            vec![DiffItem {
                action: DiffAction::Create,
                task_id: None,
                title: "第一条子任务".into(),
                changes: vec![],
                payload: serde_json::json!({
                    "parentTaskId": "ai-parent",
                    "asSubtask": true
                }),
                note: None,
            }],
        );
        let applied = ai_apply_impl(&state, preview_id, None).await.unwrap();
        assert_eq!(applied.created, 1);
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM subtasks WHERE task_id = 'ai-parent'")
                .fetch_one(state.db.pool())
                .await
                .unwrap();
        assert_eq!(count, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn apply_ai_reschedule_marks_recurring_occurrence_as_exception() {
        let dir = std::env::temp_dir().join(format!("lumen-ai-recur-{}", uuid::Uuid::now_v7()));
        let db = crate::db::Db::init(&dir).await.unwrap();
        let state = AppState::new(db);
        let created = crate::recurrence_service::create_recurring_impl(
            &state,
            crate::recurrence_service::CreateRecurringInput {
                title: "AI 重复排程".into(),
                description: None,
                priority: None,
                project_id: None,
                category_id: None,
                estimated_minutes: None,
                tag_ids: vec![],
                rrule: "FREQ=DAILY".into(),
                tzid: Some("UTC".into()),
                dtstart_local: "2026-09-25T09:00:00".into(),
                has_start_time: Some(true),
                due_local: None,
                materialize_days: Some(2),
            },
        )
        .await
        .unwrap();
        let first = crate::recurrence_service::list_instances(&state, &created.series_id)
            .await
            .unwrap()
            .remove(0);
        let original_key = first.occurrence_key.clone();
        let preview_id = register(
            "排程",
            vec![DiffItem {
                action: DiffAction::Reschedule,
                task_id: Some(first.id.clone()),
                title: first.title.clone(),
                changes: vec![],
                payload: serde_json::json!({
                    "plannedAt": "2026-09-26T11:00:00.000Z",
                    "hasPlannedTime": true
                }),
                note: None,
            }],
        );
        let applied = ai_apply_impl(&state, preview_id, None).await.unwrap();
        assert_eq!(applied.updated, 1);
        let after: crate::models::Task = sqlx::query_as("SELECT * FROM tasks WHERE id = ?1")
            .bind(&first.id)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
        assert_eq!(
            after.planned_at.as_deref(),
            Some("2026-09-26T11:00:00.000Z")
        );
        assert_eq!(after.occurrence_key, original_key);
        assert_eq!(after.occurrence_kind.as_deref(), Some("exception"));
        assert_eq!(after.is_exception, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ------------------------- JSON 提取 -------------------------

    #[test]
    fn extracts_plain_json() {
        let v = extract_json(r#"{"a":1}"#).unwrap();
        assert_eq!(v["a"], 1);
    }

    /// 模型经常把 JSON 包在代码围栏里，必须能剥掉
    #[test]
    fn extracts_fenced_json() {
        let v = extract_json("```json\n{\"tasks\":[]}\n```").unwrap();
        assert!(v["tasks"].is_array());

        let v2 = extract_json("```\n{\"tasks\":[]}\n```").unwrap();
        assert!(v2["tasks"].is_array());
    }

    /// 前后带解释文字时取第一段平衡的 JSON
    #[test]
    fn extracts_json_from_surrounding_text() {
        let s = "好的，我整理如下：\n{\"tasks\":[{\"title\":\"写周报\"}]}\n希望有帮助。";
        let v = extract_json(s).unwrap();
        assert_eq!(v["tasks"][0]["title"], "写周报");
    }

    /// 字符串里出现花括号不能干扰平衡计数
    #[test]
    fn brace_inside_string_does_not_break_extraction() {
        let s = r#"{"title":"含 } 的标题","n":1}"#;
        let v = extract_json(s).unwrap();
        assert_eq!(v["title"], "含 } 的标题");
    }

    #[test]
    fn rejects_non_json() {
        assert!(extract_json("这不是 JSON").is_err());
        assert!(extract_json("").is_err());
        assert!(extract_json("{不完整的").is_err());
    }

    // ------------------------- 日期规范化 -------------------------

    /// 纯日期应按"仅日期"处理，不能被当成凌晨到期的精确时间（§4.3）
    #[test]
    fn date_only_is_marked_as_date_only() {
        let (dt, has_time) = normalize_model_datetime("截止时间", "2026-09-25").unwrap();
        assert!(dt.is_some());
        assert!(
            !has_time,
            "纯日期必须是「仅日期」，否则全天任务会被当成凌晨到期"
        );
    }

    #[test]
    fn full_iso_datetime_keeps_time() {
        let (dt, has_time) =
            normalize_model_datetime("计划时间", "2026-09-25T09:30:00+08:00").unwrap();
        assert!(has_time);
        assert_eq!(dt.unwrap(), "2026-09-25T01:30:00.000Z", "应转换为 UTC");
    }

    #[test]
    fn empty_or_null_means_unset() {
        assert_eq!(normalize_model_datetime("x", "").unwrap(), (None, false));
        assert_eq!(
            normalize_model_datetime("x", "null").unwrap(),
            (None, false)
        );
        assert_eq!(normalize_model_datetime("x", "   ").unwrap(), (None, false));
        // "null" 字符串是模型常见的表示方式
        assert_eq!(
            normalize_model_datetime("x", "NULL").unwrap(),
            (None, false)
        );
        assert_eq!(
            normalize_model_datetime("x", "Null").unwrap(),
            (None, false)
        );
        assert_eq!(
            normalize_model_datetime("x", "None").unwrap(),
            (None, false)
        );
        assert_eq!(normalize_model_datetime("x", "-").unwrap(), (None, false));
    }

    #[test]
    fn unparseable_date_is_rejected() {
        assert!(normalize_model_datetime("x", "明天").is_err());
        assert!(normalize_model_datetime("x", "2026-13-45").is_err());
        assert!(normalize_model_datetime("x", "next monday").is_err());
    }

    /// 输出必须是固定宽度 UTC，与全库格式一致
    #[test]
    fn normalized_output_is_fixed_width() {
        for raw in [
            "2026-09-25",
            "2026-09-25T09:30:00+08:00",
            "2026-09-25T09:30:00",
        ] {
            let (dt, _) = normalize_model_datetime("x", raw).unwrap();
            let s = dt.unwrap();
            assert_eq!(s.len(), 24, "{raw} → {s}");
            assert!(s.ends_with('Z'));
        }
    }

    // ------------------------- create 条目校验 -------------------------

    fn obj(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn create_requires_title() {
        let mut issues = Vec::new();
        let r = build_create_item(
            0,
            &obj(serde_json::json!({"dueAt": "2026-09-25"})),
            &mut issues,
        );
        assert!(r.is_none(), "缺少标题应被拒绝");
        assert!(issues.iter().any(|i| i.level == "error"));
    }

    #[test]
    fn create_rejects_overlong_title() {
        let mut issues = Vec::new();
        let long = "字".repeat(501);
        let r = build_create_item(0, &obj(serde_json::json!({"title": long})), &mut issues);
        assert!(r.is_none());
        assert!(issues.iter().any(|i| i.message.contains("过长")));
    }

    #[test]
    fn create_accepts_valid_item() {
        let mut issues = Vec::new();
        let r = build_create_item(
            0,
            &obj(serde_json::json!({
                "title": "写周报",
                "dueAt": "2026-09-25",
                "priority": 2,
                "estimatedMinutes": 45
            })),
            &mut issues,
        )
        .expect("应能构建");
        assert_eq!(r.title, "写周报");
        assert!(issues.is_empty(), "{issues:?}");
        // changes 应包含标题、截止时间、优先级、耗时
        assert!(r.changes.len() >= 3);
        assert_eq!(r.payload["priority"], 2);
    }

    /// 越界优先级收敛为 0 并给 warning，而不是让整条失败
    #[test]
    fn out_of_range_priority_becomes_warning() {
        let mut issues = Vec::new();
        let r = build_create_item(
            0,
            &obj(serde_json::json!({"title": "x", "priority": 9})),
            &mut issues,
        )
        .unwrap();
        assert_eq!(r.payload["priority"], 0);
        assert!(issues.iter().any(|i| i.level == "warning"));
    }

    /// 非法日期是 error（该条不能写入），而不是静默丢掉日期
    #[test]
    fn invalid_date_makes_item_unacceptable() {
        let mut issues = Vec::new();
        let _ = build_create_item(
            0,
            &obj(serde_json::json!({"title": "x", "dueAt": "下周三"})),
            &mut issues,
        );
        assert!(
            issues.iter().any(|i| i.level == "error"),
            "模型返回无法解析的日期时必须报错，不能静默忽略"
        );
    }

    // ------------------------- reschedule 条目校验 -------------------------

    fn index_of(pairs: &[(&str, &str, Option<&str>)]) -> HashMap<String, (String, Option<String>)> {
        pairs
            .iter()
            .map(|(id, t, p)| (id.to_string(), (t.to_string(), p.map(|s| s.to_string()))))
            .collect()
    }

    /// **任务 ID 核验**：模型可能编造不存在的 id（§6 明确要求核验）
    #[test]
    fn reschedule_rejects_unknown_task_id() {
        let idx = index_of(&[("t1", "真实任务", None)]);
        let mut issues = Vec::new();
        let r = build_reschedule_item(
            0,
            &obj(serde_json::json!({"taskId": "编造的id", "plannedAt": "2026-09-25"})),
            &idx,
            &mut issues,
        );
        assert!(r.is_none(), "不存在的任务 id 必须被拒绝");
        assert!(issues.iter().any(|i| i.message.contains("不存在")));
    }

    #[test]
    fn reschedule_requires_task_id() {
        let idx = index_of(&[]);
        let mut issues = Vec::new();
        assert!(build_reschedule_item(
            0,
            &obj(serde_json::json!({"plannedAt": "2026-09-25"})),
            &idx,
            &mut issues
        )
        .is_none());
    }

    #[test]
    fn reschedule_records_before_and_after() {
        let idx = index_of(&[("t1", "写周报", Some("2026-09-25T01:00:00.000Z"))]);
        let mut issues = Vec::new();
        let r = build_reschedule_item(
            0,
            &obj(serde_json::json!({"taskId": "t1", "plannedAt": "2026-09-26"})),
            &idx,
            &mut issues,
        )
        .expect("应能构建");
        let c = &r.changes[0];
        assert_eq!(c.before.as_deref(), Some("2026-09-25T01:00:00.000Z"));
        assert!(c.after.is_some(), "必须给出改动后的值供用户对照");
    }

    /// 改期到同一时刻没有意义，应提示但不阻止
    #[test]
    fn reschedule_to_same_time_warns_only() {
        let idx = index_of(&[("t1", "x", Some("2026-09-25T00:00:00.000Z"))]);
        let mut issues = Vec::new();
        let r = build_reschedule_item(
            0,
            &obj(serde_json::json!({"taskId": "t1", "plannedAt": "2026-09-25T00:00:00.000Z"})),
            &idx,
            &mut issues,
        );
        assert!(r.is_some(), "不应阻止");
        assert!(issues
            .iter()
            .any(|i| i.level == "warning" && i.message.contains("没有变化")));
    }

    // ------------------------- 重复检测 -------------------------

    fn mk_item(action: DiffAction, title: &str, planned: Option<&str>) -> DiffItem {
        DiffItem {
            action,
            task_id: Some(format!("id-{title}")),
            title: title.to_string(),
            changes: vec![],
            payload: serde_json::json!({ "plannedAt": planned }),
            note: None,
        }
    }

    #[test]
    fn detects_duplicate_creates() {
        let items = vec![
            mk_item(DiffAction::Create, "写周报", Some("2026-09-25")),
            mk_item(DiffAction::Create, "写周报", Some("2026-09-25")),
        ];
        let mut issues = Vec::new();
        detect_duplicates(&items, &mut issues);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("重复"));
    }

    /// 标题相同但时间不同不算重复——用户可能确实要做两次
    #[test]
    fn same_title_different_time_is_not_duplicate() {
        let items = vec![
            mk_item(DiffAction::Create, "喝水", Some("2026-09-25T01:00:00.000Z")),
            mk_item(DiffAction::Create, "喝水", Some("2026-09-25T05:00:00.000Z")),
        ];
        let mut issues = Vec::new();
        detect_duplicates(&items, &mut issues);
        assert!(issues.is_empty());
    }

    #[test]
    fn detects_reschedule_same_task_twice() {
        let mut a = mk_item(DiffAction::Reschedule, "A", Some("2026-09-25"));
        a.task_id = Some("t1".into());
        let mut b = mk_item(DiffAction::Reschedule, "A", Some("2026-09-26"));
        b.task_id = Some("t1".into());
        let mut issues = Vec::new();
        detect_duplicates(&[a, b], &mut issues);
        assert!(issues.iter().any(|i| i.message.contains("同一任务")));
    }

    // ------------------------- 预览注册表 -------------------------

    /// 预览是一次性的：确认（take）之后不能再确认第二次，
    /// 否则用户重复点击就会写入两遍。
    #[test]
    fn preview_can_only_be_taken_once() {
        let pid = register("测试", vec![mk_item(DiffAction::Create, "x", None)]);
        assert!(take(&pid).is_ok(), "首次取出应成功");
        let second = take(&pid);
        assert!(second.is_err(), "同一份预览不能被应用两次");
        let e = second.unwrap_err();
        assert!(
            e.message.contains("未找到") || e.hint.is_some(),
            "错误信息应说明预览已失效并提示重新生成"
        );
    }

    #[test]
    fn unknown_preview_id_is_rejected_with_hint() {
        let e = take("不存在的id").unwrap_err();
        assert!(e.hint.is_some(), "应提示用户重新生成");
        assert!(e.hint.unwrap().contains("重新生成"));
    }

    #[test]
    fn preview_keeps_items_and_capability() {
        let items = vec![
            mk_item(DiffAction::Create, "a", None),
            mk_item(DiffAction::Create, "b", None),
        ];
        let pid = register("整理任务", items);
        let (cap, got) = take(&pid).unwrap();
        assert_eq!(cap, "整理任务");
        assert_eq!(got.len(), 2);
    }

    // ------------------------- 数据范围说明 -------------------------

    /// §6 要求说明发送给模型的数据范围，且默认不发送备注
    #[test]
    fn scope_note_states_what_is_sent() {
        let n = scope_note(false, 12);
        assert!(n.contains("12"));
        assert!(n.contains("未发送"), "默认应明确说明备注未发送：{n}");
        assert!(n.contains("附件"));

        let y = scope_note(true, 3);
        assert!(y.contains("备注"), "开启后应说明备注已发送：{y}");
        // 无论哪种情况，附件都不发送
        assert!(y.contains("附件内容从不发送") || y.contains("附件"));
    }

    #[test]
    fn default_does_not_send_notes() {
        // 这是一条安全默认值：备注常含敏感信息
        const { assert!(!DEFAULT_SEND_NOTES) };
    }

    // ------------------------- 阈值与上限 -------------------------

    #[test]
    fn limits_are_sane() {
        const { assert!(MAX_ITEMS >= 10 && MAX_ITEMS <= 200) };
        // 预览不能长期驻留内存
        assert!(PREVIEW_TTL.as_secs() >= 60);
        assert!(PREVIEW_TTL.as_secs() <= 24 * 3600);
    }

    // ------------------------- 动作类型序列化 -------------------------

    /// 界面按 snake_case 判别动作，改动会静默破坏前端分支
    #[test]
    fn diff_action_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&DiffAction::Create).unwrap(),
            "\"create\""
        );
        assert_eq!(
            serde_json::to_string(&DiffAction::Reschedule).unwrap(),
            "\"reschedule\""
        );
        assert_eq!(
            serde_json::to_string(&DiffAction::Update).unwrap(),
            "\"update\""
        );
    }

    /// payload 不应出现在返回给前端的 JSON 里（它只是 apply 用的中间数据）
    #[test]
    fn payload_is_hidden_from_frontend() {
        let it = mk_item(DiffAction::Create, "x", None);
        let v = serde_json::to_value(&it).unwrap();
        assert!(
            v.get("payload").is_none(),
            "payload 含内部字段，不应序列化给前端：{v}"
        );
        assert!(v.get("title").is_some());
    }
}
