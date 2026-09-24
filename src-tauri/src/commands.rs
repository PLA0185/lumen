//! 任务 CRUD 等 IPC 命令实现。
//!
//! 约定（任务书 §4.1 / §9 / §10）：
//! - 所有写操作走事务，避免"留下半条规则"。
//! - 删除一律软删除；永久删除仅限回收站显式操作。
//! - 输入全部校验后才落库，错误以 `AppError` 返回可读中文消息。

use serde::{Deserialize, Serialize};
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
    /// 调度循环是否应停止（退出流程使用）
    pub scheduler_stopped: std::sync::atomic::AtomicBool,
    /// 程序关闭期间错过的提醒，最多补发多少分钟内的（0 = 不补发）
    /// §4.3 要求"重启、休眠唤醒后重算待提醒项，避免漏发或重复轰炸"，
    /// 这个窗口就是"漏发"与"轰炸"之间的平衡点，且可由用户调整。
    pub missed_grace_minutes: std::sync::atomic::AtomicI64,

    /// 窗口配置的内存缓存。
    ///
    /// 为什么需要缓存而不是每次查数据库：托盘菜单的事件处理器是**同步**的，
    /// 而查库需要 async。若在那里调用 `block_on`，一旦外层已处于
    /// Tauri 的异步运行时中（例如 setup 阶段），就会造成 tokio 运行时
    /// 嵌套并直接 panic——这是实测踩到的崩溃。
    /// 因此配置以内存为准、数据库只作持久化，事件处理器只读内存。
    pub window_config: std::sync::RwLock<crate::window_mgr::WindowConfig>,

    /// 今日未完成数（托盘菜单显示用），同样为避免同步上下文查库而缓存。
    pub today_open_count: std::sync::atomic::AtomicI64,
}

impl AppState {
    /// 构造状态
    pub fn new(db: Db) -> Self {
        Self {
            db,
            reminders_paused: std::sync::atomic::AtomicBool::new(false),
            scheduler_stopped: std::sync::atomic::AtomicBool::new(false),
            // 默认 6 小时：足以覆盖"关机一晚后开机"的常见场景，
            // 又不至于把上周的提醒全部倒出来。
            missed_grace_minutes: std::sync::atomic::AtomicI64::new(6 * 60),
            window_config: std::sync::RwLock::new(crate::window_mgr::WindowConfig::default()),
            // -1 表示"尚未统计"，托盘菜单据此显示"今日概览"而不是假的 0
            today_open_count: std::sync::atomic::AtomicI64::new(-1),
        }
    }

    /// 读取窗口配置的内存副本（同步、不会 panic）
    pub fn cfg(&self) -> crate::window_mgr::WindowConfig {
        self.window_config
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// 更新窗口配置的内存副本
    pub fn set_cfg(&self, cfg: &crate::window_mgr::WindowConfig) {
        if let Ok(mut w) = self.window_config.write() {
            *w = cfg.clone();
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

/// 校验周期跨度（§4.1 的"这周/这个月做完就行"维度）。
///
/// 与 planned_at / due_at 互补，三者语义不同：
/// - `period_type` 表达"我在这个周期内完成"，**允许没有具体日期**；
/// - `planned_at`  表达"我打算这一天的这个时刻做"；
/// - `due_at`      表达"我必须在此之前交"。
///
/// 关键规则：period 型任务若没有 planned_at，**不会出现在「今天」视图**。
/// 否则"这周做完就行"的任务会每天弹出，反而比不定时间更烦人。
fn validate_period(v: &str) -> AppResult<&'static str> {
    Ok(match v.trim() {
        "none" | "" => "none",
        "day" => "day",
        "week" => "week",
        "month" => "month",
        "quarter" => "quarter",
        "year" => "year",
        other => {
            return Err(AppError::validation(format!("周期跨度非法：{other}")).with_hint(
                "允许值：none（不限）/ day / week / month / quarter / year",
            ))
        }
    })
}

/// 校验时间字符串必须是 RFC-3339，并**规范化**为固定宽度的 UTC 形式。
///
/// 为什么必须规范化成固定宽度（毫秒三位 + `Z`）：
/// 全库时间以 TEXT 存储，列表排序、区间筛选（`planned_at >= ?`）与逾期判断
/// 全都依赖**字符串字典序**。字典序要等价于时间序，前提是所有值宽度一致：
///   `2026-09-22T20:00:00Z`   ← 20 字符
///   `2026-09-22T20:00:00.000Z` ← 24 字符
/// 两者混存时，`'2026-09-22T20:00:00Z' > '2026-09-22T20:00:00.000Z'`
/// （因为 'Z' > '.'），排序与比较结果都会错乱。
/// 因此这里一律经 `to_db_time` 归一化，而不是把用户输入原样回显。
///
/// §4.3：是否含具体时刻由 `has_*_time` 表达，与本函数的格式无关。
fn validate_time(field: &str, raw: &str) -> AppResult<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(raw).map_err(|_| {
        AppError::validation(format!("{field}格式不正确：{raw}"))
            .with_hint("请使用 ISO-8601 且带时区，例如 2026-09-23T09:00:00+08:00")
    })?;
    // with_timezone(Utc) + to_db_time 保证输出恒为 `YYYY-MM-DDTHH:MM:SS.sssZ`
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

/// 读取下一个手动排序值（§4.1 手动排序）。
///
/// **必须 CAST 成 REAL**：`sort_order` 是 REAL 列，但空表时
/// `COALESCE(MAX(sort_order), 0) + 1` 的求值结果会被 SQLite 判成 INTEGER，
/// 于是 sqlx 按 f64 解码时报 "SQL type INTEGER is not compatible"。
/// 这个坑只在**全新数据库的第一条任务**上出现，很容易漏测。
async fn next_sort_order(db: &Db) -> AppResult<f64> {
    let row = sqlx::query("SELECT CAST(COALESCE(MAX(sort_order), 0) + 1 AS REAL) AS n FROM tasks")
        .fetch_one(db.pool())
        .await?;
    Ok(row.try_get::<f64, _>("n")?)
}

/// 按 ID 读取单个任务（`pub(crate)` 供集成测试复用）
pub(crate) async fn get_task_row(db: &Db, id: &str) -> AppResult<Task> {
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
    create_task_impl(&state.db, input).await
}

/// 创建任务的实现（与 Tauri 解耦，便于集成测试直接调用）。
pub async fn create_task_impl(db: &Db, input: CreateTaskInput) -> AppResult<Task> {
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
            series_id, occurrence_key, occurrence_index, occurrence_kind, is_exception,
            period_type
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5,
            ?6, ?7, ?8, ?9,
            ?10, ?11, ?12, ?13,
            ?14, 0,
            ?15, ?16, ?16,
            ?17, ?18, ?19,
            NULL, NULL, NULL, 'single', 0,
            ?20
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
    // 周期跨度：允许为空（默认 none）
    .bind(validate_period(
        input.period_type.as_deref().unwrap_or("none"),
    )?)
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
    update_task_impl(&state.db, &id, input).await
}

/// 更新任务的实现（与 Tauri 解耦，便于集成测试直接调用）。
pub async fn update_task_impl(db: &Db, id: &str, input: UpdateTaskInput) -> AppResult<Task> {
    let existing = get_task_row(db, id).await?;
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
        sep.push("title = ").push_bind_unseparated(v);
    }
    if let Some(v) = &description {
        sep.push("description = ").push_bind_unseparated(v);
    }
    if let Some(v) = &note_md {
        sep.push("note_md = ").push_bind_unseparated(v);
    }
    if let Some(v) = &status {
        sep.push("status = ").push_bind_unseparated(*v);
    }
    if let Some(v) = priority {
        sep.push("priority = ").push_bind_unseparated(v);
    }
    if let Some(v) = &new_completed_at {
        sep.push("completed_at = ").push_bind_unseparated(v.clone());
    }

    // 项目/分类：支持显式清空
    if input.clear_project {
        sep.push("project_id = ").push_bind_unseparated(None::<String>);
    } else if let Some(v) = input.project_id.as_deref().filter(|s| !s.is_empty()) {
        sep.push("project_id = ").push_bind_unseparated(v.to_string());
    }
    if input.clear_category {
        sep.push("category_id = ").push_bind_unseparated(None::<String>);
    } else if let Some(v) = input.category_id.as_deref().filter(|s| !s.is_empty()) {
        sep.push("category_id = ").push_bind_unseparated(v.to_string());
    }

    // 计划时间：清空优先于赋值，避免两者同时传时行为歧义
    if input.clear_planned_at {
        sep.push("planned_at = ").push_bind_unseparated(None::<String>);
        sep.push("has_planned_time = ").push_bind_unseparated(0i64);
    } else if let Some(v) = &planned {
        sep.push("planned_at = ").push_bind_unseparated(v.clone());
        if let Some(h) = input.has_planned_time {
            sep.push("has_planned_time = ").push_bind_unseparated(h as i64);
        }
    } else if let Some(h) = input.has_planned_time {
        // 只切换"是否含具体时刻"，保留原日期
        sep.push("has_planned_time = ").push_bind_unseparated(h as i64);
    }

    // 截止时间：同上
    if input.clear_due_at {
        sep.push("due_at = ").push_bind_unseparated(None::<String>);
        sep.push("has_due_time = ").push_bind_unseparated(0i64);
    } else if let Some(v) = &due {
        sep.push("due_at = ").push_bind_unseparated(v.clone());
        if let Some(h) = input.has_due_time {
            sep.push("has_due_time = ").push_bind_unseparated(h as i64);
        }
    } else if let Some(h) = input.has_due_time {
        sep.push("has_due_time = ").push_bind_unseparated(h as i64);
    }

    if input.clear_link {
        sep.push("link_url = ").push_bind_unseparated(None::<String>);
    } else if let Some(v) = input.link_url.as_deref().filter(|s| !s.trim().is_empty()) {
        sep.push("link_url = ").push_bind_unseparated(v.to_string());
    }

    if let Some(v) = input.estimated_minutes {
        sep.push("estimated_minutes = ").push_bind_unseparated(v);
    }
    if let Some(v) = input.actual_minutes {
        sep.push("actual_minutes = ").push_bind_unseparated(v);
    }
    if let Some(v) = input.is_pinned {
        sep.push("is_pinned = ").push_bind_unseparated(v as i64);
    }
    if let Some(v) = input.is_favorite {
        sep.push("is_favorite = ").push_bind_unseparated(v as i64);
    }
    // 周期跨度：传空串视为"取消周期"，与前端"不限"选项一致
    if let Some(v) = input.period_type.as_deref() {
        sep.push("period_type = ").push_bind_unseparated(validate_period(v)?.to_string());
    }

    sep.push("updated_at = ").push_bind_unseparated(now);
    b.push(" WHERE id = ").push_bind(id);

    let affected = b.build().execute(&mut *tx).await?.rows_affected();
    if affected == 0 {
        return Err(AppError::not_found("任务", id));
    }

    // §4.3「重算待提醒项」：用户改了计划/截止时间后，所有相对型提醒
    // （如"到期前 30 分钟"）都必须跟着移动，否则会在错误时间响起。
    //
    // 整改任务书 §5：这一步必须与上面的 UPDATE **在同一个事务里**完成。
    // 原来的写法是先 commit、再调用重算且把错误吞掉，于是可能出现
    // "任务截止时间 = 新值、提醒触发时间 = 旧值"而界面还显示保存成功。
    if input.planned_at.is_some()
        || input.due_at.is_some()
        || input.clear_planned_at
        || input.clear_due_at
    {
        crate::reminders::recompute_task_reminders_tx(&mut tx, id).await?;
    }

    tx.commit().await?;

    get_task_row(db, id).await
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
    list_tasks_impl(&state.db, query).await
}

/// 列表查询的实现（与 Tauri 解耦，便于集成测试与报告导出直接调用）。
pub async fn list_tasks_impl(db: &Db, query: TaskQuery) -> AppResult<Vec<Task>> {
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

    // 周期跨度过滤（「周任务」「月任务」等视图使用）
    if !query.period_types.is_empty() {
        let mut valid: Vec<&'static str> = Vec::new();
        for p in &query.period_types {
            valid.push(validate_period(p)?);
        }
        b.push(" AND period_type IN (");
        let mut psep = b.separated(", ");
        for v in valid {
            psep.push_bind(v.to_string());
        }
        psep.push_unseparated(")");
    }

    // 项目筛选：三种语义必须分清（详见 `TaskQuery::without_project` 的注释）。
    // 曾经的写法是"projectId 为 None 就当没这个条件"，导致收件箱退化成全部任务。
    if query.without_project {
        b.push(" AND project_id IS NULL");
    } else if let Some(pid) = query.project_id.as_deref().filter(|s| !s.is_empty()) {
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

/// 打印 / PDF 报告用的一行：任务本体 + 已解析好的归属名称。
///
/// 归属名称在 Rust 侧一次查完，而不是让前端对每条任务各调一次
/// （1000 条任务会产生 1000 次 IPC 往返，§10 明确禁止这种写法）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskReportRow {
    pub task: Task,
    pub project_name: Option<String>,
    pub category_name: Option<String>,
    pub tag_names: Vec<String>,
}

/// 取报告数据：复用 `task_list` 的筛选语义，再补上归属名称。
#[tauri::command]
pub async fn task_report(
    state: State<'_, AppState>,
    query: TaskQuery,
) -> AppResult<Vec<TaskReportRow>> {
    let tasks = task_list(state.clone(), query).await?;
    if tasks.is_empty() {
        return Ok(Vec::new());
    }
    let db = &state.db;

    // 项目与分类都是小表，整表取出后在内存里映射，避免拼接超长 IN 列表
    let projects: Vec<(String, String)> =
        sqlx::query_as("SELECT id, name FROM projects WHERE deleted_at IS NULL")
            .fetch_all(db.pool())
            .await?;
    let categories: Vec<(String, String)> =
        sqlx::query_as("SELECT id, name FROM categories WHERE deleted_at IS NULL")
            .fetch_all(db.pool())
            .await?;
    let project_map: std::collections::HashMap<String, String> = projects.into_iter().collect();
    let category_map: std::collections::HashMap<String, String> = categories.into_iter().collect();

    // 标签只查这批任务
    let mut b = QueryBuilder::<Sqlite>::new(
        "SELECT tt.task_id AS task_id, t.name AS name
         FROM task_tags tt JOIN tags t ON t.id = tt.tag_id
         WHERE t.deleted_at IS NULL AND tt.task_id IN (",
    );
    let mut sep = b.separated(", ");
    for t in &tasks {
        sep.push_bind(t.id.clone());
    }
    sep.push_unseparated(")");
    b.push(" ORDER BY t.sort_order ASC, t.name ASC");

    let tag_rows = b.build().fetch_all(db.pool()).await?;
    let mut tag_map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for r in tag_rows {
        let task_id: String = r.try_get("task_id")?;
        let name: String = r.try_get("name")?;
        tag_map.entry(task_id).or_default().push(name);
    }

    Ok(tasks
        .into_iter()
        .map(|t| TaskReportRow {
            project_name: t.project_id.as_ref().and_then(|id| project_map.get(id).cloned()),
            category_name: t.category_id.as_ref().and_then(|id| category_map.get(id).cloned()),
            tag_names: tag_map.get(&t.id).cloned().unwrap_or_default(),
            task: t,
        })
        .collect())
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

/// 日历视图：查询落在指定时间范围内的任务。
///
/// 与 `task_list` 的区别在于**归属判定规则**（§4.2 要求界面明确说明规则）：
/// 日历把任务显示到"它所属的那一天"，因此用 COALESCE(planned_at, due_at)
/// 作为落点；没有计划时间也没有截止时间的任务不会出现在日历中
/// （它们只属于列表视图）。
///
/// 前端按本地时区算出范围边界后传入 UTC，后端不做任何本地时区假设。
#[tauri::command]
pub async fn tasks_in_range(
    state: State<'_, AppState>,
    start_utc: String,
    end_utc: String,
) -> AppResult<Vec<Task>> {
    let db = &state.db;
    let start = validate_time("范围起点", &start_utc)?;
    let end = validate_time("范围终点", &end_utc)?;
    if end <= start {
        return Err(AppError::validation("时间范围的终点必须晚于起点"));
    }

    // 已归档与已删除的不进日历；已完成仍显示（用户想知道那天做了什么）
    let rows = sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks
         WHERE deleted_at IS NULL
           AND status <> 'archived'
           AND COALESCE(planned_at, due_at) IS NOT NULL
           AND COALESCE(planned_at, due_at) >= ?1
           AND COALESCE(planned_at, due_at) < ?2
         ORDER BY COALESCE(planned_at, due_at) ASC, priority DESC, created_at ASC",
    )
    .bind(&start)
    .bind(&end)
    .fetch_all(db.pool())
    .await?;

    Ok(rows)
}

/// 计算改期后的新计划时间。
///
/// 抽成纯函数是为了可测试——这段语义很容易写错，而错了用户会莫名发现
/// 时间变成 00:00。
///
/// 规则：
/// - 原任务是**精确时间**（has_planned_time = 1）→ 保留原时刻，只换日期；
/// - 原任务是**仅日期**（has_planned_time = 0）→ 用目标日期的零点，保持"仅日期"；
/// - 原本没有计划时间 → 直接用目标日期，仍标为"仅日期"（用户只拖了一下，
///   不该因此把任务变成有时刻的）。
///
/// 返回 `(新的 UTC 时间字符串, 是否含具体时刻)`。
fn compute_rescheduled_at(
    old_planned: Option<&str>,
    old_has_time: i64,
    target: chrono::DateTime<chrono::Utc>,
) -> (String, bool) {
    use chrono::Timelike;

    let old = old_planned
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc));

    match old {
        Some(old_dt) if old_has_time == 1 => {
            // 把原时刻搬到新日期。用 with_hour/with_minute 逐级设置并兜底，
            // 避免夏令时切换日出现不存在的本地时刻导致 panic。
            let moved = target
                .with_hour(old_dt.hour())
                .and_then(|d| d.with_minute(old_dt.minute()))
                .and_then(|d| d.with_second(old_dt.second()))
                .unwrap_or(target);
            (to_db_time(moved), true)
        }
        _ => (to_db_time(target), false),
    }
}

/// 拖拽改期：把任务的计划时间移动到新的日期（§4.3「拖拽改期前展示目标日期」）。
///
/// 语义要点：
/// - 只改 `planned_at`，**不动 `due_at`**。任务书 §4.1 明确三个时间字段含义不同，
///   拖日历改的是"安排在什么时候做"，不是"什么时候必须交"。
/// - 保留原有的**时刻**（见 `compute_rescheduled_at`）。
/// - 重复任务的实例可单独改期（§5「仅此次」得以成立）：因为 occurrence_key
///   不变，所以这不会在旧日期生成副本，也不会影响同系列的其他发生。
#[tauri::command]
pub async fn task_reschedule(
    state: State<'_, AppState>,
    id: String,
    new_date_utc: String,
) -> AppResult<Task> {
    let db = &state.db;
    let existing = get_task_row(db, &id).await?;
    if existing.deleted_at.is_some() {
        return Err(AppError::conflict("任务在回收站中，无法改期"));
    }

    let target = chrono::DateTime::parse_from_rfc3339(&new_date_utc)
        .map_err(|_| {
            AppError::validation(format!("目标日期格式不正确：{new_date_utc}"))
                .with_hint("请使用 ISO-8601 且带时区")
        })?
        .with_timezone(&chrono::Utc);

    let (new_at, has_time) =
        compute_rescheduled_at(existing.planned_at.as_deref(), existing.has_planned_time, target);

    let now = to_db_time(utc_now());
    // 改期同样要保证"任务时间与提醒时刻一起落库"（整改任务书 §5）：
    // 放进一个事务，任何一步失败都整体回滚，而不是留下半新半旧的状态。
    let mut tx = db.pool().begin().await?;
    sqlx::query(
        "UPDATE tasks SET planned_at = ?1, has_planned_time = ?2, updated_at = ?3 WHERE id = ?4",
    )
    .bind(&new_at)
    .bind(has_time as i64)
    .bind(&now)
    .bind(&id)
    .execute(&mut *tx)
    .await?;

    // 改了计划时间，相对型提醒必须跟着走（§4.3）
    crate::reminders::recompute_task_reminders_tx(&mut tx, &id).await?;

    tx.commit().await?;

    get_task_row(db, &id).await
}

/// 复制任务（§4.1 任务生命周期）。
///
/// 语义设计：
/// - 新标题加「（副本）」后缀；若已存在同名副本则递增为「（副本 2）」，
///   避免用户连续复制几次后分不清哪个是哪个。
/// - **状态重置为未完成**，且不复制完成时间——复制一个已完成任务，
///   用户要的是"再做一遍"，而不是复制一份"已完成的记录"。
/// - 保留计划与截止时间（复制通常是为了重复同类安排），
///   但清空提醒的触发记录，让提醒能对新任务重新生效。
/// - 复制标签与子任务（子任务进度重置为未完成）。
/// - **不复制附件**：附件的"复制模式"会占双份磁盘，且原件路径是共享的。
///   界面会提示"附件需要单独添加"，而不是静默丢掉。
#[tauri::command]
pub async fn task_duplicate(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<serde_json::Value> {
    duplicate_task_impl(&state.db, &id).await
}

/// 复制任务的实现（与 Tauri 解耦，便于集成测试直接调用）。
pub async fn duplicate_task_impl(db: &Db, id: &str) -> AppResult<serde_json::Value> {
    let src = get_task_row(db, id).await?;
    if src.deleted_at.is_some() {
        return Err(AppError::conflict("任务在回收站中，无法复制"));
    }

    // 生成不冲突的副本标题
    let base = format!("{}（副本）", src.title);
    let mut title = base.clone();
    let mut n = 2;
    loop {
        let exists: i64 = sqlx::query(
            "SELECT COUNT(*) AS n FROM tasks WHERE title = ?1 AND deleted_at IS NULL",
        )
        .bind(&title)
        .fetch_one(db.pool())
        .await?
        .try_get("n")?;
        if exists == 0 {
            break;
        }
        title = format!("{}（副本 {n}）", src.title);
        n += 1;
        if n > 999 {
            return Err(AppError::conflict("同名副本过多，请先整理已有任务"));
        }
    }

    let new_id = uuid::Uuid::now_v7().to_string();
    let now = to_db_time(utc_now());
    // 放在原任务之后：sort_order 取原值与下一个值的中点，
    // 这样不会打乱其它任务的相对顺序（手动排序用的就是 REAL）
    let next_order: f64 = sqlx::query(
        // CAST 见 `next_sort_order` 的注释：空结果集时 SQLite 会给出 INTEGER
        "SELECT CAST(COALESCE(MIN(sort_order), ?1 + 1) AS REAL) AS n
         FROM tasks WHERE sort_order > ?1",
    )
    .bind(src.sort_order)
    .fetch_one(db.pool())
    .await?
    .try_get("n")?;
    let sort_order = if next_order > src.sort_order {
        (src.sort_order + next_order) / 2.0
    } else {
        src.sort_order + 1.0
    };

    let mut tx = db.pool().begin().await?;

    sqlx::query(
        "INSERT INTO tasks (
            id, title, description, note_md, link_url,
            status, priority, project_id, category_id,
            planned_at, has_planned_time, due_at, has_due_time,
            estimated_minutes, actual_minutes,
            completed_at, created_at, updated_at,
            sort_order, is_pinned, is_favorite,
            series_id, occurrence_key, occurrence_index, occurrence_kind, is_exception,
            period_type
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5,
            'todo', ?6, ?7, ?8,
            ?9, ?10, ?11, ?12,
            ?13, 0,
            NULL, ?14, ?14,
            ?15, 0, ?16,
            NULL, NULL, NULL, 'single', 0,
            ?17
         )",
    )
    .bind(&new_id)
    .bind(&title)
    .bind(&src.description)
    .bind(&src.note_md)
    .bind(&src.link_url)
    .bind(src.priority)
    .bind(&src.project_id)
    .bind(&src.category_id)
    .bind(&src.planned_at)
    .bind(src.has_planned_time)
    .bind(&src.due_at)
    .bind(src.has_due_time)
    .bind(src.estimated_minutes)
    .bind(&now)
    .bind(sort_order)
    .bind(src.is_favorite)
    .bind(&src.period_type)
    .execute(&mut *tx)
    .await?;

    // 复制标签
    let tags = sqlx::query(
        "INSERT OR IGNORE INTO task_tags (task_id, tag_id) SELECT ?1, tag_id FROM task_tags WHERE task_id = ?2",
    )
    .bind(&new_id)
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected() as i64;

    // 复制子任务，但进度重置（副本是"再做一遍"）
    let subtasks = sqlx::query(
        "INSERT INTO subtasks (id, task_id, title, is_done, sort_order, completed_at, created_at, updated_at)
         SELECT lower(hex(randomblob(16))), ?1, title, 0, sort_order, NULL, ?2, ?2
         FROM subtasks WHERE task_id = ?3",
    )
    .bind(&new_id)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected() as i64;

    // 复制提醒，但清空 fired_at —— 否则新任务的提醒永远不会触发
    let reminders = sqlx::query(
        "INSERT INTO reminders (id, task_id, kind, offset_minutes, remind_at, is_enabled, fired_at, snoozed_until, created_at, updated_at)
         SELECT lower(hex(randomblob(16))), ?1, kind, offset_minutes, remind_at, is_enabled, NULL, NULL, ?2, ?2
         FROM reminders WHERE task_id = ?3",
    )
    .bind(&new_id)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected() as i64;

    // 附件数量（用于提示用户"附件未复制"）
    let attachments: i64 = sqlx::query("SELECT COUNT(*) AS n FROM attachments WHERE task_id = ?1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?
        .try_get("n")?;

    tx.commit().await?;

    log::info!(
        "已复制任务 {id} → {new_id}（标签 {tags}、子任务 {subtasks}、提醒 {reminders}）"
    );

    Ok(serde_json::json!({
        "newTaskId": new_id,
        "title": title,
        "copiedTags": tags,
        "copiedSubtasks": subtasks,
        "copiedReminders": reminders,
        // 附件刻意不复制，界面据此提示
        "skippedAttachments": attachments,
        "note": if attachments > 0 {
            format!("已复制任务，但 {attachments} 个附件未一起复制——附件的原件是共享的，请在新任务上按需重新添加。")
        } else {
            "已复制任务".to_string()
        },
    }))
}

/// 拖拽排序的输入（§4.1「拖拽排序和状态变更」）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderInput {
    /// 被移动的任务
    pub moved_id: String,
    /// 目标位置：移动到该任务之前；为 None 表示移到末尾
    #[serde(default)]
    pub before_id: Option<String>,
}

/// 手动排序：把任务移动到指定位置。
///
/// 实现用**中点插入法**：新 sort_order 取前后两个任务的中点值，
/// 只更新被移动的那一行，而不是重排整表。这样：
/// - 单次操作是 O(1) 次写入，上千条任务也不卡；
/// - 其它任务的相对顺序完全不受影响。
///
/// 中点法会让相邻差不断减半，约 50 次同位置插入后 REAL 精度可能不足。
/// 因此在差值过小时**自动重新编号**（间隔 1000），这只在极端情况下发生。
#[tauri::command]
pub async fn task_reorder(
    state: State<'_, AppState>,
    input: ReorderInput,
) -> AppResult<f64> {
    reorder_task_impl(&state.db, &input).await
}

/// 排序实现（与 Tauri 解耦，便于集成测试直接调用）。
pub async fn reorder_task_impl(db: &Db, input: &ReorderInput) -> AppResult<f64> {
    let moved = get_task_row(db, &input.moved_id).await?;
    if moved.deleted_at.is_some() {
        return Err(AppError::conflict("任务在回收站中，无法排序"));
    }

    // 只在"手动排序"语境下有意义：其它排序规则会覆盖 sort_order 的效果，
    // 但值仍会被保存，切回手动排序时生效。这里不阻止，只记日志。
    let now = to_db_time(utc_now());

    // 取邻居 → 算中点。若前后差值已被"对半砍"到精度不足，就重新编号，
    // 然后**必须重新取一次邻居**再算：用旧的 p/n 会算出一个落在错误位置的
    // 值（实测表现为"任务被插到了列表最前面"）。
    let mut renumbered = false;
    let new_order = loop {
        let (prev_order, next_order) =
            reorder_neighbors(db, &input.moved_id, input.before_id.as_deref()).await?;
        match (prev_order, next_order) {
            (Some(p), Some(n)) if (n - p).abs() < 1e-6 => {
                if renumbered {
                    // 重新编号后依然分不开，说明数据已被外部改坏，明确报错而不是乱插
                    return Err(AppError::conflict(
                        "排序值精度不足，无法在两者之间插入",
                    )
                    .with_hint("请在设置中执行一次「数据维护」，或把该任务拖到列表末尾"));
                }
                renumbered = true;
                renumber_sort_orders(db).await?;
            }
            (Some(p), Some(n)) => break p + (n - p) / 2.0,
            (Some(p), None) => break p + 1000.0,
            (None, Some(n)) => break n - 1000.0,
            // 列表里只有它自己
            (None, None) => break 0.0,
        }
    };

    sqlx::query("UPDATE tasks SET sort_order = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(new_order)
        .bind(&now)
        .bind(&input.moved_id)
        .execute(db.pool())
        .await?;

    Ok(new_order)
}

/// 取"插入到 before 之前"所需的左右邻居排序值。
///
/// 注意排除被移动任务自身，否则它会把自己当成邻居，算出错误的中点。
/// `before` 为 None（或等于自己）表示移到末尾。
async fn reorder_neighbors(
    db: &Db,
    moved_id: &str,
    before_id: Option<&str>,
) -> AppResult<(Option<f64>, Option<f64>)> {
    match before_id {
        Some(before) if before != moved_id => {
            let target =
                sqlx::query("SELECT sort_order FROM tasks WHERE id = ?1 AND deleted_at IS NULL")
                    .bind(before)
                    .fetch_optional(db.pool())
                    .await?
                    .ok_or_else(|| AppError::not_found("目标任务", before))?;
            let t: f64 = target.try_get("sort_order")?;

            let prev: Option<(f64,)> = sqlx::query_as(
                "SELECT sort_order FROM tasks
                 WHERE deleted_at IS NULL AND id <> ?1 AND sort_order < ?2
                 ORDER BY sort_order DESC LIMIT 1",
            )
            .bind(moved_id)
            .bind(t)
            .fetch_optional(db.pool())
            .await?;

            Ok((prev.map(|(v,)| v), Some(t)))
        }
        _ => {
            // 移到末尾
            let last: Option<(f64,)> = sqlx::query_as(
                "SELECT sort_order FROM tasks
                 WHERE deleted_at IS NULL AND id <> ?1
                 ORDER BY sort_order DESC LIMIT 1",
            )
            .bind(moved_id)
            .fetch_optional(db.pool())
            .await?;
            Ok((last.map(|(v,)| v), None))
        }
    }
}

/// 把全部任务的 sort_order 按当前顺序重新编号为 1000 的整数倍。
///
/// 用途有两个：中点插入导致精度不足时修复；以及为将来的"批量排序"提供基础。
/// 只更新未删除任务，已删除的保持原值（回收站恢复后顺序不乱）。
async fn renumber_sort_orders(db: &Db) -> AppResult<usize> {
    let ids: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM tasks WHERE deleted_at IS NULL
         ORDER BY is_pinned DESC, sort_order ASC, created_at DESC",
    )
    .fetch_all(db.pool())
    .await?;

    let now = to_db_time(utc_now());
    let mut tx = db.pool().begin().await?;
    let mut n = 0usize;
    for (i, (id,)) in ids.iter().enumerate() {
        sqlx::query("UPDATE tasks SET sort_order = ?1, updated_at = ?2 WHERE id = ?3")
            .bind((i as f64 + 1.0) * 1000.0)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        n += 1;
    }
    tx.commit().await?;
    log::info!("已重新编号 {n} 个任务的排序值（修复中点插入的精度不足）");
    Ok(n)
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

    // =========================================================================
    // 周期跨度（§4.1：这周/这个月做完就行）
    // =========================================================================

    #[test]
    fn period_accepts_all_six_values() {
        for (input, expected) in [
            ("none", "none"),
            ("day", "day"),
            ("week", "week"),
            ("month", "month"),
            ("quarter", "quarter"),
            ("year", "year"),
        ] {
            assert_eq!(validate_period(input).unwrap(), expected, "输入 {input}");
        }
    }

    /// 空字符串视为"不限"：前端下拉框在某些状态可能提交空值，
    /// 把它当成非法值会让用户困惑于"我明明没设周期却报错"。
    #[test]
    fn empty_period_means_none() {
        assert_eq!(validate_period("").unwrap(), "none");
        assert_eq!(validate_period("  ").unwrap(), "none");
    }

    #[test]
    fn period_trims_whitespace() {
        assert_eq!(validate_period(" week ").unwrap(), "week");
        assert_eq!(validate_period("\tmonth\n").unwrap(), "month");
    }

    /// 非法值必须报错且列出允许值——静默当"不限"会让用户的设置悄悄失效
    #[test]
    fn invalid_period_is_rejected_with_allowed_values() {
        for bad in ["weekly", "WEEK", "月", "1", "forever", "week "] {
            if bad.trim() == "week" {
                continue; // 这是合法的
            }
            let e = validate_period(bad).unwrap_err();
            assert!(e.message.contains(bad.trim()), "错误应包含非法值：{}", e.message);
            let hint = e.hint.unwrap_or_default();
            assert!(hint.contains("week"), "提示应列出允许值：{hint}");
            assert!(hint.contains("year"), "提示应包含 year：{hint}");
        }
    }

    /// 大小写敏感是刻意的：数据库 CHECK 约束用的是小写，
    /// 若这里宽松接受 'WEEK' 会写库失败并抛出难以理解的数据库错误。
    #[test]
    fn period_is_case_sensitive_by_design() {
        assert!(validate_period("WEEK").is_err());
        assert!(validate_period("Week").is_err());
        assert!(validate_period("week").is_ok());
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

    /// 固定宽度是字典序可比较的前提。
    ///
    /// 注意时区换算：北京 2026-09-23 01:00 (+08:00) 实际等于
    /// **UTC 2026-09-22 17:00**，比 UTC 2026-09-22 20:00 更早。
    /// 这个测试刻意包含一次跨日换算，防止把"本地时刻的大小"
    /// 误当成"UTC 时刻的大小"。
    #[test]
    fn time_comparison_is_lexicographically_safe() {
        let earlier = validate_time("x", "2026-09-23T01:00:00+08:00").unwrap();
        let later = validate_time("x", "2026-09-22T20:00:00Z").unwrap();

        // 所有产出必须等宽，否则字符串比较会失效
        assert_eq!(earlier.len(), 24, "时间字符串必须固定 24 字符宽");
        assert_eq!(later.len(), 24, "时间字符串必须固定 24 字符宽");
        assert!(earlier.ends_with('Z'), "必须归一为 UTC 并以 Z 结尾");

        assert_eq!(earlier, "2026-09-22T17:00:00.000Z");
        assert_eq!(later, "2026-09-22T20:00:00.000Z");
        // 字典序必须与时间序一致
        assert!(earlier < later, "UTC 17:00 应早于 UTC 20:00");
    }

    /// 相反方向也要成立，避免"单向巧合"掩盖问题。
    #[test]
    fn lexicographic_order_holds_across_days() {
        let a = validate_time("x", "2026-09-22T23:59:59Z").unwrap();
        let b = validate_time("x", "2026-09-23T00:00:01Z").unwrap();
        assert!(a < b, "跨日边界必须保持字典序");
    }

    #[test]
    fn empty_string_time_is_treated_as_unset_not_as_error() {
        // 前端表单清空日期后常提交 ""，必须视为"未设置"
        assert_eq!(validate_opt_time("计划时间", None).unwrap(), None);
        let s = String::new();
        assert_eq!(validate_opt_time("计划时间", Some(&s)).unwrap(), None);
    }

    // =========================================================================
    // 拖拽改期（§4.3）
    // =========================================================================

    fn utc(s: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(s)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    /// 精确时间的任务改期后必须**保留原时刻**。
    /// 这是最容易写错的一点：若丢失时刻，用户会莫名发现任务变成 00:00。
    #[test]
    fn reschedule_preserves_time_of_day() {
        // 注意时区：北京 10-05 00:00 == UTC 10-04 16:00，
        // 所以"保留 01:00 时刻"后落在 UTC 的 10-04 01:00
        let target = utc("2026-10-05T00:00:00+08:00");
        let (got, has_time) =
            compute_rescheduled_at(Some("2026-09-25T01:00:00.000Z"), 1, target);
        assert!(has_time, "原任务含具体时刻，改期后仍应含时刻");
        assert_eq!(got, "2026-10-04T01:00:00.000Z");
    }

    /// 仅日期的任务改期后仍是"仅日期"，不能被升级成有时刻。
    #[test]
    fn reschedule_keeps_date_only_unchanged() {
        let target = utc("2026-10-05T00:00:00+08:00");
        let (got, has_time) =
            compute_rescheduled_at(Some("2026-09-25T00:00:00.000Z"), 0, target);
        assert!(!has_time, "原任务是仅日期，改期后仍应是仅日期");
        assert_eq!(got, "2026-10-04T16:00:00.000Z", "应为目标日期的本地零点所对应的 UTC");
    }

    /// 原本没有计划时间的任务，拖到某天后仍标记为"仅日期"。
    #[test]
    fn reschedule_without_previous_plan_marks_date_only() {
        let target = utc("2026-10-05T00:00:00+08:00");
        let (_, has_time) = compute_rescheduled_at(None, 0, target);
        assert!(!has_time, "用户只拖了一下，不应因此把任务变成有时刻");
    }

    /// 输出必须是固定宽度 UTC，否则会破坏全库的字典序比较。
    #[test]
    fn reschedule_output_is_fixed_width() {
        let target = utc("2026-10-05T00:00:00+08:00");
        for (old, ht) in [
            (Some("2026-09-25T01:00:00.000Z"), 1),
            (Some("2026-09-25T00:00:00.000Z"), 0),
            (None, 0),
        ] {
            let (got, _) = compute_rescheduled_at(old, ht, target);
            assert_eq!(got.len(), 24, "时间字符串必须固定 24 字符：{got}");
            assert!(got.ends_with('Z'), "必须以 Z 结尾：{got}");
        }
    }

    /// 时间字符串非法时应安全回退为"仅日期"，而不是 panic 或写坏数据。
    #[test]
    fn reschedule_handles_malformed_previous_value() {
        let target = utc("2026-10-05T00:00:00+08:00");
        let (got, has_time) = compute_rescheduled_at(Some("不是时间"), 1, target);
        assert!(!has_time, "无法解析时应回退为仅日期");
        assert_eq!(got.len(), 24);
    }

    /// 跨日边界：原时刻为 23:30 UTC，改期后仍是 23:30 UTC。
    #[test]
    fn reschedule_preserves_late_evening_time() {
        let target = utc("2026-12-31T00:00:00Z");
        let (got, _) = compute_rescheduled_at(Some("2026-09-25T23:30:00.000Z"), 1, target);
        assert_eq!(got, "2026-12-31T23:30:00.000Z");
    }
}
