//! 重复系列的业务层（任务书 §5）。
//!
//! ## 数据模型（三层，与任务书要求一一对应）
//!
//! ```text
//! task_series          规则 + 时区 + DTSTART + rule_version
//!   ├─ task_series_segments   规则分段：从某次起改用新规则/新字段（"此次及以后"）
//!   ├─ task_series_skips      单次跳过：该次不发生（"单次取消"）
//!   └─ tasks (series_id)      实例：真实任务记录，含 occurrence_key
//!                              occurrence_kind = generated（规则生成）
//!                                              | exception（用户单独改过）
//! ```
//!
//! ## 稳定身份
//!
//! 实例的身份是 **`(series_id, occurrence_key)`**，其中 `occurrence_key` 是该次的
//! **原始计划发生时间**（UTC）。用户把某次改期到别的日子后，`occurrence_key`
//! 保持不变，因此：
//! - 不会在旧日期再生成一个副本（唯一索引直接保证）；
//! - "同一次"的概念在改期、改标题、改提醒后依然成立；
//! - 分段与跳过都能精确指认"是哪一次"。
//!
//! ## 惰性展开
//!
//! 绝不预先复制无限多的未来任务。`materialize_range` 只把**请求范围内**
//! 应当发生的实例落库，且对已存在的实例不做任何改动（保护用户的修改与完成记录）。

use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::commands::AppState;
use crate::db::{to_db_time, utc_now};
use crate::error::{AppError, AppResult};
use crate::models::Task;
use crate::recurrence::{
    occurrences_to_utc, parse_local_date, parse_local_datetime, EndCondition, Freq, Occurrence,
    RecurrenceRule, SetPos,
};

/// 单次物化的实例数量上限，防止请求超大范围时一次性写入过多行
const MAX_MATERIALIZE: usize = 500;

/// 范围选择的三种语义（§5 强制要求）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditScope {
    /// 仅此次
    ThisOnly,
    /// 此次及以后
    ThisAndFuture,
    /// 整个系列
    WholeSeries,
}

/// SQL 绑定值的极简表达。
///
/// 用于"手动拼 SET 片段"的场景：片段里用 `?` 占位，值按同顺序放进
/// `Vec<BindValue>`，绑定时按变体分派。比 `QueryBuilder` 的链式调用
/// 更朴素，但顺序完全显式，不容易出现分隔符错位。
enum BindValue {
    /// 文本
    S(String),
    /// 整数（SQLite 无布尔类型，布尔也走这里）
    I(i64),
}

/// 系列视图
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    pub id: String,
    pub rrule: String,
    pub tzid: String,
    pub dtstart_local: String,
    pub has_start_time: i64,
    pub recurrence_end_kind: String,
    pub recurrence_until: Option<String>,
    pub recurrence_count: Option<i64>,
    pub terminated_from_occurrence_key: Option<String>,
    pub rule_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

fn end_columns(rule: &RecurrenceRule) -> (&'static str, Option<&str>, Option<i64>) {
    match &rule.end {
        EndCondition::Never => ("never", None, None),
        EndCondition::Until { date } => ("until", Some(date.as_str()), None),
        EndCondition::Count { count } => ("count", None, Some(*count)),
    }
}

/// 规则分段
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub id: String,
    pub series_id: String,
    pub rule_version: i64,
    pub effective_from_occurrence: String,
    pub new_rrule: Option<String>,
    pub new_tzid: Option<String>,
    pub override_title: Option<String>,
    pub override_description: Option<String>,
    pub override_priority: Option<i64>,
    pub override_project_id: Option<String>,
    pub override_category_id: Option<String>,
    pub override_estimated_minutes: Option<i64>,
}

/// 创建重复任务的输入
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRecurringInput {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub estimated_minutes: Option<i64>,
    #[serde(default)]
    pub tag_ids: Vec<String>,

    /// RRULE 字符串，例如 `FREQ=WEEKLY;BYDAY=MO,WE,FR`
    pub rrule: String,
    /// 时区标识，由桌面客户端读取系统 IANA 时区；IPC 缺省按 UTC
    #[serde(default)]
    pub tzid: Option<String>,
    /// 首次发生的本地墙上时间 `YYYY-MM-DDTHH:MM:SS`
    pub dtstart_local: String,
    /// 是否含具体时刻
    #[serde(default)]
    pub has_start_time: Option<bool>,
    /// 是否同时设置截止时间（可选）
    #[serde(default)]
    pub due_local: Option<String>,
    /// 立即物化多少天内的实例（默认 90 天）
    #[serde(default)]
    pub materialize_days: Option<i64>,
}

/// 创建结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRecurringResult {
    pub series_id: String,
    /// 物化出的实例数量
    pub created_count: usize,
    /// 规则的可读描述（供界面回显）
    pub description: String,
    /// 边界策略说明（若该规则涉及月末等情形）
    pub edge_note: Option<String>,
    pub warning: Option<String>,
    pub needs_repair: bool,
}

// =============================================================================
// 内部工具
// =============================================================================

/// 读取系列
async fn get_series(state: &AppState, id: &str) -> AppResult<Series> {
    let s = sqlx::query_as::<_, Series>("SELECT * FROM task_series WHERE id = ?1")
        .bind(id)
        .fetch_optional(state.db.pool())
        .await?;
    s.ok_or_else(|| AppError::not_found("重复系列", id))
}

/// 把规则与分段合成为不同版本的规则列表。
///
/// 返回 `(rule_version, 生效起点, 规则, 字段覆盖)`，按生效起点升序。
/// 物化时对每个发生找出"对它生效的那一段"。
type RuleTimeline = Vec<(i64, Option<String>, RecurrenceRule, FieldOverride)>;

fn build_rule_timeline(base: &Series, segments: &[Segment]) -> AppResult<RuleTimeline> {
    let mut out = Vec::new();

    // 第 1 段永远存在：系列本身的规则，从 dtstart 起生效
    let base_rule = RecurrenceRule::from_rrule_string(
        &base.rrule,
        &base.tzid,
        &base.dtstart_local,
        base.has_start_time == 1,
    )?;
    out.push((1, None, base_rule.clone(), FieldOverride::default()));
    // A later edit supersedes older edits at and after its anchor. Sorting by
    // anchor alone would let a previously scheduled future edit revive after
    // a newer "whole series" change made from an earlier occurrence.
    let mut ordered = segments.to_vec();
    ordered.sort_by_key(|seg| seg.rule_version);
    for seg in &ordered {
        out.retain(|(_, anchor, _, _)| {
            anchor
                .as_deref()
                .is_none_or(|old| old < seg.effective_from_occurrence.as_str())
        });
        let previous = out.last().expect("base recurrence rule is always present");
        let current_rule = previous.2.clone();
        let mut current_override = previous.3.clone();
        let active_tzid = seg.new_tzid.as_deref().unwrap_or(&current_rule.tzid);
        let rule = if seg.new_rrule.is_some() || seg.new_tzid.is_some() {
            let anchor = segment_anchor_local(seg, active_tzid, &current_rule.dtstart_local)?;
            let rrule = match &seg.new_rrule {
                Some(r) => r.clone(),
                None => current_rule.to_rrule_string()?,
            };
            RecurrenceRule::from_rrule_string(
                &rrule,
                active_tzid,
                &anchor,
                base.has_start_time == 1,
            )?
        } else {
            current_rule.clone()
        };
        if let Some(title) = &seg.override_title {
            current_override.title = Some(title.clone());
        }
        if let Some(description) = &seg.override_description {
            current_override.description = Some(description.clone());
        }
        if let Some(priority) = seg.override_priority {
            current_override.priority = Some(priority);
        }
        if let Some(project_id) = &seg.override_project_id {
            current_override.project_id = Some(project_id.clone());
        }
        if let Some(category_id) = &seg.override_category_id {
            current_override.category_id = Some(category_id.clone());
        }
        if let Some(estimated) = seg.override_estimated_minutes {
            current_override.estimated_minutes = Some(estimated);
        }
        out.push((
            seg.rule_version,
            Some(seg.effective_from_occurrence.clone()),
            rule,
            current_override.clone(),
        ));
    }

    Ok(out)
}

/// 分段生效起点的本地日期串
fn segment_anchor_local(seg: &Segment, tzid: &str, previous_start: &str) -> AppResult<String> {
    let dt =
        chrono::DateTime::parse_from_rfc3339(&seg.effective_from_occurrence).map_err(|_| {
            AppError::validation(format!(
                "分段生效时间格式不正确：{}",
                seg.effective_from_occurrence
            ))
        })?;
    let tz: chrono_tz::Tz = tzid
        .parse()
        .map_err(|_| AppError::validation("重复系列时区无效"))?;
    // A timezone change keeps the wall-clock time of the prior schedule.
    // The UTC anchor still determines which segment owns an occurrence.
    let local = dt.with_timezone(&tz);
    let time = if seg.new_tzid.is_some() {
        parse_local_datetime(previous_start)?.time()
    } else {
        local.time()
    };
    Ok(local
        .date_naive()
        .and_time(time)
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string())
}

/// 分段携带的字段覆盖
#[derive(Debug, Clone, Default)]
struct FieldOverride {
    title: Option<String>,
    description: Option<String>,
    priority: Option<i64>,
    project_id: Option<String>,
    category_id: Option<String>,
    estimated_minutes: Option<i64>,
}

#[derive(Debug, sqlx::FromRow)]
struct SeriesTemplate {
    title: String,
    description: String,
    note_md: String,
    link_url: Option<String>,
    priority: i64,
    project_id: Option<String>,
    category_id: Option<String>,
    estimated_minutes: Option<i64>,
}

/// 物化：确保范围内所有应当发生的实例都已存在于 tasks 表。
///
/// 语义保证：
/// - **已存在的实例一律不改动**（保护用户的完成记录与单独修改）；
/// - 被跳过的发生不会创建实例；
/// - 分段规则按"该次落在哪一段"生效；
/// - 只创建 `occurrence_kind = 'generated'` 的实例，例外由用户操作产生。
async fn materialize_range(
    state: &AppState,
    series_id: &str,
    range_start_utc: &str,
    range_end_utc: &str,
    limit: usize,
) -> AppResult<usize> {
    let series = get_series(state, series_id).await?;

    let seg_rows = sqlx::query_as::<_, SeriesSegmentRow>(
        "SELECT * FROM task_series_segments WHERE series_id = ?1 ORDER BY effective_from_occurrence ASC",
    )
    .bind(series_id)
    .fetch_all(state.db.pool())
    .await?;
    let segments: Vec<Segment> = seg_rows.into_iter().map(Segment::from).collect();

    let timeline = build_rule_timeline(&series, &segments)?;

    // 已有的实例 key（含例外），用于跳过重复创建
    let existing: Vec<(String,)> = sqlx::query_as(
        "SELECT occurrence_key FROM tasks WHERE series_id = ?1 AND occurrence_key IS NOT NULL",
    )
    .bind(series_id)
    .fetch_all(state.db.pool())
    .await?;
    let existing_keys: std::collections::HashSet<String> =
        existing.into_iter().map(|(k,)| k).collect();

    // 已跳过的发生
    let skips: Vec<(String,)> =
        sqlx::query_as("SELECT occurrence_key FROM task_series_skips WHERE series_id = ?1")
            .bind(series_id)
            .fetch_all(state.db.pool())
            .await?;
    let skip_keys: std::collections::HashSet<String> = skips.into_iter().map(|(k,)| k).collect();

    let start_utc = chrono::DateTime::parse_from_rfc3339(range_start_utc)
        .map_err(|_| AppError::validation("物化起点不是有效 UTC 日期时间"))?
        .with_timezone(&chrono::Utc);
    let end_utc = chrono::DateTime::parse_from_rfc3339(range_end_utc)
        .map_err(|_| AppError::validation("物化终点不是有效 UTC 日期时间"))?
        .with_timezone(&chrono::Utc);
    if end_utc <= start_utc {
        return Err(AppError::validation("物化终点必须晚于起点"));
    }
    let mut created = 0usize;
    let to_utc = |local: &str, tzid: &str| -> AppResult<String> {
        let dt = parse_local_datetime(local)?;
        let pair = occurrences_to_utc(
            &[Occurrence {
                local: dt,
                index: 0,
            }],
            tzid,
        );
        Ok(to_db_time(pair[0].1))
    };

    // Each segment owns [anchor, next anchor). Expand only near the requested
    // range, never only the first N occurrences from DTSTART.
    let mut candidates: Vec<(String, i64, Occurrence, FieldOverride)> = Vec::new();
    for (i, (version, anchor, rule, ov)) in timeline.iter().enumerate() {
        let next_anchor = timeline.get(i + 1).and_then(|segment| segment.1.as_deref());
        let tz: chrono_tz::Tz = rule
            .tzid
            .parse()
            .map_err(|_| AppError::validation("重复系列时区无效"))?;
        let local_start = start_utc.with_timezone(&tz).naive_local() - chrono::Duration::days(1);
        let local_end = end_utc.with_timezone(&tz).naive_local() + chrono::Duration::days(1);
        for o in rule.expand_between(local_start, local_end, limit + 1)? {
            // The persisted end columns constrain the active rule too. Old
            // segments retain their historical RRULE end condition.
            if i + 1 == timeline.len()
                && ((series.recurrence_end_kind == "until"
                    && series
                        .recurrence_until
                        .as_deref()
                        .is_some_and(|end| o.local.date().to_string().as_str() > end))
                    || (series.recurrence_end_kind == "count"
                        && series.recurrence_count.is_some_and(|end| o.index > end)))
            {
                continue;
            }
            let key = to_utc(&o.local.to_string(), &rule.tzid)?;
            if key.as_str() < range_start_utc
                || key.as_str() >= range_end_utc
                || series
                    .terminated_from_occurrence_key
                    .as_deref()
                    .is_some_and(|cutoff| key.as_str() >= cutoff)
                || anchor.as_deref().is_some_and(|a| key.as_str() < a)
                || next_anchor.is_some_and(|a| key.as_str() >= a)
            {
                continue;
            }
            candidates.push((key, *version, o, ov.clone()));
        }
    }
    // A boundary can coincide with both rules; the newer version wins.
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut seen: std::collections::HashSet<String> = Default::default();
    candidates.retain(|(k, _, _, _)| seen.insert(k.clone()));

    let template = sqlx::query_as::<_, SeriesTemplate>(
        "SELECT title, description, note_md, link_url, priority, project_id,
                category_id, estimated_minutes
         FROM task_series_template WHERE series_id = ?1",
    )
    .bind(series_id)
    .fetch_optional(state.db.pool())
    .await?
    .ok_or_else(|| AppError::conflict("重复系列缺少持久化模板，无法安全物化"))?;

    let mut tx = state.db.pool().begin().await?;

    for (key, _ver, occ, ov) in candidates {
        if existing_keys.contains(&key) || skip_keys.contains(&key) {
            continue;
        }
        if created >= limit {
            return Err(AppError::validation(
                "本次物化范围内的实例超过安全上限，请缩小日期范围",
            ));
        }

        let id = uuid::Uuid::now_v7().to_string();
        let now = to_db_time(utc_now());

        let title = ov.title.as_deref().unwrap_or(&template.title);
        let description = ov.description.as_deref().unwrap_or(&template.description);
        let note_md = &template.note_md;
        let priority = ov.priority.unwrap_or(template.priority);
        let project_id = ov.project_id.as_ref().or(template.project_id.as_ref());
        let category_id = ov.category_id.as_ref().or(template.category_id.as_ref());
        let estimated = ov.estimated_minutes.or(template.estimated_minutes);
        let link_url = &template.link_url;

        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO tasks (
                id, title, description, note_md, link_url,
                status, priority, project_id, category_id,
                planned_at, has_planned_time, due_at, has_due_time,
                estimated_minutes, actual_minutes,
                completed_at, created_at, updated_at,
                sort_order, is_pinned, is_favorite,
                series_id, occurrence_key, occurrence_index, occurrence_kind, is_exception
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                'todo', ?6, ?7, ?8,
                ?9, ?10, NULL, 0,
                ?11, 0,
                NULL, ?12, ?12,
                0, 0, 0,
                ?13, ?14, ?15, 'generated', 0
             )",
        )
        .bind(&id)
        .bind(title)
        .bind(description)
        .bind(note_md)
        .bind(link_url)
        .bind(priority)
        .bind(project_id)
        .bind(category_id)
        .bind(&key)
        .bind(series.has_start_time)
        .bind(estimated)
        .bind(&now)
        .bind(series_id)
        .bind(&key)
        .bind(occ.index)
        .execute(&mut *tx)
        .await?;
        if inserted.rows_affected() == 0 {
            continue;
        }

        sqlx::query(
            "INSERT OR IGNORE INTO task_tags (task_id, tag_id)
             SELECT ?1, tag_id FROM task_series_tags WHERE series_id = ?2",
        )
        .bind(&id)
        .bind(series_id)
        .execute(&mut *tx)
        .await?;
        created += 1;
    }

    tx.commit().await?;
    if created > 0 {
        log::info!("系列 {series_id} 物化了 {created} 个实例");
    }
    Ok(created)
}

/// Resume a committed rebuild in bounded chunks. The cursor is durable, so a
/// crash or SQLite error cannot silently turn a partial rebuild into success.
async fn regenerate_pending(state: &AppState, series_id: &str) -> AppResult<usize> {
    let pending: Option<(String, String)> = sqlx::query_as(
        "SELECT range_start_utc, range_end_utc FROM task_series_rebuilds WHERE series_id = ?1",
    )
    .bind(series_id)
    .fetch_optional(state.db.pool())
    .await?;
    let Some((start, end)) = pending else {
        return Ok(0);
    };
    let mut cursor = chrono::DateTime::parse_from_rfc3339(&start)
        .map_err(|_| AppError::internal("待重建起点损坏"))?
        .with_timezone(&chrono::Utc);
    let finish = chrono::DateTime::parse_from_rfc3339(&end)
        .map_err(|_| AppError::internal("待重建终点损坏"))?
        .with_timezone(&chrono::Utc);
    let mut total = 0usize;
    while cursor < finish {
        let next = (cursor + chrono::Duration::days(90)).min(finish);
        let from = to_db_time(cursor);
        let to = to_db_time(next);
        match materialize_range(state, series_id, &from, &to, MAX_MATERIALIZE).await {
            Ok(n) => {
                total += n;
                sqlx::query(
                    "UPDATE task_series_rebuilds SET range_start_utc = ?1, last_error = NULL
                     WHERE series_id = ?2",
                )
                .bind(&to)
                .bind(series_id)
                .execute(state.db.pool())
                .await?;
                cursor = next;
            }
            Err(e) => {
                sqlx::query("UPDATE task_series_rebuilds SET last_error = ?1 WHERE series_id = ?2")
                    .bind(e.to_string())
                    .bind(series_id)
                    .execute(state.db.pool())
                    .await?;
                return Err(AppError::internal(format!(
                    "重复系列重建尚未完成，维护任务将重试：{e}"
                )));
            }
        }
    }
    sqlx::query("DELETE FROM task_series_rebuilds WHERE series_id = ?1")
        .bind(series_id)
        .execute(state.db.pool())
        .await?;
    Ok(total)
}

/// 分段行 → 视图
#[derive(Debug, sqlx::FromRow)]
struct SeriesSegmentRow {
    id: String,
    series_id: String,
    rule_version: i64,
    effective_from_occurrence: String,
    new_rrule: Option<String>,
    new_tzid: Option<String>,
    override_title: Option<String>,
    override_description: Option<String>,
    override_priority: Option<i64>,
    override_project_id: Option<String>,
    override_category_id: Option<String>,
    override_estimated_minutes: Option<i64>,
}

impl From<SeriesSegmentRow> for Segment {
    fn from(r: SeriesSegmentRow) -> Self {
        Self {
            id: r.id,
            series_id: r.series_id,
            rule_version: r.rule_version,
            effective_from_occurrence: r.effective_from_occurrence,
            new_rrule: r.new_rrule,
            new_tzid: r.new_tzid,
            override_title: r.override_title,
            override_description: r.override_description,
            override_priority: r.override_priority,
            override_project_id: r.override_project_id,
            override_category_id: r.override_category_id,
            override_estimated_minutes: r.override_estimated_minutes,
        }
    }
}

// =============================================================================
// 命令：创建
// =============================================================================

/// 创建重复任务的**业务实现**。
///
/// 拆成 `*_impl(state: &AppState, ...)` 是为了让集成测试能直接调用，
/// 而不必构造 Tauri 的 `State`（那需要一个完整的 App 实例）。
/// 紧随其后的 `#[tauri::command]` 函数只把它接到 IPC 上。
///
/// 同时创建系列、首个实例（作为后续实例的字段模板）。
pub async fn create_recurring_impl(
    state: &AppState,
    input: CreateRecurringInput,
) -> AppResult<CreateRecurringResult> {
    let db = &state.db;

    let title = input.title.trim();
    if title.is_empty() {
        return Err(AppError::validation("标题不能为空"));
    }
    if title.chars().count() > 500 {
        return Err(AppError::validation("标题不能超过 500 个字符"));
    }

    let tzid = input.tzid.clone().unwrap_or_else(|| {
        // Frontend sends the system IANA zone. UTC is the unambiguous fallback
        // for direct IPC clients that omit it.
        "UTC".to_string()
    });
    let has_start_time = input.has_start_time.unwrap_or(true);

    // 规则必须能解析，且 dtstart 合法
    let rule = RecurrenceRule::from_rrule_string(
        &input.rrule,
        &tzid,
        &input.dtstart_local,
        has_start_time,
    )?;

    let now = to_db_time(utc_now());
    let series_id = uuid::Uuid::now_v7().to_string();

    // 结束条件拆成三列存储，便于查询与校验互斥
    let (end_kind, end_until, end_count) = end_columns(&rule);

    let mut tx = db.pool().begin().await?;

    sqlx::query(
        "INSERT INTO task_series
            (id, rrule, tzid, dtstart_local, has_start_time,
             recurrence_end_kind, recurrence_until, recurrence_count,
             rule_version, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?9)",
    )
    .bind(&series_id)
    .bind(&input.rrule)
    .bind(&tzid)
    .bind(&input.dtstart_local)
    .bind(has_start_time as i64)
    .bind(end_kind)
    .bind(end_until)
    .bind(end_count)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO task_series_template
         (series_id, title, description, note_md, link_url, priority,
          project_id, category_id, estimated_minutes)
         VALUES (?1, ?2, ?3, '', NULL, ?4, ?5, ?6, ?7)",
    )
    .bind(&series_id)
    .bind(title)
    .bind(input.description.as_deref().unwrap_or(""))
    .bind(input.priority.unwrap_or(0))
    .bind(input.project_id.as_deref().filter(|s| !s.is_empty()))
    .bind(input.category_id.as_deref().filter(|s| !s.is_empty()))
    .bind(input.estimated_minutes)
    .execute(&mut *tx)
    .await?;

    // 首个实例：其 occurrence_key 就是首次发生时刻
    let first_occ = rule.expand(1)?.into_iter().next().ok_or_else(|| {
        AppError::validation("该重复规则不会产生任何发生，请检查规则")
            .with_hint("例如「每年 2 月 30 日」这样的规则永远不会发生")
    })?;

    let pair = occurrences_to_utc(std::slice::from_ref(&first_occ), &tzid);
    let first_utc = to_db_time(pair[0].1);

    let task_id = uuid::Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO tasks (
            id, title, description, priority, project_id, category_id,
            status, planned_at, has_planned_time,
            estimated_minutes, created_at, updated_at,
            sort_order, series_id, occurrence_key, occurrence_index, occurrence_kind, is_exception
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            'todo', ?7, ?8,
            ?9, ?10, ?10,
            0, ?11, ?7, ?12, 'generated', 0
         )",
    )
    .bind(&task_id)
    .bind(title)
    .bind(input.description.clone().unwrap_or_default())
    .bind(input.priority.unwrap_or(0))
    .bind(input.project_id.as_deref().filter(|s| !s.is_empty()))
    .bind(input.category_id.as_deref().filter(|s| !s.is_empty()))
    .bind(&first_utc)
    .bind(has_start_time as i64)
    .bind(input.estimated_minutes)
    .bind(&now)
    .bind(&series_id)
    .bind(first_occ.index)
    .execute(&mut *tx)
    .await?;

    // 标签
    for tid in input.tag_ids.iter().filter(|s| !s.is_empty()) {
        sqlx::query("INSERT OR IGNORE INTO task_series_tags (series_id, tag_id) VALUES (?1, ?2)")
            .bind(&series_id)
            .bind(tid)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT OR IGNORE INTO task_tags (task_id, tag_id) VALUES (?1, ?2)")
            .bind(&task_id)
            .bind(tid)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;

    // 物化后续实例（默认 90 天）。
    //
    // 起点必须从**首个发生时刻**起算，而不是 utc_now()：
    // dtstart 可能在过去（用户补录一个已开始的习惯），也可能在未来
    // （提前安排好接下来的安排）。以 first_utc 为基准两种情况都正确，
    // 而以 now 为基准会在 dtstart 已过时把范围算短、导致实例缺失。
    let days = input.materialize_days.unwrap_or(90).clamp(1, 730);
    let first_dt = chrono::DateTime::parse_from_rfc3339(&first_utc)
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| utc_now());
    let end_range = to_db_time(first_dt + chrono::Duration::days(days));
    // 物化失败必须**暴露出来**，不能静默吞掉：
    // 用户会以为"创建成功"，实际日历里只有第一个实例，且不知道原因。
    let (created, warning) =
        match materialize_range(state, &series_id, &first_utc, &end_range, MAX_MATERIALIZE).await {
            Ok(n) => (n, None),
            Err(e) => {
                log::error!("系列 {series_id} 物化失败：{e}");
                (
                    0,
                    Some(format!(
                        "系列已创建，但后续发生尚未生成：{e}。后台维护会重试。"
                    )),
                )
            }
        };

    log::info!(
        "已创建重复系列 {series_id}（{}），共物化 {} 个实例",
        rule.describe(),
        created
    );

    Ok(CreateRecurringResult {
        series_id,
        created_count: created,
        description: rule.describe(),
        edge_note: rule.edge_policy_note(),
        needs_repair: warning.is_some(),
        warning,
    })
}

/// 创建重复任务（IPC 入口）
#[tauri::command]
pub async fn recurring_create(
    state: State<'_, AppState>,
    input: CreateRecurringInput,
) -> AppResult<CreateRecurringResult> {
    create_recurring_impl(&state, input).await
}

/// 展开预览：接下来 N 次发生的本地日期（§5 要求"提供规则预览"）
#[tauri::command]
pub async fn recurring_preview(
    rrule: String,
    tzid: Option<String>,
    dtstart_local: String,
    has_start_time: Option<bool>,
    count: Option<usize>,
) -> AppResult<Vec<String>> {
    let tz = tzid.unwrap_or_else(|| "UTC".to_string());
    let rule = RecurrenceRule::from_rrule_string(
        &rrule,
        &tz,
        &dtstart_local,
        has_start_time.unwrap_or(true),
    )?;
    let n = count.unwrap_or(10).clamp(1, 50);
    let occ = rule.expand(n)?;
    let utc = occurrences_to_utc(&occ, &tz);

    Ok(occ
        .iter()
        .zip(utc.iter())
        .map(|(o, (_, u))| {
            format!(
                "{}（UTC {}）",
                o.local.format("%Y-%m-%d %H:%M"),
                u.format("%Y-%m-%d %H:%M")
            )
        })
        .collect())
}

/// 若某个本地日期时间落在范围内，则物化对应系列的实例。
///
/// 供 `task_list` / 日历查询调用，实现"按显示范围惰性展开"（§5）。
#[tauri::command]
pub async fn recurring_materialize(
    state: State<'_, AppState>,
    series_id: String,
    range_start_utc: String,
    range_end_utc: String,
) -> AppResult<usize> {
    recurring_materialize_inner(&state, series_id, range_start_utc, range_end_utc).await
}

/// 物化封装（业务实现，供测试与命令共用）
pub async fn recurring_materialize_inner(
    state: &AppState,
    series_id: String,
    range_start_utc: String,
    range_end_utc: String,
) -> AppResult<usize> {
    materialize_range(
        state,
        &series_id,
        &range_start_utc,
        &range_end_utc,
        MAX_MATERIALIZE,
    )
    .await
}

/// Repair and extend every active series before a view queries its date range.
/// Repeated calls are idempotent because materialization uses stable occurrence keys.
#[tauri::command]
pub async fn recurring_ensure_range(
    state: State<'_, AppState>,
    range_start_utc: String,
    range_end_utc: String,
) -> AppResult<usize> {
    let ids: Vec<(String,)> = sqlx::query_as("SELECT id FROM task_series ORDER BY id")
        .fetch_all(state.db.pool())
        .await?;
    let mut total = 0usize;
    for (id,) in ids {
        total += regenerate_pending(&state, &id).await?;
        total += materialize_range(
            &state,
            &id,
            &range_start_utc,
            &range_end_utc,
            MAX_MATERIALIZE,
        )
        .await?;
    }
    Ok(total)
}

/// 读取系列及其分段（界面展示规则与"从某次起改用新规则"的历史）
#[tauri::command]
pub async fn recurring_get(
    state: State<'_, AppState>,
    series_id: String,
) -> AppResult<serde_json::Value> {
    let series = get_series(&state, &series_id).await?;
    let rows = sqlx::query_as::<_, SeriesSegmentRow>(
        "SELECT * FROM task_series_segments WHERE series_id = ?1 ORDER BY effective_from_occurrence ASC",
    )
    .bind(&series_id)
    .fetch_all(state.db.pool())
    .await?;

    let segments: Vec<Segment> = rows.into_iter().map(Segment::from).collect();
    let timeline = build_rule_timeline(&series, &segments)?;
    let now = to_db_time(utc_now());
    let rule = timeline
        .iter()
        .rev()
        .find(|(_, anchor, _, _)| anchor.as_deref().is_none_or(|key| key <= now.as_str()))
        .map(|(_, _, rule, _)| rule)
        .ok_or_else(|| AppError::conflict("重复系列没有可用规则"))?;

    let skips: Vec<(String,)> = sqlx::query_as(
        "SELECT occurrence_key FROM task_series_skips WHERE series_id = ?1 ORDER BY occurrence_key",
    )
    .bind(&series_id)
    .fetch_all(state.db.pool())
    .await?;
    let next_future: Option<String> = sqlx::query_scalar(
        "SELECT MIN(occurrence_key) FROM tasks
         WHERE series_id = ?1 AND occurrence_key >= ?2 AND deleted_at IS NULL",
    )
    .bind(&series_id)
    .bind(&now)
    .fetch_one(state.db.pool())
    .await?;

    Ok(serde_json::json!({
        "series": series,
        "rule": rule,
        "description": rule.describe(),
        "edgeNote": rule.edge_policy_note(),
        "nextFutureOccurrenceKey": next_future,
        "segments": segments,
        "skippedOccurrences": skips.into_iter().map(|(k,)| k).collect::<Vec<_>>(),
    }))
}

/// 单个发生的可读预览（含 UTC 与序号），供界面在改范围前提示影响
#[tauri::command]
pub async fn recurring_occurrences(
    state: State<'_, AppState>,
    series_id: String,
    count: Option<usize>,
) -> AppResult<Vec<serde_json::Value>> {
    let series = get_series(&state, &series_id).await?;
    let rule = RecurrenceRule::from_rrule_string(
        &series.rrule,
        &series.tzid,
        &series.dtstart_local,
        series.has_start_time == 1,
    )?;
    let n = count.unwrap_or(10).clamp(1, 50);
    let occ = rule.expand(n)?;
    let utc = occurrences_to_utc(&occ, &series.tzid);

    Ok(occ
        .iter()
        .zip(utc.iter())
        .map(|(o, (_, u))| {
            serde_json::json!({
                "index": o.index,
                "local": o.local.format("%Y-%m-%d %H:%M:%S").to_string(),
                "utc": to_db_time(*u),
            })
        })
        .collect())
}

/// The sole rebuild-safety decision for a materialized occurrence. The
/// `is_user_modified` bit records past instance actions; the related-row
/// checks protect data written through every child-table IPC, including
/// direct calls that never touch the parent task row.
async fn is_occurrence_rebuild_safe(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
) -> AppResult<bool> {
    let safe: i64 = sqlx::query_scalar(
        "SELECT CASE WHEN
             t.series_id IS NOT NULL AND t.occurrence_kind = 'generated'
             AND t.is_exception = 0 AND t.is_user_modified = 0
             AND t.deleted_at IS NULL AND t.status = 'todo'
             AND t.completed_at IS NULL AND t.actual_minutes = 0
             AND t.is_pinned = 0 AND t.is_favorite = 0
             AND t.planned_at IS t.occurrence_key AND t.due_at IS NULL
             AND t.has_due_time = 0 AND t.period_type = 'none'
             AND t.has_planned_time = s.has_start_time
             AND NOT EXISTS (SELECT 1 FROM subtasks x WHERE x.task_id = t.id)
             AND NOT EXISTS (SELECT 1 FROM attachments x WHERE x.task_id = t.id)
             AND NOT EXISTS (SELECT 1 FROM reminders x WHERE x.task_id = t.id)
             AND NOT EXISTS (SELECT 1 FROM focus_sessions x WHERE x.task_id = t.id)
             AND NOT EXISTS (SELECT 1 FROM task_dependencies x
                             WHERE x.task_id = t.id OR x.depends_on_id = t.id)
             AND NOT EXISTS (
               SELECT 1 FROM task_tags tt WHERE tt.task_id = t.id
               AND NOT EXISTS (SELECT 1 FROM task_series_tags st
                               WHERE st.series_id = t.series_id AND st.tag_id = tt.tag_id))
             AND NOT EXISTS (
               SELECT 1 FROM task_series_tags st WHERE st.series_id = t.series_id
               AND NOT EXISTS (SELECT 1 FROM task_tags tt
                               WHERE tt.task_id = t.id AND tt.tag_id = st.tag_id))
           THEN 1 ELSE 0 END
         FROM tasks t JOIN task_series s ON s.id = t.series_id WHERE t.id = ?1",
    )
    .bind(task_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(safe == 1)
}

/// Rebuild only untouched cache rows. Unsafe rows become stable exceptions so
/// a later rebuild cannot delete them after an attachment/reminder is removed.
async fn purge_future_generated(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    series_id: &str,
    from_key: &str,
) -> AppResult<(usize, usize)> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM tasks WHERE series_id = ?1 AND occurrence_key >= ?2
         AND occurrence_kind = 'generated' ORDER BY occurrence_key, id",
    )
    .bind(series_id)
    .bind(from_key)
    .fetch_all(&mut **tx)
    .await?;

    let mut purged = 0usize;
    let mut preserved = 0usize;
    for id in ids {
        if is_occurrence_rebuild_safe(tx, &id).await? {
            purged += sqlx::query(
                "DELETE FROM tasks WHERE id = ?1 AND occurrence_kind = 'generated'
                 AND is_user_modified = 0 AND deleted_at IS NULL",
            )
            .bind(&id)
            .execute(&mut **tx)
            .await?
            .rows_affected() as usize;
        } else {
            preserved += sqlx::query(
                "UPDATE tasks SET occurrence_kind = 'exception', is_exception = 1
                 WHERE id = ?1 AND occurrence_kind = 'generated'",
            )
            .bind(&id)
            .execute(&mut **tx)
            .await?
            .rows_affected() as usize;
        }
    }

    Ok((purged, preserved))
}

/// 供其它模块引用：给定系列在指定范围内的实例
pub async fn list_instances(state: &AppState, series_id: &str) -> AppResult<Vec<Task>> {
    let rows = sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE series_id = ?1 AND deleted_at IS NULL
         ORDER BY occurrence_key ASC",
    )
    .bind(series_id)
    .fetch_all(state.db.pool())
    .await?;
    Ok(rows)
}

/// 把一批标签复制给新实例（物化时用）
pub async fn copy_tags(state: &AppState, from_task: &str, to_task: &str) -> AppResult<usize> {
    let n = sqlx::query(
        "INSERT OR IGNORE INTO task_tags (task_id, tag_id)
         SELECT ?1, tag_id FROM task_tags WHERE task_id = ?2",
    )
    .bind(to_task)
    .bind(from_task)
    .execute(state.db.pool())
    .await?
    .rows_affected() as usize;
    Ok(n)
}

/// 供其它模块引用：某任务是否属于重复系列
pub async fn series_of_task(state: &AppState, task_id: &str) -> AppResult<Option<String>> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT series_id FROM tasks WHERE id = ?1")
            .bind(task_id)
            .fetch_optional(state.db.pool())
            .await?;
    Ok(row.and_then(|(s,)| s))
}

/// 供其它模块引用：`EditScope` 的字符串形式（供日志与错误信息）
pub fn scope_label(s: EditScope) -> &'static str {
    match s {
        EditScope::ThisOnly => "仅此次",
        EditScope::ThisAndFuture => "此次及以后",
        EditScope::WholeSeries => "整个系列",
    }
}

/// 把规则序列化为可存储的字符串（供分段使用）
pub fn rule_to_string(rule: &RecurrenceRule) -> AppResult<String> {
    rule.to_rrule_string()
}

/// 判断某次发生是否早于给定 UTC 时刻（用于"不追改历史"的判定）
pub fn is_before(occurrence_key: &str, cutoff_utc: &str) -> bool {
    occurrence_key < cutoff_utc
}

/// 从本地日期构造一个系列起点字符串
pub fn local_date_to_dtstart(date: &str, time: Option<&str>) -> AppResult<String> {
    let d = parse_local_date(date)?;
    let t = match time {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => "00:00:00".to_string(),
    };
    Ok(format!("{}T{}", d.format("%Y-%m-%d"), t))
}

/// 便捷：把 `Freq` 与其参数合成为 RRULE（供界面构造规则时使用）
pub fn build_simple_rrule(
    freq: Freq,
    interval: i64,
    by_weekday: &[u8],
    weekdays_only: bool,
) -> String {
    let mut s = format!(
        "FREQ={}",
        match freq {
            Freq::Daily => "DAILY",
            Freq::Weekly => "WEEKLY",
            Freq::Monthly => "MONTHLY",
            Freq::Yearly => "YEARLY",
        }
    );
    if interval > 1 {
        s.push_str(&format!(";INTERVAL={interval}"));
    }
    if weekdays_only {
        s.push_str(";BYDAY=MO,TU,WE,TH,FR");
    } else if !by_weekday.is_empty() {
        let names: Vec<&str> = by_weekday
            .iter()
            .filter_map(|w| {
                ["MO", "TU", "WE", "TH", "FR", "SA", "SU"]
                    .get((*w as usize).saturating_sub(1))
                    .copied()
            })
            .collect();
        s.push_str(&format!(";BYDAY={}", names.join(",")));
    }
    s
}

/// 便捷：构造"每月第 N 个星期 X"的 RRULE
pub fn build_setpos_rrule(sp: SetPos) -> String {
    let code = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"]
        .get((sp.weekday as usize).saturating_sub(1))
        .copied()
        .unwrap_or("MO");
    format!("FREQ=MONTHLY;BYDAY={code};BYSETPOS={}", sp.nth)
}

/// 便捷：构造"每月指定日"的 RRULE
pub fn build_monthday_rrule(days: &[u8]) -> String {
    let list: Vec<String> = days.iter().map(|d| d.to_string()).collect();
    format!("FREQ=MONTHLY;BYMONTHDAY={}", list.join(","))
}

/// 查询用的辅助类型：系列实例统计
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesStats {
    pub series_id: String,
    pub total_instances: i64,
    pub completed: i64,
    pub exceptions: i64,
    pub skipped: i64,
    pub segments: i64,
}

/// 系列的实例统计（用于界面说明"这个系列已经有多少次，改规则会影响什么"）
#[tauri::command]
pub async fn recurring_stats(
    state: State<'_, AppState>,
    series_id: String,
) -> AppResult<SeriesStats> {
    get_series(&state, &series_id).await?;
    let row = sqlx::query(
        "SELECT
            (SELECT COUNT(*) FROM tasks WHERE series_id = ?1) AS total,
            (SELECT COUNT(*) FROM tasks WHERE series_id = ?1 AND status = 'done') AS done,
            (SELECT COUNT(*) FROM tasks WHERE series_id = ?1 AND occurrence_kind = 'exception') AS exc,
            (SELECT COUNT(*) FROM task_series_skips WHERE series_id = ?1) AS skipped,
            (SELECT COUNT(*) FROM task_series_segments WHERE series_id = ?1) AS segs",
    )
    .bind(&series_id)
    .fetch_one(state.db.pool())
    .await?;

    Ok(SeriesStats {
        series_id,
        total_instances: row.try_get("total")?,
        completed: row.try_get("done")?,
        exceptions: row.try_get("exc")?,
        skipped: row.try_get("skipped")?,
        segments: row.try_get("segs")?,
    })
}

// =============================================================================
// 命令：范围操作（§5 核心）
// =============================================================================

/// 单个实例的字段补丁（用于"仅此次"）
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstancePatch {
    #[serde(default)]
    pub tzid: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub note_md: Option<String>,
    #[serde(default)]
    pub link_url: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub tag_ids: Option<Vec<String>>,
    #[serde(default)]
    pub period_type: Option<String>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub planned_at: Option<String>,
    #[serde(default)]
    pub has_planned_time: Option<bool>,
    #[serde(default)]
    pub due_at: Option<String>,
    #[serde(default)]
    pub has_due_time: Option<bool>,
    #[serde(default)]
    pub estimated_minutes: Option<i64>,
    #[serde(default)]
    pub clear_planned_at: bool,
    #[serde(default)]
    pub clear_due_at: bool,
    #[serde(default)]
    pub clear_link: bool,
    #[serde(default)]
    pub clear_project: bool,
    #[serde(default)]
    pub clear_category: bool,
    #[serde(default)]
    pub clear_estimated_minutes: bool,
}

/// 范围操作的结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeActionResult {
    /// 实际影响的任务数
    pub affected: i64,
    /// 受影响的历史（已完成）任务数——用于界面提示"这会影响历史"
    pub affected_history: i64,
    /// 新生成的实例数（改规则后重算时）
    pub regenerated: i64,
    /// 人类可读的结果说明
    pub message: String,
}

/// 校验：按范围操作时，若会影响历史且用户未确认，则拒绝并说明。
///
/// §5 要求「如用户选择整个系列且操作会影响历史，须预览影响并要求明确确认」。
/// 这里把"确认"表达为调用方显式传 `confirm_history = true`，
/// 而不是让后端默默改掉历史数据。
async fn history_guard(
    state: &AppState,
    series_id: &str,
    cutoff_key: &str,
    confirm: bool,
) -> AppResult<i64> {
    let n: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM tasks
         WHERE series_id = ?1 AND occurrence_key < ?2
           AND (status = 'done' OR completed_at IS NOT NULL)",
    )
    .bind(series_id)
    .bind(cutoff_key)
    .fetch_one(state.db.pool())
    .await?
    .try_get("n")?;

    if n > 0 && !confirm {
        return Err(
            AppError::conflict(format!("此操作会影响 {n} 个已完成的历史任务"))
                .with_hint("请先在界面上查看影响范围，确认后再执行（避免已完成记录被追改）"),
        );
    }
    Ok(n)
}

/// 编辑某次发生，按范围决定生效方式（§5「仅此次 / 此次及以后 / 整个系列」）。
///
/// - **仅此次**：把该实例标记为 `exception` 并写入新字段。
///   该次的 `occurrence_key` 不变，因此不会在旧日期生成副本。
/// - **此次及以后**：新增一个规则分段，起点为该次；同时删除该次之后
///   **未改动且未完成**的旧实例（它们会按新规则重新物化）。
///   已完成实例与用户改过的例外一律保留。
/// - **整个系列**：更新系列本身的基础字段，并同步到所有未完成的实例
///   （不改已完成实例的字段，保护历史）。
pub async fn edit_instance_impl(
    state: &AppState,
    task_id: String,
    scope: EditScope,
    patch: InstancePatch,
    // 新增/变更的规则（仅"此次及以后"与"整个系列"使用）
    new_rrule: Option<String>,
    // 允许影响历史（§5 要求明确确认）。
    // 注意：函数参数上不能用 #[serde(default)]，那是结构体字段的属性。
    // Option 类型在参数缺省时本就会被反序列化为 None；
    // bool 则必须由前端显式传入（前端始终会传，避免歧义）。
    confirm_history: bool,
) -> AppResult<ScopeActionResult> {
    let db = &state.db;

    // 读取实例
    let task = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?1")
        .bind(&task_id)
        .fetch_optional(db.pool())
        .await?;
    let Some(task) = task else {
        return Err(AppError::not_found("任务", &task_id));
    };
    let Some(series_id) = task.series_id.clone() else {
        return Err(AppError::validation("该任务不属于任何重复系列")
            .with_hint("请直接编辑这个任务（它本身不重复）"));
    };
    let Some(occ_key) = task.occurrence_key.clone() else {
        return Err(
            AppError::conflict("重复实例缺少 occurrence_key，无法确定是哪一次")
                .with_hint("数据可能已损坏，请导出一份备份后联系支持"),
        );
    };

    let series = get_series(state, &series_id).await?;
    let now = to_db_time(utc_now());

    if scope == EditScope::ThisOnly && patch.tzid.is_some() {
        return Err(AppError::validation("单次任务不能单独更换重复系列的时区"));
    }
    let zone_anchor = if scope == EditScope::WholeSeries {
        &now
    } else {
        &occ_key
    };
    let segment_rows = sqlx::query_as::<_, SeriesSegmentRow>(
        "SELECT * FROM task_series_segments WHERE series_id = ?1",
    )
    .bind(&series_id)
    .fetch_all(db.pool())
    .await?;
    let segments: Vec<Segment> = segment_rows.into_iter().map(Segment::from).collect();
    let timeline = build_rule_timeline(&series, &segments)?;
    let current_tzid = timeline
        .iter()
        .rev()
        .find(|(_, anchor, _, _)| {
            anchor
                .as_deref()
                .is_none_or(|key| key <= zone_anchor.as_str())
        })
        .map(|(_, _, rule, _)| rule.tzid.clone())
        .unwrap_or_else(|| series.tzid.clone());
    let requested_tzid = patch.tzid.as_deref().unwrap_or(&current_tzid);
    let _: chrono_tz::Tz = requested_tzid
        .parse()
        .map_err(|_| AppError::validation("时区名称无效，请使用 IANA 时区，例如 Asia/Shanghai"))?;
    let changed_tzid = (requested_tzid != current_tzid).then(|| requested_tzid.to_string());

    // These fields have no segment-level representation. They can be changed
    // atomically on one occurrence, but a broader scope would silently drop
    // them when a future occurrence is materialized.
    if scope != EditScope::ThisOnly
        && (patch.note_md.is_some()
            || patch.link_url.is_some()
            || patch.project_id.is_some()
            || patch.category_id.is_some()
            || patch.tag_ids.is_some()
            || patch.period_type.is_some()
            || patch.clear_link
            || patch.clear_project
            || patch.clear_category
            || patch.clear_estimated_minutes)
    {
        return Err(AppError::validation(
            "备注、链接、归属、标签、周期及清空预计耗时目前仅支持「仅此次」",
        ));
    }

    match scope {
        // ------------------------------------------------------------------
        // 仅此次：写例外，绝不影响其它发生
        // ------------------------------------------------------------------
        EditScope::ThisOnly => {
            // occurrence_kind 改为 exception 且 is_exception = 1：
            // 这样后续"此次及以后"重算时会跳过它（用户改过的必须保留）
            let title = match &patch.title {
                Some(t) => {
                    let v = t.trim();
                    if v.is_empty() {
                        return Err(AppError::validation("标题不能为空"));
                    }
                    if v.chars().count() > 500 {
                        return Err(AppError::validation("标题不能超过 500 个字符"));
                    }
                    Some(v.to_string())
                }
                None => None,
            };

            // 手动累积 SET 片段与绑定值。
            //
            // 这里刻意**不用** `QueryBuilder::separated()`：
            // `Separated::push` 会把分隔符拼进片段流，与 `push_bind`
            // 混用时空格与逗号的位置很容易错位，产生 `SET , title = ?`
            // 这类语法错误。手动维护「片段数组 + 值数组」虽然朴素，
            // 但顺序一目了然，也便于断言两者数量一致。
            let mut sets: Vec<String> = Vec::new();
            let mut binds: Vec<BindValue> = Vec::new();

            if let Some(t) = &title {
                sets.push("title = ?".into());
                binds.push(BindValue::S(t.clone()));
            }
            if let Some(d) = &patch.description {
                sets.push("description = ?".into());
                binds.push(BindValue::S(crate::commands::validate_long_text(
                    "描述", d,
                )?));
            }
            if let Some(n) = &patch.note_md {
                sets.push("note_md = ?".into());
                binds.push(BindValue::S(crate::commands::validate_long_text(
                    "备注", n,
                )?));
            }
            if patch.clear_link {
                sets.push("link_url = NULL".into());
            } else if let Some(link) = &patch.link_url {
                sets.push("link_url = ?".into());
                binds.push(BindValue::S(crate::commands::validate_link_url(link)?));
            }
            if patch.clear_project {
                sets.push("project_id = NULL".into());
            } else if let Some(project) = &patch.project_id {
                sets.push("project_id = ?".into());
                binds.push(BindValue::S(project.clone()));
            }
            if patch.clear_category {
                sets.push("category_id = NULL".into());
            } else if let Some(category) = &patch.category_id {
                sets.push("category_id = ?".into());
                binds.push(BindValue::S(category.clone()));
            }
            if let Some(period) = &patch.period_type {
                sets.push("period_type = ?".into());
                binds.push(BindValue::S(
                    crate::commands::validate_period(period)?.to_string(),
                ));
            }
            if let Some(p) = patch.priority {
                if !(0..=3).contains(&p) {
                    return Err(AppError::validation("优先级只能是 0–3"));
                }
                sets.push("priority = ?".into());
                binds.push(BindValue::I(p));
            }
            if patch.clear_estimated_minutes {
                sets.push("estimated_minutes = NULL".into());
            } else if let Some(v) = patch.estimated_minutes {
                sets.push("estimated_minutes = ?".into());
                binds.push(BindValue::I(crate::commands::validate_minutes(
                    "预计耗时",
                    v,
                )?));
            }

            if patch.clear_planned_at {
                sets.push("planned_at = NULL".into());
                sets.push("has_planned_time = 0".into());
            } else if let Some(p) = &patch.planned_at {
                // 改期时 occurrence_key 保持原值——它代表"原本该发生的时刻"，
                // 是这一次的稳定身份，不能跟着 planned_at 一起变。
                sets.push("planned_at = ?".into());
                binds.push(BindValue::S(p.clone()));
                if let Some(h) = patch.has_planned_time {
                    sets.push("has_planned_time = ?".into());
                    binds.push(BindValue::I(h as i64));
                }
            }

            if patch.clear_due_at {
                sets.push("due_at = NULL".into());
                sets.push("has_due_time = 0".into());
            } else if let Some(d) = &patch.due_at {
                sets.push("due_at = ?".into());
                binds.push(BindValue::S(d.clone()));
                if let Some(h) = patch.has_due_time {
                    sets.push("has_due_time = ?".into());
                    binds.push(BindValue::I(h as i64));
                }
            }

            // 固定标记这个实例为"用户改过的例外"，并刷新时间戳
            sets.push("occurrence_kind = ?".into());
            binds.push(BindValue::S("exception".to_string()));
            sets.push("is_exception = 1".into());
            sets.push("updated_at = ?".into());
            binds.push(BindValue::S(now.clone()));

            let sql = format!("UPDATE tasks SET {} WHERE id = ?", sets.join(", "));
            let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
            for v in binds {
                q = match v {
                    BindValue::S(s) => q.bind(s),
                    BindValue::I(i) => q.bind(i),
                };
            }
            let mut tx = db.pool().begin().await?;
            q.bind(task_id.clone()).execute(&mut *tx).await?;

            if let Some(tag_ids) = &patch.tag_ids {
                sqlx::query("DELETE FROM task_tags WHERE task_id = ?1")
                    .bind(&task_id)
                    .execute(&mut *tx)
                    .await?;
                for tag_id in tag_ids.iter().filter(|id| !id.is_empty()) {
                    sqlx::query("INSERT INTO task_tags (task_id, tag_id) VALUES (?1, ?2)")
                        .bind(&task_id)
                        .bind(tag_id)
                        .execute(&mut *tx)
                        .await?;
                }
            }

            // 时间变了要重算相对型提醒（§4.3）。
            // 整改任务书 §5：这里同样不能吞掉错误——重算失败必须让整个
            // "只改这一次"的操作失败，而不是留下时间与提醒不一致的实例。
            crate::reminders::recompute_task_reminders_tx(&mut tx, &task_id).await?;
            tx.commit().await?;

            Ok(ScopeActionResult {
                affected: 1,
                affected_history: 0,
                regenerated: 0,
                message: "已只修改这一次；同系列的其它发生不受影响".to_string(),
            })
        }

        // ------------------------------------------------------------------
        // 此次及以后：分段 + 重算未来
        // ------------------------------------------------------------------
        EditScope::ThisAndFuture => {
            // §5：不追改历史。若该次之前有已完成实例，必须先确认。
            let history = history_guard(state, &series_id, &occ_key, confirm_history).await?;

            let effective_rrule = match &new_rrule {
                Some(r) if !r.trim().is_empty() => {
                    // 新规则必须能解析，且要与系列的时区/起点兼容
                    RecurrenceRule::from_rrule_string(
                        r,
                        requested_tzid,
                        &series.dtstart_local,
                        series.has_start_time == 1,
                    )?;
                    Some(r.clone())
                }
                // No rule change: retain the previous segment's phase.
                _ => None,
            };

            let mut tx = db.pool().begin().await?;

            // 新分段的版本号 = 当前最大版本 + 1
            let max_ver: Option<i64> = sqlx::query(
                "SELECT MAX(rule_version) AS v FROM task_series_segments WHERE series_id = ?1",
            )
            .bind(&series_id)
            .fetch_one(&mut *tx)
            .await?
            .try_get("v")?;
            let next_ver = max_ver
                .unwrap_or(series.rule_version)
                .max(series.rule_version)
                + 1;

            let seg_id = uuid::Uuid::now_v7().to_string();
            sqlx::query(
                "INSERT INTO task_series_segments
                    (id, series_id, rule_version, effective_from_occurrence, new_rrule, new_tzid,
                     override_title, override_description, override_priority, override_project_id,
                     override_category_id, override_estimated_minutes, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL, ?10, ?11)",
            )
            .bind(&seg_id)
            .bind(&series_id)
            .bind(next_ver)
            .bind(&occ_key)
            .bind(&effective_rrule)
            .bind(&changed_tzid)
            .bind(
                patch
                    .title
                    .as_ref()
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty()),
            )
            .bind(&patch.description)
            .bind(patch.priority)
            .bind(patch.estimated_minutes)
            .bind(&now)
            .execute(&mut *tx)
            .await?;

            // 系列自身的版本号推进，便于排查
            if let Some(ref r) = effective_rrule {
                let parsed = RecurrenceRule::from_rrule_string(
                    r,
                    requested_tzid,
                    &series.dtstart_local,
                    series.has_start_time == 1,
                )?;
                let (kind, until, count) = end_columns(&parsed);
                sqlx::query("UPDATE task_series SET rule_version = ?1, recurrence_end_kind = ?2, recurrence_until = ?3, recurrence_count = ?4, updated_at = ?5 WHERE id = ?6")
                    .bind(next_ver)
                    .bind(kind)
                    .bind(until)
                    .bind(count)
                    .bind(&now)
                    .bind(&series_id)
                    .execute(&mut *tx)
                    .await?;
            } else {
                sqlx::query(
                    "UPDATE task_series SET rule_version = ?1, updated_at = ?2 WHERE id = ?3",
                )
                .bind(next_ver)
                .bind(&now)
                .bind(&series_id)
                .execute(&mut *tx)
                .await?;
            }

            // 删除"该次及以后、未改动且未完成"的旧实例。
            // 已完成的、以及用户单独改过的（exception）一律保留——
            // §5 明确要求历史不能因改规则而消失。
            let (purged, preserved) = purge_future_generated(&mut tx, &series_id, &occ_key).await?;
            let now_horizon = utc_now() + chrono::Duration::days(365);
            let anchor_dt = chrono::DateTime::parse_from_rfc3339(&occ_key)
                .map_err(|_| AppError::validation("发生时间格式无效"))?
                .with_timezone(&chrono::Utc);
            let end_range = to_db_time(now_horizon.max(anchor_dt + chrono::Duration::days(365)));
            sqlx::query(
                "INSERT INTO task_series_rebuilds
                 (series_id, range_start_utc, range_end_utc, requested_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(series_id) DO UPDATE SET
                   range_start_utc = excluded.range_start_utc,
                   range_end_utc = excluded.range_end_utc,
                   requested_at = excluded.requested_at, last_error = NULL",
            )
            .bind(&series_id)
            .bind(&occ_key)
            .bind(&end_range)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;

            let regenerated = regenerate_pending(state, &series_id).await?;

            log::info!(
                "系列 {series_id} 从 {occ_key} 起改用新规则（版本 {next_ver}）：\
                 清理 {purged} 个纯物化实例，保留 {preserved} 个已使用实例，重新生成 {regenerated} 个"
            );

            Ok(ScopeActionResult {
                affected: purged as i64,
                affected_history: history,
                regenerated: regenerated as i64,
                message: format!(
                    "已修改这一次及以后的发生（规则版本 {next_ver}）；\
                     保留 {preserved} 个已有数据的实例，之前的 {history} 个已完成记录保持不变"
                ),
            })
        }

        // ------------------------------------------------------------------
        // 整个系列：改基础规则/字段，历史实例的字段不动
        // ------------------------------------------------------------------
        EditScope::WholeSeries => {
            let history = history_guard(state, &series_id, &occ_key, confirm_history).await?;

            let mut tx = db.pool().begin().await?;
            let mut rebuild_from: Option<String> = None;
            let mut preserved_by_rebuild = 0usize;

            if let Some(r) = new_rrule.as_ref().filter(|r| !r.trim().is_empty()) {
                let parsed = RecurrenceRule::from_rrule_string(
                    r,
                    requested_tzid,
                    &series.dtstart_local,
                    series.has_start_time == 1,
                )?;
                let (kind, until, count) = end_columns(&parsed);
                // `task_series.rrule` is the original segment's rule. Changing
                // it here would retroactively rewrite the base timeline when
                // an old range is requested again. The new rule lives only in
                // the segment whose anchor starts the future schedule.
                sqlx::query(
                    "UPDATE task_series SET rule_version = rule_version + 1,
                     recurrence_end_kind = ?1, recurrence_until = ?2,
                     recurrence_count = ?3, updated_at = ?4 WHERE id = ?5",
                )
                .bind(kind)
                .bind(until)
                .bind(count)
                .bind(&now)
                .bind(&series_id)
                .execute(&mut *tx)
                .await?;

                // Preserve historical segments, but make a new final segment
                // authoritative from the selected occurrence onward.
                let next_future: Option<String> = sqlx::query_scalar(
                    "SELECT MIN(occurrence_key) FROM tasks
                     WHERE series_id = ?1 AND occurrence_key >= ?2 AND deleted_at IS NULL",
                )
                .bind(&series_id)
                .bind(&now)
                .fetch_one(&mut *tx)
                .await?;
                let anchor = next_future.as_deref().unwrap_or_else(|| {
                    if occ_key > now {
                        &occ_key
                    } else {
                        &now
                    }
                });
                let next_ver: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(MAX(rule_version), 1) + 1
                     FROM task_series_segments WHERE series_id = ?1",
                )
                .bind(&series_id)
                .fetch_one(&mut *tx)
                .await?;
                sqlx::query(
                    "INSERT INTO task_series_segments
                     (id, series_id, rule_version, effective_from_occurrence, new_rrule, new_tzid, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )
                .bind(uuid::Uuid::now_v7().to_string())
                .bind(&series_id)
                .bind(next_ver)
                .bind(anchor)
                .bind(r)
                .bind(&changed_tzid)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
                let (_, preserved) = purge_future_generated(&mut tx, &series_id, anchor).await?;
                preserved_by_rebuild = preserved;
                let end = to_db_time(utc_now() + chrono::Duration::days(365));
                sqlx::query(
                    "INSERT INTO task_series_rebuilds
                     (series_id, range_start_utc, range_end_utc, requested_at)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(series_id) DO UPDATE SET
                       range_start_utc = excluded.range_start_utc,
                       range_end_utc = excluded.range_end_utc,
                       requested_at = excluded.requested_at, last_error = NULL",
                )
                .bind(&series_id)
                .bind(anchor)
                .bind(&end)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
                rebuild_from = Some(anchor.to_string());
            }

            // 方案 A：单次例外的系列内容字段作为整体保留。
            // schema 没有字段级例外元数据，所以所有系列内容更新均只覆盖
            // generated 实例；已完成历史也一律不动。
            let mut affected = 0i64;
            if let Some(p) = patch.priority {
                if !(0..=3).contains(&p) {
                    return Err(AppError::validation("优先级只能是 0–3"));
                }
                affected += sqlx::query(
                    "UPDATE tasks SET priority = ?1, updated_at = ?2
                     WHERE series_id = ?3 AND status <> 'done' AND completed_at IS NULL
                       AND deleted_at IS NULL AND occurrence_kind = 'generated'",
                )
                .bind(p)
                .bind(&now)
                .bind(&series_id)
                .execute(&mut *tx)
                .await?
                .rows_affected() as i64;
                sqlx::query("UPDATE task_series_template SET priority = ?1 WHERE series_id = ?2")
                    .bind(p)
                    .bind(&series_id)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query(
                    "UPDATE task_series_segments SET override_priority = ?1 WHERE series_id = ?2",
                )
                .bind(p)
                .bind(&series_id)
                .execute(&mut *tx)
                .await?;
            }
            if let Some(t) = patch
                .title
                .as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                // 标题是例外最常定制的字段，因此只同步到非例外实例
                affected += sqlx::query(
                    "UPDATE tasks SET title = ?1, updated_at = ?2
                     WHERE series_id = ?3 AND status <> 'done' AND completed_at IS NULL
                       AND deleted_at IS NULL AND occurrence_kind = 'generated'",
                )
                .bind(t)
                .bind(&now)
                .bind(&series_id)
                .execute(&mut *tx)
                .await?
                .rows_affected() as i64;
                sqlx::query("UPDATE task_series_template SET title = ?1 WHERE series_id = ?2")
                    .bind(t)
                    .bind(&series_id)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query(
                    "UPDATE task_series_segments SET override_title = ?1 WHERE series_id = ?2",
                )
                .bind(t)
                .bind(&series_id)
                .execute(&mut *tx)
                .await?;
            }
            if let Some(v) = patch.estimated_minutes {
                affected += sqlx::query(
                    "UPDATE tasks SET estimated_minutes = ?1, updated_at = ?2
                     WHERE series_id = ?3 AND status <> 'done' AND completed_at IS NULL
                       AND deleted_at IS NULL AND occurrence_kind = 'generated'",
                )
                .bind(v)
                .bind(&now)
                .bind(&series_id)
                .execute(&mut *tx)
                .await?
                .rows_affected() as i64;
                sqlx::query(
                    "UPDATE task_series_template SET estimated_minutes = ?1 WHERE series_id = ?2",
                )
                .bind(v)
                .bind(&series_id)
                .execute(&mut *tx)
                .await?;
                sqlx::query("UPDATE task_series_segments SET override_estimated_minutes = ?1 WHERE series_id = ?2")
                    .bind(v)
                    .bind(&series_id)
                    .execute(&mut *tx)
                    .await?;
            }
            if let Some(d) = patch.description.as_ref() {
                affected += sqlx::query(
                    "UPDATE tasks SET description = ?1, updated_at = ?2
                     WHERE series_id = ?3 AND status <> 'done' AND completed_at IS NULL
                       AND deleted_at IS NULL AND occurrence_kind = 'generated'",
                )
                .bind(d)
                .bind(&now)
                .bind(&series_id)
                .execute(&mut *tx)
                .await?
                .rows_affected() as i64;
                sqlx::query(
                    "UPDATE task_series_template SET description = ?1 WHERE series_id = ?2",
                )
                .bind(d)
                .bind(&series_id)
                .execute(&mut *tx)
                .await?;
                sqlx::query("UPDATE task_series_segments SET override_description = ?1 WHERE series_id = ?2")
                    .bind(d)
                    .bind(&series_id)
                    .execute(&mut *tx)
                    .await?;
            }

            tx.commit().await?;
            let regenerated = if rebuild_from.is_some() {
                regenerate_pending(state, &series_id).await? as i64
            } else {
                0
            };

            Ok(ScopeActionResult {
                affected,
                affected_history: history,
                regenerated,
                message: format!(
                    "已修改整个系列（本次同步了 {affected} 个未完成实例）；\
                     保留 {preserved_by_rebuild} 个已有数据的实例，已完成的 {history} 个历史记录保持原样"
                ),
            })
        }
    }
}

/// 取消某次发生（§5：「单次取消显示为该次跳过，不意外取消整条系列」）。
///
/// 实现方式：删除已物化的实例（若有）并写入一条跳过记录，
/// 使后续物化不会再生成它。系列本身完全不受影响。
pub async fn skip_occurrence_impl(
    state: &AppState,
    task_id: String,
) -> AppResult<ScopeActionResult> {
    let db = &state.db;

    let task = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?1")
        .bind(&task_id)
        .fetch_optional(db.pool())
        .await?;
    let Some(task) = task else {
        return Err(AppError::not_found("任务", &task_id));
    };
    let Some(series_id) = task.series_id.clone() else {
        return Err(AppError::validation("该任务不属于重复系列，请直接删除它"));
    };
    let Some(occ_key) = task.occurrence_key.clone() else {
        return Err(AppError::conflict(
            "重复实例缺少 occurrence_key，无法定位要跳过的那一次",
        ));
    };
    if task.status == "done" {
        return Err(AppError::conflict("已完成的发生不能跳过")
            .with_hint("如需撤销完成，请先点回未完成状态"));
    }

    let now = to_db_time(utc_now());
    let mut tx = db.pool().begin().await?;

    sqlx::query(
        "INSERT OR REPLACE INTO task_series_skips
            (id, series_id, occurrence_key, occurrence_index, reason, created_at)
         VALUES (?1, ?2, ?3, ?4, 'user', ?5)",
    )
    .bind(uuid::Uuid::now_v7().to_string())
    .bind(&series_id)
    .bind(&occ_key)
    .bind(task.occurrence_index)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    // 软删除该实例（进回收站，仍可恢复；而不是硬删除）
    sqlx::query("UPDATE tasks SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2")
        .bind(&now)
        .bind(&task_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    log::info!("系列 {series_id} 的 {occ_key} 这一次已被跳过");

    Ok(ScopeActionResult {
        affected: 1,
        affected_history: 0,
        regenerated: 0,
        message: "已跳过这一次；系列本身与其它发生都不受影响".to_string(),
    })
}

/// 删除重复任务，按范围执行（§5「删除时同样提供范围」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteMode {
    /// 仅本次（等价于跳过）
    ThisOnly,
    /// 此次及以后
    ThisAndFuture,
    /// 整个系列
    WholeSeries,
}

/// 不可恢复的删除（清空系列全部痕迹）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    pub affected: i64,
    pub affected_history: i64,
    pub message: String,
}

/// 删除重复任务的某一部分或全部
pub async fn delete_recurring_impl(
    state: &AppState,
    task_id: String,
    mode: DeleteMode,
    confirm_history: bool,
) -> AppResult<DeleteResult> {
    let db = &state.db;

    let task = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?1")
        .bind(&task_id)
        .fetch_optional(db.pool())
        .await?;
    let Some(task) = task else {
        return Err(AppError::not_found("任务", &task_id));
    };
    let Some(series_id) = task.series_id.clone() else {
        return Err(AppError::validation("该任务不属于重复系列，请用普通删除"));
    };
    let Some(occ_key) = task.occurrence_key.clone() else {
        return Err(AppError::conflict(
            "重复实例缺少 occurrence_key，无法按范围删除",
        ));
    };

    let now = to_db_time(utc_now());

    match mode {
        DeleteMode::ThisOnly => {
            // 与"跳过"同一语义：这一次不发生，系列继续
            let mut tx = db.pool().begin().await?;
            sqlx::query(
                "INSERT OR REPLACE INTO task_series_skips
                    (id, series_id, occurrence_key, occurrence_index, reason, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'user', ?5)",
            )
            .bind(uuid::Uuid::now_v7().to_string())
            .bind(&series_id)
            .bind(&occ_key)
            .bind(task.occurrence_index)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE tasks SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2")
                .bind(&now)
                .bind(&task_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;

            Ok(DeleteResult {
                affected: 1,
                affected_history: 0,
                message: "已删除这一次；系列会继续按规则发生".to_string(),
            })
        }

        DeleteMode::ThisAndFuture => {
            let history = history_guard(state, &series_id, &occ_key, confirm_history).await?;
            let mut tx = db.pool().begin().await?;

            // 已完成的历史实例保留（§5 要求历史不消失）
            let affected = sqlx::query(
                "UPDATE tasks SET deleted_at = ?1, updated_at = ?1
                 WHERE series_id = ?2 AND occurrence_key >= ?3
                   AND status <> 'done' AND completed_at IS NULL AND deleted_at IS NULL",
            )
            .bind(&now)
            .bind(&series_id)
            .bind(&occ_key)
            .execute(&mut *tx)
            .await?
            .rows_affected() as i64;

            // The stable UTC key is an exclusive cutoff. Updating only the
            // local UNTIL date would allow future occurrences to reappear,
            // especially near timezone/DST boundaries.
            sqlx::query(
                "UPDATE task_series SET
                   terminated_from_occurrence_key = CASE
                     WHEN terminated_from_occurrence_key IS NULL
                       OR terminated_from_occurrence_key > ?1 THEN ?1
                     ELSE terminated_from_occurrence_key END,
                   updated_at = ?2 WHERE id = ?3",
            )
            .bind(&occ_key)
            .bind(&now)
            .bind(&series_id)
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;

            Ok(DeleteResult {
                affected,
                affected_history: history,
                message: format!(
                    "已删除这一次及以后的所有发生（保留 {history} 个已完成记录）；系列到此为止"
                ),
            })
        }

        DeleteMode::WholeSeries => {
            let history = history_guard(state, &series_id, &occ_key, confirm_history).await?;
            let mut tx = db.pool().begin().await?;

            // 全部实例软删除（可恢复），再删除系列本身
            let affected = sqlx::query(
                "UPDATE tasks SET deleted_at = ?1, updated_at = ?1
                 WHERE series_id = ?2 AND deleted_at IS NULL",
            )
            .bind(&now)
            .bind(&series_id)
            .execute(&mut *tx)
            .await?
            .rows_affected() as i64;

            // 分段与跳过记录随系列级联删除；实例的 series_id 置空以免悬挂
            sqlx::query(
                "UPDATE tasks SET series_id = NULL, occurrence_key = NULL WHERE series_id = ?1",
            )
            .bind(&series_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query("DELETE FROM task_series WHERE id = ?1")
                .bind(&series_id)
                .execute(&mut *tx)
                .await?;

            tx.commit().await?;

            Ok(DeleteResult {
                affected,
                affected_history: history,
                message: format!("已删除整个系列的 {affected} 个发生（可在回收站恢复）"),
            })
        }
    }
}

/// 查询某任务的重复系列信息（IPC 入口）
#[tauri::command]
pub async fn recurring_scope_info(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<serde_json::Value> {
    recurring_scope_info_inner(&state, task_id).await
}

/// 查询某任务的重复系列信息（业务实现，供测试与命令共用）
///
/// 返回：是否重复、属于哪一次、系列统计、该次之前的已完成历史数，
/// 以及三种范围各自的影响说明——供界面禁用不适用的选项并说明原因（§5）。
pub async fn recurring_scope_info_inner(
    state: &AppState,
    task_id: String,
) -> AppResult<serde_json::Value> {
    let series_id = series_of_task(state, &task_id).await?;
    let Some(sid) = series_id else {
        return Ok(serde_json::json!({
            "isRecurring": false,
            "reason": "该任务不重复，编辑时无需选择范围",
        }));
    };

    let task = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?1")
        .bind(&task_id)
        .fetch_optional(state.db.pool())
        .await?;
    let occ = task.as_ref().and_then(|t| t.occurrence_key.clone());

    let stats = {
        let row = sqlx::query(
            "SELECT
                (SELECT COUNT(*) FROM tasks WHERE series_id = ?1) AS total,
                (SELECT COUNT(*) FROM tasks WHERE series_id = ?1 AND status = 'done') AS done,
                (SELECT COUNT(*) FROM tasks WHERE series_id = ?1 AND occurrence_kind = 'exception') AS exc,
                (SELECT COUNT(*) FROM task_series_segments WHERE series_id = ?1) AS segs",
        )
        .bind(&sid)
        .fetch_one(state.db.pool())
        .await?;
        (
            row.try_get::<i64, _>("total")?,
            row.try_get::<i64, _>("done")?,
            row.try_get::<i64, _>("exc")?,
            row.try_get::<i64, _>("segs")?,
        )
    };

    // 该次之前有多少已完成历史（决定"整个系列"是否会影响历史）
    let history_before = match &occ {
        Some(k) => sqlx::query(
            "SELECT COUNT(*) AS n FROM tasks
                 WHERE series_id = ?1 AND occurrence_key < ?2
                   AND (status = 'done' OR completed_at IS NOT NULL)",
        )
        .bind(&sid)
        .bind(k)
        .fetch_one(state.db.pool())
        .await?
        .try_get::<i64, _>("n")?,
        None => 0,
    };

    Ok(serde_json::json!({
        "isRecurring": true,
        "seriesId": sid,
        "occurrenceKey": occ,
        "occurrenceIndex": task.as_ref().and_then(|t| t.occurrence_index),
        "isException": task.as_ref().map(|t| t.is_exception == 1).unwrap_or(false),
        "totalInstances": stats.0,
        "completed": stats.1,
        "exceptions": stats.2,
        "segments": stats.3,
        "completedBefore": history_before,
        // 界面据此禁用不适用的范围选项并说明原因（§5 要求）
        "availableScopes": {
            "thisOnly": true,
            "thisAndFuture": true,
            "wholeSeries": true,
        },
        "notes": {
            "thisOnly": "只改这一次，其它发生不受影响",
            "thisAndFuture": format!("会重算这一次之后尚未完成的 {} 个发生", (stats.0 - stats.1).max(0)),
            "wholeSeries": if history_before > 0 {
                format!("会影响 {history_before} 个已完成的历史记录，需要确认")
            } else {
                "修改整个系列的基础字段与规则".to_string()
            },
        },
    }))
}

// =============================================================================
// IPC 包装（薄层：只把 Tauri 的 State 转成 &AppState 后转调业务实现）
// =============================================================================

/// 按范围编辑某次发生（§5「仅此次 / 此次及以后 / 整个系列」）
#[tauri::command]
pub async fn recurring_edit_instance(
    state: State<'_, AppState>,
    task_id: String,
    scope: EditScope,
    patch: InstancePatch,
    new_rrule: Option<String>,
    confirm_history: bool,
) -> AppResult<ScopeActionResult> {
    edit_instance_impl(&state, task_id, scope, patch, new_rrule, confirm_history).await
}

/// 跳过某一次发生（等价于"单次取消"，系列继续）
#[tauri::command]
pub async fn recurring_skip_occurrence(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<ScopeActionResult> {
    skip_occurrence_impl(&state, task_id).await
}

/// 按范围删除重复任务
#[tauri::command]
pub async fn recurring_delete(
    state: State<'_, AppState>,
    task_id: String,
    mode: DeleteMode,
    confirm_history: bool,
) -> AppResult<DeleteResult> {
    delete_recurring_impl(&state, task_id, mode, confirm_history).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_labels_are_stable() {
        assert_eq!(scope_label(EditScope::ThisOnly), "仅此次");
        assert_eq!(scope_label(EditScope::ThisAndFuture), "此次及以后");
        assert_eq!(scope_label(EditScope::WholeSeries), "整个系列");
    }

    /// 未知范围值必须报错，绝不能静默按"整个系列"处理——
    /// 那会让用户以为只改了这一次，实际上动了全部。
    #[test]
    fn unknown_scope_is_rejected() {
        assert!(serde_json::from_str::<EditScope>("\"this_only\"").is_ok());
        assert!(serde_json::from_str::<EditScope>("\"this_and_future\"").is_ok());
        assert!(serde_json::from_str::<EditScope>("\"whole_series\"").is_ok());
        assert!(serde_json::from_str::<EditScope>("\"everything\"").is_err());
    }

    #[test]
    fn builds_simple_rrule() {
        assert_eq!(build_simple_rrule(Freq::Daily, 1, &[], false), "FREQ=DAILY");
        assert_eq!(
            build_simple_rrule(Freq::Daily, 3, &[], false),
            "FREQ=DAILY;INTERVAL=3"
        );
        assert_eq!(
            build_simple_rrule(Freq::Weekly, 1, &[1, 3, 5], false),
            "FREQ=WEEKLY;BYDAY=MO,WE,FR"
        );
        assert_eq!(
            build_simple_rrule(Freq::Weekly, 1, &[], true),
            "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"
        );
    }

    #[test]
    fn builds_setpos_and_monthday_rrule() {
        assert_eq!(
            build_setpos_rrule(SetPos { nth: 3, weekday: 5 }),
            "FREQ=MONTHLY;BYDAY=FR;BYSETPOS=3"
        );
        assert_eq!(
            build_setpos_rrule(SetPos {
                nth: -1,
                weekday: 5
            }),
            "FREQ=MONTHLY;BYDAY=FR;BYSETPOS=-1"
        );
        assert_eq!(
            build_monthday_rrule(&[1, 15, 31]),
            "FREQ=MONTHLY;BYMONTHDAY=1,15,31"
        );
    }

    /// 构造出的 RRULE 必须能被解析回来（否则界面构造的规则存不下去）
    #[test]
    fn built_rrules_are_parseable() {
        for s in [
            build_simple_rrule(Freq::Daily, 1, &[], false),
            build_simple_rrule(Freq::Daily, 5, &[], false),
            build_simple_rrule(Freq::Weekly, 2, &[1, 5], false),
            build_simple_rrule(Freq::Weekly, 1, &[], true),
            build_setpos_rrule(SetPos { nth: 2, weekday: 2 }),
            build_setpos_rrule(SetPos {
                nth: -1,
                weekday: 7,
            }),
            build_monthday_rrule(&[1, 31]),
        ] {
            let r =
                RecurrenceRule::from_rrule_string(&s, "Asia/Shanghai", "2026-01-05T09:00:00", true)
                    .unwrap_or_else(|e| panic!("{s} 应可解析：{e}"));
            // 再序列化一次，保证往返稳定
            let back = rule_to_string(&r).unwrap();
            assert!(
                RecurrenceRule::from_rrule_string(
                    &back,
                    "Asia/Shanghai",
                    "2026-01-05T09:00:00",
                    true
                )
                .is_ok(),
                "{s} → {back} 应可再次解析"
            );
        }
    }

    #[test]
    fn local_date_to_dtstart_defaults_to_midnight() {
        assert_eq!(
            local_date_to_dtstart("2026-09-21", None).unwrap(),
            "2026-09-21T00:00:00"
        );
        assert_eq!(
            local_date_to_dtstart("2026-09-21", Some("09:30")).unwrap(),
            "2026-09-21T09:30"
        );
        assert_eq!(
            local_date_to_dtstart("2026-09-21", Some("")).unwrap(),
            "2026-09-21T00:00:00",
            "空时刻应视为仅日期而不是报错"
        );
        assert!(local_date_to_dtstart("2026-13-45", None).is_err());
    }

    #[test]
    fn is_before_compares_utc_strings() {
        // 固定宽度 UTC 字符串的字典序即时间序
        assert!(is_before(
            "2026-09-21T01:00:00.000Z",
            "2026-09-22T00:00:00.000Z"
        ));
        assert!(!is_before(
            "2026-09-23T01:00:00.000Z",
            "2026-09-22T00:00:00.000Z"
        ));
        assert!(!is_before(
            "2026-09-22T00:00:00.000Z",
            "2026-09-22T00:00:00.000Z"
        ));
    }

    /// 分段列表必须按生效起点排序，否则"哪一段对某次生效"会判错。
    #[test]
    fn segments_sort_by_effective_point() {
        let mut v = vec!["2026-10-01", "2026-09-01", "2026-11-01"];
        v.sort();
        assert_eq!(v, vec!["2026-09-01", "2026-10-01", "2026-11-01"]);
    }
}
