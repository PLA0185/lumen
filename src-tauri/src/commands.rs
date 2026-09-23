//! 任务 CRUD 等 IPC 命令实现。
//!
//! 约定（任务书 §4.1 / §9 / §10）：
//! - 所有写操作走事务，避免"留下半条规则"。
//! - 删除一律软删除；永久删除仅限回收站显式操作。
//! - 输入全部校验后才落库，错误以 `AppError` 返回可读中文消息。

use sqlx::{QueryBuilder, Row, Sqlite};
use tauri::State;

use crate::db::{to_db_time, utc_now, Db};
use crate::error::{AppError, AppResult};
use crate::models::{
    BulkActionInput, CreateTaskInput, PurgeResult, SoftDeleteResult, Task, TaskQuery,
    TodayOverview, UpdateTaskInput,
};

/// 标题长度上限（§10 校验所有输入）
const TITLE_MAX: usize = 500;
/// 描述长度上限，防止把巨型文本塞进列表接口
const DESCRIPTION_MAX: usize = 20_000;
/// 单页最大返回条数，避免一次拉爆内存（§10 上千条任务仍要流畅）
const PAGE_MAX: i64 = 1000;

/// 应用运行期状态，由 Tauri 管理并注入到命令中。
pub struct AppState {
    /// 数据库句柄
    pub db: Db,
    /// 提醒调度是否处于暂停状态（§8.6 托盘「暂停提醒」）
    pub reminders_paused: std::sync::atomic::AtomicBool,
}

impl AppState {
    /// 构造状态
    pub fn new(db: Db) -> Self {
        Self {
            db,
            reminders_paused: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

// =============================================================================
// 内部工具
// =============================================================================

/// 校验标题
fn validate_title(raw: &str) -> AppResult<String> {
    let t = raw.trim();
    if t.is_empty() {
        return Err(AppError::validation("标题不能为空").with_hint("请输入 1–500 个字符的标题"));
    }
    if t.chars().count() > TITLE_MAX {
        return Err(AppError::validation(format!(
            "标题过长（{} 字符），上限 {TITLE_MAX} 字符",
            t.chars().count()
        )));
    }
    Ok(t.to_string())
}

/// 校验描述类长文本
fn validate_long_text(field: &str, raw: &str) -> AppResult<String> {
    if raw.chars().count() > DESCRIPTION_MAX {
        return Err(AppError::validation(format!(
            "{field}过长（{} 字符），上限 {DESCRIPTION_MAX} 字符",
            raw.chars().count()
        )));
    }
    Ok(raw.to_string())
}

/// 校验优先级（§4.1 四级：0 无 / 1 低 / 2 中 / 3 高）
fn validate_priority(p: i64) -> AppResult<i64> {
    if (0..=3).contains(&p) {
        Ok(p)
    } else {
        Err(AppError::validation(format!("优先级取值非法：{p}")).with_hint("优先级只能是 0–3"))
    }
}

/// 校验状态字符串
fn validate_status(s: &str) -> AppResult<&'static str> {
    Ok(match s {
        "todo" => "todo",
        "doing" => "doing",
        "waiting" => "waiting",
        "done" => "done",
        "archived" => "archived",
        other => {
            return Err(AppError::validation(format!("任务状态非法：{other}"))
                .with_hint("允许值：todo / doing / waiting / done / archived"))
        }
    })
}

/// 校验时间字符串必须是 UTC ISO-8601，否则排序与比较语义会被破坏。
///
/// §4.3：时间区分"仅日期"和"精确时间"。此处只校验格式；
/// 是否含时刻由 `has_*_time` 表达，且要求仅日期时时间部分归一为 00:00:00。
fn validate_time(field: &str, raw: &str) -> AppResult<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(raw).map_err(|_| {
        AppError::validation(format!("{field}格式不正确：{raw}"))
            .with_hint("请使用 ISO-8601 且带时区，例如 2026-09-23T09:00:00+08:00")
    })?;
    Ok(to_db_time(parsed.with_timezone(&chrono::Utc)))
}

/// 校验可选时间：空字符串视为未设置（前端表单常见），避免把 "" 当有效时间。
fn validate_opt_time(field: &str, raw: Option<&String>) -> AppResult<Option<String>> {
    match raw {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => Ok(Some(validate_time(field, s)?)),
    }
}

/// 校验非空外键：要求目标存在且未被软删除，避免指向已删除的项目/分类。
async fn ensure_exists(
    db: &Db,
    table: &str,
    id: &str,
    label: &str,
) -> AppResult<()> {
    // table 只可能来自本文件内的字面量，不存在拼接注入风险
    let sql = match table {
        "projects" => "SELECT COUNT(*) AS c FROM projects WHERE id = ?1 AND deleted_at IS NULL",
        "categories" => "SELECT COUNT(*) AS c FROM categories WHERE id = ?1 AND deleted_at IS NULL",
        "tags" => "SELECT COUNT(*) AS c FROM tags WHERE id = ?1 AND deleted_at IS NULL",
        _ => return Err(AppError::internal("内部错误：非法表名")),
    };
    let row = sqlx::query(sql).bind(id).fetch_one(db.pool()).await?;
    let c: i64 = row.try_get("c")?;
    if c == 0 {
        return Err(AppError::not_found(label, id)
            .with_hint("该记录可能已被删除，请刷新后重试"));
    }
    Ok(())
}

/// 读取下一个手动排序值（§4.1 手动排序）
async fn next_sort_order(db: &Db) -> AppResult<f64> {
    let row = sqlx::query("SELECT COALESCE(MAX(sort_order), 0) + 1 AS n FROM tasks")
        .fetch_one(db.pool())
        .await?;
    Ok(row.try_get::<f64, _>("n")?)
}

/// 按 ID 读取单个任务
async fn get_task_row(db: &Db, id: &str) -> AppResult<Task> {
    let task = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?1")
        .bind(id)
        .fetch_optional(db.pool())
        .await?;
    task.ok_or_else(|| AppError::not_found("任务", id))
}

// =============================================================================
// 任务 CRUD
// =============================================================================

/// 创建任务。返回落库后的完整任务对象（含服务端生成的时间戳）。
///
/// 任务书 §4.4/§6：AI 或自然语言解析的结果必须先经用户确认，
/// 因此本命令不做任何隐式字段推断，传入什么就存什么。
#[tauri::command]
pub async fn task_create(
    state: State<'_, AppState>,
    input: CreateTaskInput,
) -> AppResult<Task> {
    let db = &state.db;
    let title = validate_title(&input.title)?;
    let description = validate_long_text("描述", input.description.as_deref().unwrap_or(""))?;
    let note_md = validate_long_text("备注", input.note_md.as_deref().unwrap_or(""))?;
    let status = validate_status(&input.status)?;
    let priority = validate_priority(input.priority.unwrap_or(0))?;

    if let Some(pid) = input.project_id.as_deref().filter(|s| !s.is_empty()) {
        ensure_exists(db, "projects", pid, "项目").await?;
    }
    if let Some(cid) = input.category_id.as_deref().filter(|s| !s.is_empty()) {
        ensure_exists(db, "categories", cid, "分类").await?;
    }
    for tid in input.tag_ids.iter().filter(|s| !s.is_empty()) {
        ensure_exists(db, "tags", tid, "标签").await?;
    }

    let planned = validate_opt_time("计划时间", input.planned_at.as_ref())?;
    let due = validate_opt_time("截止时间", input.due_at.as_ref())?;

    if let Some(m) = input.estimated_minutes {
        if !(0..=600_000).contains(&m) {
            return Err(AppError::validation(format!("预计耗时非法：{m} 分钟")));
        }
    }

    let id = uuid::Uuid::now_v7().to_string();
    let now = to_db_time(utc_now());
    let sort_order = next_sort_order(db).await?;

    let mut tx = db.pool().begin().await?;

    sqlx::query(
        "INSERT INTO tasks (
            id, title, description, note_md, link_url,
            status, priority, project_id, category_id,
            planned_at, has_planned_time, due_at, has_due_time,
            estimated_minutes, actual_minutes,
            completed_at, created_at, updated_at,
            sort_order, is_pinned, is_favorite,
            series_id, occurrence_key, occurrence_index, occurrence_kind, is_exception
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5,
            ?6, ?7, ?8, ?9,
            ?10, ?11, ?12, ?13,
            ?14, 0,
            ?15, ?16, ?16,
            ?17, ?18, ?19,
            NULL, NULL, NULL, 'single', 0
         )",
    )
    .bind(&id)
    .bind(&title)
    .bind(&description)
    .bind(&note_md)
    .bind(input.link_url.as_deref().filter(|s| !s.trim().is_empty()))
    .bind(status)
    .bind(priority)
    .bind(input.project_id.as_deref().filter(|s| !s.is_empty()))
    .bind(input.category_id.as_deref().filter(|s| !s.is_empty()))
    .bind(planned.as_deref())
    .bind(input.has_planned_time.unwrap_or(false) as i64)
    .bind(due.as_deref())
    .bind(input.has_due_time.unwrap_or(false) as i64)
    .bind(input.estimated_minutes)
    // 若创建时就标记为已完成，必须同时写入真实完成时间（§4.1）
    .bind(if status == "done" { Some(now.clone()) } else { None })
    .bind(&now)
    .bind(sort_order)
    .bind(input.is_pinned.unwrap_or(false) as i64)
    .bind(input.is_favorite.unwrap_or(false) as i64)
    .execute(&mut *tx)
    .await?;

    for tid in input.tag_ids.iter().filter(|s| !s.is_empty()) {
        sqlx::query("INSERT OR IGNORE INTO task_tags (task_id, tag_id) VALUES (?1, ?2)")
            .bind(&id)
            .bind(tid)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    get_task_row(db, &id).await
}

/// 更新任务（部分更新）。
#[tauri::command]
pub async fn task_update(
    state: State<'_, AppState>,
    id: String,
    input: UpdateTaskInput,
) -> AppResult<Task> {
    let db = &state.db;
    let existing = get_task_row(db, &id).await?;
    if existing.deleted_at.is_some() {
        return Err(AppError::conflict("任务已在回收站中，请先恢复再编辑"));
    }

    let title = match &input.title {
        Some(t) => Some(validate_title(t)?),
        None => None,
    };
    let description = match &input.description {
        Some(d) => Some(validate_long_text("描述", d)?),
        None => None,
    };
    let note_md = match &input.note_md {
        Some(n) => Some(validate_long_text("备注", n)?),
        None => None,
    };
    let status = match &input.status {
        Some(s) => Some(validate_status(s)?),
        None => None,
    };
    let priority = match input.priority {
        Some(p) => Some(validate_priority(p)?),
        None => None,
    };

    if let Some(pid) = input.project_id.as_deref().filter(|s| !s.is_empty()) {
        ensure_exists(db, "projects", pid, "项目").await?;
    }
    if let Some(cid) = input.category_id.as_deref().filter(|s| !s.is_empty()) {
        ensure_exists(db, "categories", cid, "分类").await?;
    }

    let planned = validate_opt_time("计划时间", input.planned_at.as_ref())?;
    let due = validate_opt_time("截止时间", input.due_at.as_ref())?;

    let now = to_db_time(utc_now());
    let mut tx = db.pool().begin().await?;

    // 完成时间必须真实记录，并且在状态来回切换时保持一致语义：
    // 进入 done 且原先没有完成时间 → 写入 now；离开 done → 清空。
    let new_completed_at: Option<Option<String>> = match status {
        Some("done") => Some(existing.completed_at.clone().or_else(|| Some(now.clone()))),
        Some(_) => Some(None),
        None => None,
    };

    let mut b = QueryBuilder::<Sqlite>::new("UPDATE tasks SET ");
    let mut sep = b.separated(", ");

    if let Some(v) = &title {
        sep.push("title = ").push_bind(v);
    }
    if let Some(v) = &description {
        sep.push("description = ").push_bind(v);
    }
    if let Some(v) = &note_md {
        sep.push("note_md = ").push_bind(v);
    }
    if let Some(v) = &status {
        sep.push("status = ").push_bind(*v);
    }
    if let Some(v) = priority {
        sep.push("priority = ").push_bind(v);
    }
    if let Some(v) = &new_completed_at {
        sep.push("completed_at = ").push_bind(v.clone());
    }

    // 项目/分类：支持显式清空
    if input.clear_project {
        sep.push("project_id = ").push_bind(None::<String>);
    } else if let Some(v) = input.project_id.as_deref().filter(|s| !s.is_empty()) {
        sep.push("project_id = ").push_bind(v.to_string());
    }
    if input.clear_category {
        sep.push("category_id = ").push_bind(None::<String>);
    } else if let Some(v) = input.category_id.as_deref().filter(|s| !s.is_empty()) {
        sep.push("category_id = ").push_bind(v.to_string());
    }

    // 计划时间：清空优先于赋值，避免两者同时传时行为歧义
    if input.clear_planned_at {
        sep.push("planned_at = ").push_bind(None::<String>);
        sep.push("has_planned_time = ").push_bind(0i64);
    } else if let Some(v) = &planned {
        sep.push("planned_at = ").push_bind(v.clone());
        if let Some(h) = input.has_planned_time {
            sep.push("has_planned_time = ").push_bind(h as i64);
        }
    } else if let Some(h) = input.has_planned_time {
        // 只切换"是否含具体时刻"，保留原日期
        sep.push("has_planned_time = ").push_bind(h as i64);
    }

    // 截止时间：同上
    if input.clear_due_at {
        sep.push("due_at = ").push_bind(None::<String>);
        sep.push("has_due_time = ").push_bind(0i64);
    } else if let Some(v) = &due {
        sep.push("due_at = ").push_bind(v.clone());
        if let Some(h) = input.has_due_time {
            sep.push("has_due_time = ").push_bind(h as i64);
        }
    } else if let Some(h) = input.has_due_time {
        sep.push("has_due_time = ").push_bind(h as i64);
    }

    if input.clear_link {
        sep.push("link_url = ").push_bind(None::<String>);
    } else if let Some(v) = input.link_url.as_deref().filter(|s| !s.trim().is_empty()) {
        sep.push("link_url = ").push_bind(v.to_string());
    }

    if let Some(v) = input.estimated_minutes {
        sep.push("estimated_minutes = ").push_bind(v);
    }
    if let Some(v) = input.actual_minutes {
        sep.push("actual_minutes = ").push_bind(v);
    }
    if let Some(v) = input.is_pinned {
        sep.push("is_pinned = ").push_bind(v as i64);
    }
    if let Some(v) = input.is_favorite {
        sep.push("is_favorite = ").push_bind(v as i64);
    }

    sep.push("updated_at = ").push_bind(now);
    b.push(" WHERE id = ").push_bind(&id);

    let affected = b.build().execute(&mut *tx).await?.rows_affected();
    if affected == 0 {
        return Err(AppError::not_found("任务", &id));
    }

    tx.commit().await?;
    get_task_row(db, &id).await
}

/// 完成 / 撤销完成。
///
/// §5 明确：完成操作只针对本次（单个实例），不弹范围选择。
#[tauri::command]
pub async fn task_toggle_done(
    state: State<'_, AppState>,
    id: String,
    done: bool,
) -> AppResult<Task> {
    let db = &state.db;
    // 先确认任务存在：不存在时返回 not_found，而不是静默影响 0 行
    get_task_row(db, &id).await?;
    let now = to_db_time(utc_now());

    // 注意用 clone：now 后面还要绑定给 updated_at，不能被移走
    let (status, completed_at) = if done {
        ("done", Some(now.clone()))
    } else {
        ("todo", None)
    };

    sqlx::query("UPDATE tasks SET status = ?1, completed_at = ?2, updated_at = ?3 WHERE id = ?4")
        .bind(status)
        .bind(completed_at)
        .bind(&now)
        .bind(&id)
        .execute(db.pool())
        .await?;

    get_task_row(db, &id).await
}

/// 软删除（移入回收站）。
///
/// §4.1：支持软删除与回收站恢复；保留 `deleted_at` 以便"重要操作可撤销"。
#[tauri::command]
pub async fn task_soft_delete(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<SoftDeleteResult> {
    let db = &state.db;
    get_task_row(db, &id).await?; // 不存在则报 not_found
    let now = to_db_time(utc_now());
    sqlx::query("UPDATE tasks SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2")
        .bind(&now)
        .bind(&id)
        .execute(db.pool())
        .await?;
    Ok(SoftDeleteResult { id, deleted_at: now, movable_to_trash: true })
}

/// 从回收站恢复。
#[tauri::command]
pub async fn task_restore(state: State<'_, AppState>, id: String) -> AppResult<Task> {
    let db = &state.db;
    let row = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?1")
        .bind(&id)
        .fetch_optional(db.pool())
        .await?;
    let Some(t) = row else {
        return Err(AppError::not_found("任务", &id));
    };
    if t.deleted_at.is_none() {
        return Err(AppError::conflict("该任务不在回收站中"));
    }
    let now = to_db_time(utc_now());
    sqlx::query("UPDATE tasks SET deleted_at = NULL, updated_at = ?1 WHERE id = ?2")
        .bind(&now)
        .bind(&id)
        .execute(db.pool())
        .await?;
    get_task_row(db, &id).await
}

/// 永久删除（仅限回收站内的任务）。
///
/// 安全约束：必须先软删除，避免误触造成不可恢复的数据丢失。
#[tauri::command]
pub async fn task_purge(state: State<'_, AppState>, id: String) -> AppResult<PurgeResult> {
    let db = &state.db;
    let t = get_task_row(db, &id).await?;
    if t.deleted_at.is_none() {
        return Err(AppError::conflict("只能永久删除回收站中的任务")
            .with_hint("请先将其移入回收站"));
    }
    sqlx::query("DELETE FROM tasks WHERE id = ?1")
        .bind(&id)
        .execute(db.pool())
        .await?;
    Ok(PurgeResult { purged: 1 })
}

/// 清空回收站。
#[tauri::command]
pub async fn task_purge_all_deleted(state: State<'_, AppState>) -> AppResult<PurgeResult> {
    let db = &state.db;
    let n = sqlx::query("DELETE FROM tasks WHERE deleted_at IS NOT NULL")
        .execute(db.pool())
        .await?
        .rows_affected() as i64;
    Ok(PurgeResult { purged: n })
}

/// 按条件查询任务列表（§4.1 搜索 + 组合筛选 + 可切换排序）。
#[tauri::command]
pub async fn task_list(state: State<'_, AppState>, query: TaskQuery) -> AppResult<Vec<Task>> {
    let db = &state.db;
    let mut b = QueryBuilder::<Sqlite>::new("SELECT * FROM tasks WHERE 1 = 1");

    // 删除状态
    if query.deleted_only {
        b.push(" AND deleted_at IS NOT NULL");
    } else if !query.include_deleted {
        b.push(" AND deleted_at IS NULL");
    }

    // 状态过滤
    if !query.statuses.is_empty() {
        let mut valid: Vec<&'static str> = Vec::new();
        for s in &query.statuses {
            valid.push(validate_status(s)?);
        }
        b.push(" AND status IN (");
        let mut sep = b.separated(", ");
        for v in valid {
            sep.push_bind(v.to_string());
        }
        sep.push_unseparated(")");
    }

    if let Some(pid) = query.project_id.as_deref().filter(|s| !s.is_empty()) {
        b.push(" AND project_id = ").push_bind(pid.to_string());
    }
    if let Some(cid) = query.category_id.as_deref().filter(|s| !s.is_empty()) {
        b.push(" AND category_id = ").push_bind(cid.to_string());
    }

    if !query.priorities.is_empty() {
        for p in &query.priorities {
            validate_priority(*p)?;
        }
        b.push(" AND priority IN (");
        let mut sep = b.separated(", ");
        for p in &query.priorities {
            sep.push_bind(*p);
        }
        sep.push_unseparated(")");
    }

    // 标签过滤：要求任务拥有全部指定标签（AND 语义，便于组合筛选）
    for tid in query.tag_ids.iter().filter(|s| !s.is_empty()) {
        b.push(" AND EXISTS (SELECT 1 FROM task_tags tt WHERE tt.task_id = tasks.id AND tt.tag_id = ")
            .push_bind(tid.clone())
            .push(")");
    }

    if let Some(from) = validate_opt_time("计划起始", query.planned_from.as_ref())? {
        b.push(" AND planned_at >= ").push_bind(from);
    }
    if let Some(to) = validate_opt_time("计划结束", query.planned_to.as_ref())? {
        b.push(" AND planned_at <= ").push_bind(to);
    }
    if let Some(from) = validate_opt_time("截止起始", query.due_from.as_ref())? {
        b.push(" AND due_at >= ").push_bind(from);
    }
    if let Some(to) = validate_opt_time("截止结束", query.due_to.as_ref())? {
        b.push(" AND due_at <= ").push_bind(to);
    }

    // 搜索：标题/描述/备注/链接（§4.1 要求可搜索项目与标签，此处额外用 EXISTS 覆盖）
    if let Some(kw) = query.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let like = format!("%{}%", kw.replace('%', "\\%").replace('_', "\\_"));
        b.push(" AND (title LIKE ").push_bind(like.clone()).push(" ESCAPE '\\'");
        b.push(" OR description LIKE ").push_bind(like.clone()).push(" ESCAPE '\\'");
        b.push(" OR note_md LIKE ").push_bind(like.clone()).push(" ESCAPE '\\'");
        b.push(" OR COALESCE(link_url, '') LIKE ").push_bind(like.clone()).push(" ESCAPE '\\'");
        b.push(" OR EXISTS (SELECT 1 FROM projects p WHERE p.id = tasks.project_id AND p.name LIKE ")
            .push_bind(like.clone())
            .push(" ESCAPE '\\')");
        b.push(" OR EXISTS (SELECT 1 FROM tags tg JOIN task_tags tt2 ON tt2.tag_id = tg.id WHERE tt2.task_id = tasks.id AND tg.name LIKE ")
            .push_bind(like)
            .push(" ESCAPE '\\'))");
    }

    match query.is_recurring {
        Some(true) => {
            b.push(" AND series_id IS NOT NULL");
        }
        Some(false) => {
            b.push(" AND series_id IS NULL");
        }
        None => {}
    }

    if query.overdue_only {
        let now = query
            .now_utc
            .as_deref()
            .map(|s| validate_time("当前时间", s))
            .transpose()?
            .unwrap_or_else(|| to_db_time(utc_now()));
        b.push(" AND due_at IS NOT NULL AND due_at < ")
            .push_bind(now)
            .push(" AND status NOT IN ('done', 'archived')");
    }

    // 排序
    let desc = query.sort_desc.unwrap_or(false);
    let order = match query.sort_by.as_deref().unwrap_or("manual") {
        "manual" => "is_pinned DESC, sort_order ASC, created_at DESC",
        "due" => {
            if desc {
                "due_at IS NULL, due_at DESC"
            } else {
                "due_at IS NULL, due_at ASC"
            }
        }
        "priority" => {
            if desc {
                "priority DESC, created_at DESC"
            } else {
                "priority ASC, created_at DESC"
            }
        }
        "created" => {
            if desc {
                "created_at DESC"
            } else {
                "created_at ASC"
            }
        }
        "planned" => {
            if desc {
                "planned_at IS NULL, planned_at DESC"
            } else {
                "planned_at IS NULL, planned_at ASC"
            }
        }
        "title" => {
            if desc {
                "title DESC"
            } else {
                "title ASC"
            }
        }
        other => {
            return Err(AppError::validation(format!("不支持的排序方式：{other}")).with_hint(
                "允许值：manual / due / priority / created / planned / title",
            ))
        }
    };
    b.push(" ORDER BY ").push(order);

    let limit = query.limit.unwrap_or(300).clamp(1, PAGE_MAX);
    b.push(" LIMIT ").push_bind(limit);
    if let Some(off) = query.offset {
        b.push(" OFFSET ").push_bind(off.max(0));
    }

    let rows = b.build_query_as::<Task>().fetch_all(db.pool()).await?;
    Ok(rows)
}

/// 读取单个任务（含已删除的，用于回收站详情）。
#[tauri::command]
pub async fn task_get(state: State<'_, AppState>, id: String) -> AppResult<Task> {
    get_task_row(&state.db, &id).await
}

/// 批量操作（§4.1 批量操作）。
///
/// 全部在一个事务内完成，避免"部分成功"导致 UI 与数据库不一致。
#[tauri::command]
pub async fn task_bulk(
    state: State<'_, AppState>,
    input: BulkActionInput,
) -> AppResult<i64> {
    let db = &state.db;
    if input.ids.is_empty() {
        return Err(AppError::validation("请至少选择一项任务"));
    }
    if input.ids.len() > 5000 {
        return Err(AppError::validation("单次批量操作最多 5000 项"));
    }

    let now = to_db_time(utc_now());
    let mut tx = db.pool().begin().await?;
    let mut affected: i64 = 0;

    for id in &input.ids {
        let n = match input.action.as_str() {
            "complete" => sqlx::query(
                "UPDATE tasks SET status = 'done', completed_at = COALESCE(completed_at, ?1), updated_at = ?1 WHERE id = ?2",
            )
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected(),

            "uncomplete" => sqlx::query(
                "UPDATE tasks SET status = 'todo', completed_at = NULL, updated_at = ?1 WHERE id = ?2",
            )
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected(),

            "delete" => sqlx::query(
                "UPDATE tasks SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
            )
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected(),

            "restore" => sqlx::query(
                "UPDATE tasks SET deleted_at = NULL, updated_at = ?1 WHERE id = ?2 AND deleted_at IS NOT NULL",
            )
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected(),

            "archive" => sqlx::query(
                "UPDATE tasks SET status = 'archived', updated_at = ?1 WHERE id = ?2",
            )
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected(),

            "set_priority" => {
                let p = validate_priority(input.priority.ok_or_else(|| {
                    AppError::validation("set_priority 必须提供 priority")
                })?)?;
                sqlx::query("UPDATE tasks SET priority = ?1, updated_at = ?2 WHERE id = ?3")
                    .bind(p)
                    .bind(&now)
                    .bind(id)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected()
            }

            "move_project" => {
                if let Some(pid) = input.project_id.as_deref().filter(|s| !s.is_empty()) {
                    ensure_exists(db, "projects", pid, "项目").await?;
                }
                sqlx::query("UPDATE tasks SET project_id = ?1, updated_at = ?2 WHERE id = ?3")
                    .bind(input.project_id.as_deref().filter(|s| !s.is_empty()))
                    .bind(&now)
                    .bind(id)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected()
            }

            "add_tag" => {
                let tid = input.tag_id.as_deref().ok_or_else(|| {
                    AppError::validation("add_tag 必须提供 tagId")
                })?;
                ensure_exists(db, "tags", tid, "标签").await?;
                sqlx::query("INSERT OR IGNORE INTO task_tags (task_id, tag_id) VALUES (?1, ?2)")
                    .bind(id)
                    .bind(tid)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected()
            }

            "remove_tag" => {
                let tid = input.tag_id.as_deref().ok_or_else(|| {
                    AppError::validation("remove_tag 必须提供 tagId")
                })?;
                sqlx::query("DELETE FROM task_tags WHERE task_id = ?1 AND tag_id = ?2")
                    .bind(id)
                    .bind(tid)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected()
            }

            other => {
                return Err(AppError::validation(format!("不支持的批量操作：{other}")).with_hint(
                    "允许值：complete / uncomplete / delete / restore / archive / set_priority / move_project / add_tag / remove_tag",
                ))
            }
        };
        affected += n as i64;
    }

    tx.commit().await?;
    Ok(affected)
}

/// 今日概览。托盘菜单、悬浮小窗、主界面「今天」共用同一份计算口径（§4.2/§8.6）。
///
/// 口径说明（界面必须展示同样的说明，避免用户误解）：
/// - `plannedTotal`：计划时间落在 [今日 00:00, 明日 00:00) 本地时区内的未删除任务
/// - `dueTotal`：截止时间落在同一区间
/// - `overdue`：截止时间早于 now 且未完成、未归档
/// - `openTotal`：上述今日任务中未完成的数量（托盘图标/菜单显示）
#[tauri::command]
pub async fn today_overview(
    state: State<'_, AppState>,
    day_start_utc: String,
    day_end_utc: String,
    now_utc: Option<String>,
) -> AppResult<TodayOverview> {
    let db = &state.db;
    let start = validate_time("今日起点", &day_start_utc)?;
    let end = validate_time("今日终点", &day_end_utc)?;
    let now = match now_utc.as_deref() {
        Some(s) => validate_time("当前时间", s)?,
        None => to_db_time(utc_now()),
    };

    let row = sqlx::query(
        "SELECT
            (SELECT COUNT(*) FROM tasks
               WHERE deleted_at IS NULL AND status NOT IN ('archived')
                 AND planned_at IS NOT NULL AND planned_at >= ?1 AND planned_at < ?2) AS planned_total,
            (SELECT COUNT(*) FROM tasks
               WHERE deleted_at IS NULL AND status = 'done'
                 AND planned_at IS NOT NULL AND planned_at >= ?1 AND planned_at < ?2) AS planned_done,
            (SELECT COUNT(*) FROM tasks
               WHERE deleted_at IS NULL AND status NOT IN ('archived')
                 AND due_at IS NOT NULL AND due_at >= ?1 AND due_at < ?2) AS due_total,
            (SELECT COUNT(*) FROM tasks
               WHERE deleted_at IS NULL AND status NOT IN ('done', 'archived')
                 AND due_at IS NOT NULL AND due_at < ?3) AS overdue,
            (SELECT COUNT(*) FROM tasks
               WHERE deleted_at IS NULL AND status NOT IN ('done', 'archived')
                 AND planned_at IS NOT NULL AND planned_at >= ?1 AND planned_at < ?2) AS open_total",
    )
    .bind(&start)
    .bind(&end)
    .bind(&now)
    .fetch_one(db.pool())
    .await?;

    Ok(TodayOverview {
        planned_total: row.try_get("planned_total")?,
        planned_done: row.try_get("planned_done")?,
        due_total: row.try_get("due_total")?,
        overdue: row.try_get("overdue")?,
        open_total: row.try_get("open_total")?,
    })
}

/// 数据目录信息（§9：提供数据目录查看、一键打开日志目录）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataPaths {
    /// 数据目录（数据库、附件、备份所在）
    pub data_dir: String,
    /// 数据库文件
    pub db_path: String,
    /// 备份目录
    pub backup_dir: String,
    /// 当前 schema 版本
    pub schema_version: String,
}

/// 查询数据目录与 schema 版本，供设置页展示。
#[tauri::command]
pub async fn app_data_paths(state: State<'_, AppState>) -> AppResult<DataPaths> {
    let db = &state.db;
    let ver: Option<(String,)> =
        sqlx::query_as("SELECT value FROM app_meta WHERE key = 'schema_version'")
            .fetch_optional(db.pool())
            .await?;
    Ok(DataPaths {
        data_dir: db.data_dir().to_string_lossy().to_string(),
        db_path: db.db_path().to_string_lossy().to_string(),
        backup_dir: db.data_dir().join("backups").to_string_lossy().to_string(),
        schema_version: ver.map(|v| v.0).unwrap_or_else(|| "未知".into()),
    })
}

/// 应用与数据库健康信息，用于「关于」与故障排查。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// 应用版本
    pub version: String,
    /// 状态：ok / degraded
    pub status: String,
    /// 任务总数（不含已删除）
    pub task_count: i64,
    /// 已删除任务数（回收站）
    pub trash_count: i64,
    /// 当前是否暂停提醒
    pub reminders_paused: bool,
}

/// 读取应用健康信息。
#[tauri::command]
pub async fn app_info(state: State<'_, AppState>) -> AppResult<AppInfo> {
    let db = &state.db;
    let row = sqlx::query(
        "SELECT
            (SELECT COUNT(*) FROM tasks WHERE deleted_at IS NULL) AS alive,
            (SELECT COUNT(*) FROM tasks WHERE deleted_at IS NOT NULL) AS trashed",
    )
    .fetch_one(db.pool())
    .await?;

    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        status: "ok".to_string(),
        task_count: row.try_get("alive")?,
        trash_count: row.try_get("trashed")?,
        reminders_paused: state
            .reminders_paused
            .load(std::sync::atomic::Ordering::Relaxed),
    })
}

/// 切换提醒暂停状态（§8.6 托盘「暂停提醒」）。
#[tauri::command]
pub async fn set_reminders_paused(
    state: State<'_, AppState>,
    paused: bool,
) -> AppResult<bool> {
    state
        .reminders_paused
        .store(paused, std::sync::atomic::Ordering::Relaxed);
    Ok(paused)
}

/// 前端的 Ready 探针：确认后端、数据库与迁移均已就绪。
#[tauri::command]
pub async fn ping(state: State<'_, AppState>) -> AppResult<String> {
    // 真正打一次数据库，确认连接池可用而不只是进程活着
    let row = sqlx::query("SELECT 1 AS one").fetch_one(state.db.pool()).await?;
    let one: i64 = row.try_get("one")?;
    if one == 1 {
        Ok("pong".to_string())
    } else {
        Err(AppError::internal("数据库探针返回值异常"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_validation_rejects_empty_and_whitespace() {
        assert!(validate_title("").is_err());
        assert!(validate_title("   ").is_err());
        assert!(validate_title("\t\n ").is_err());
    }

    #[test]
    fn title_validation_trims_and_accepts_chinese() {
        assert_eq!(validate_title("  写周报  ").unwrap(), "写周报");
    }

    #[test]
    fn title_length_limit_counts_chars_not_bytes() {
        // 500 个汉字应当通过（UTF-8 下是 1500 字节，按字节判断会误报）
        let ok = "任".repeat(500);
        assert!(validate_title(&ok).is_ok());
        let too_long = "任".repeat(501);
        assert!(validate_title(&too_long).is_err());
    }

    #[test]
    fn priority_accepts_exactly_four_levels() {
        for p in 0..=3 {
            assert!(validate_priority(p).is_ok());
        }
        assert!(validate_priority(4).is_err());
        assert!(validate_priority(-1).is_err());
        // §4.1「四级优先级」的具体含义必须稳定
        assert_eq!(validate_priority(3).unwrap(), 3);
    }

    #[test]
    fn status_validation_lists_allowed_values_on_error() {
        assert_eq!(validate_status("doing").unwrap(), "doing");
        let err = validate_status("nope").unwrap_err();
        assert!(err.message.contains("nope"), "错误消息应包含非法值便于排查");
        assert!(err.hint.as_deref().unwrap_or("").contains("todo"));
    }

    #[test]
    fn time_validation_normalizes_offset_to_utc() {
        // 北京时间 09:00 == UTC 01:00
        let got = validate_time("计划时间", "2026-09-23T09:00:00+08:00").unwrap();
        assert_eq!(got, "2026-09-23T01:00:00.000Z");
    }

    #[test]
    fn time_validation_rejects_non_iso_and_bare_date() {
        // 纯日期不能再被当作"今天零点"静默接受——时区语义不明确（§4.3）
        assert!(validate_time("截止时间", "2026-09-23").is_err());
        assert!(validate_time("截止时间", "tomorrow").is_err());
        assert!(validate_time("截止时间", "").is_err());
    }

    #[test]
    fn empty_string_time_is_treated_as_unset_not_as_error() {
        // 前端表单清空日期后常提交 ""，必须视为"未设置"
        assert_eq!(validate_opt_time("计划时间", None).unwrap(), None);
        let s = String::new();
        assert_eq!(validate_opt_time("计划时间", Some(&s)).unwrap(), None);
    }

    #[test]
    fn time_comparison_is_lexicographically_safe() {
        // 全库时间统一为固定宽度 UTC 字符串，字符串比较等价于时间比较
        let a = validate_time("x", "2026-09-23T01:00:00+08:00").unwrap();
        let b = validate_time("x", "2026-09-22T20:00:00Z").unwrap();
        assert!(b < a, "2026-09-22T20:00Z 早于 2026-09-23T01:00Z");
    }
}
