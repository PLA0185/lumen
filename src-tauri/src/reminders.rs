//! 提醒调度（任务书 §4.3）。
//!
//! ## 为什么自己实现调度而不是用插件的 schedule
//!
//! 官方 `tauri-plugin-notification` 的 schedule 能力在桌面端**不可靠**：
//! 其源码明确写着 `scheduling, grouping and action related options are ignored`
//! （已核实，见 docs/api-research-tauri2.md）。若依赖它，程序重启后提醒
//! 会直接丢失——这正是任务书 §4.3 要防的"漏发"。
//!
//! ## 本实现的语义
//!
//! 以数据库为唯一事实来源，内存里只跑一个轮询循环：
//!
//! - 每 `TICK` 查一次「已启用 + 未触发 + remind_at <= now」的提醒并发出；
//! - 发出后写 `fired_at`，因此**重复触发不可能发生**（含重启、休眠唤醒）；
//! - 程序关闭期间到期的提醒**不会被丢弃**：下次启动时它们仍然满足查询条件，
//!   会按「补发」策略处理；
//! - 补发窗口 `missed_grace` 限制补发范围，避免开机时被几十条历史提醒轰炸。
//!   超出窗口的提醒被标记为已过期（`expired_at`），在界面上可见而不是静默消失。
//!
//! ## 时间基准
//!
//! 全部比较基于 UTC 字符串（固定宽度，字典序即时间序）。系统时间被向前
//! 调整时，`remind_at <= now` 仍成立，提醒会照常发出；向后调整时提醒会
//! 延后，但不会丢失。这是刻意选择的"宁可晚到不可丢失"策略。

use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::AppState;
use crate::db::{to_db_time, utc_now};
use crate::error::{AppError, AppResult};

/// 轮询间隔。30 秒足够精确到"分钟级"提醒，且开销可忽略
/// （每次只是一个走索引的查询）。
const TICK: Duration = Duration::from_secs(30);

/// 单次 tick 最多发出的提醒数，防止异常数据导致一次性弹满屏幕
const MAX_PER_TICK: i64 = 20;

/// 提醒被停用的原因（对应 `reminders.disabled_reason`，migration 0005）。
///
/// 为什么要区分：如果只有一个 `is_enabled`，就没法回答"这条提醒是用户自己关的，
/// 还是系统因为缺少依赖时间临时停用的"。前者任何自动逻辑都不许重新打开，
/// 后者在时间恢复后应当自动恢复——混在一起就会出现
/// "用户关掉的提醒被系统偷偷打开"或"时间补回来了提醒却永久不再触发"。
pub const DISABLED_BY_USER: &str = "user";
/// 系统原因：相对提醒依赖的 planned_at / due_at 为空
pub const DISABLED_MISSING_BASE_TIME: &str = "missing_base_time";

/// 提醒类型（与数据库 CHECK 约束一致）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReminderKind {
    /// 到期时提醒
    AtDue,
    /// 到期前提醒
    BeforeDue,
    /// 计划时间到点提醒
    AtPlanned,
    /// 计划时间前提醒
    BeforePlanned,
    /// 自定义绝对时间
    Custom,
}

impl ReminderKind {
    fn as_db(self) -> &'static str {
        match self {
            Self::AtDue => "at_due",
            Self::BeforeDue => "before_due",
            Self::AtPlanned => "at_planned",
            Self::BeforePlanned => "before_planned",
            Self::Custom => "custom",
        }
    }

    fn from_db(s: &str) -> Self {
        match s {
            "at_due" => Self::AtDue,
            "before_due" => Self::BeforeDue,
            "at_planned" => Self::AtPlanned,
            "before_planned" => Self::BeforePlanned,
            _ => Self::Custom,
        }
    }
}

/// 提醒视图（前端直接使用）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub id: String,
    pub task_id: String,
    pub kind: String,
    pub offset_minutes: Option<i64>,
    pub remind_at: String,
    pub is_enabled: i64,
    pub fired_at: Option<String>,
    pub snoozed_until: Option<String>,
    /// 停用原因：NULL=启用中 / `user`=用户主动关闭 /
    /// `missing_base_time`=系统因缺少依赖时间临时停用（migration 0005）
    pub disabled_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 待触发的提醒（带任务标题，通知正文需要）
#[derive(Debug, Clone)]
pub(crate) struct DueReminder {
    id: String,
    task_id: String,
    title: String,
    remind_at: String,
    /// 任务是否已完成——已完成的提醒不应再打扰用户
    task_done: bool,
}

/// 创建提醒的输入
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateReminderInput {
    pub task_id: String,
    /// 提醒类型
    pub kind: ReminderKind,
    /// 提前多少分钟（kind 为 before_due / before_planned 时必填）
    #[serde(default)]
    pub offset_minutes: Option<i64>,
    /// 自定义绝对时间（kind 为 custom 时必填），RFC-3339
    #[serde(default)]
    pub remind_at: Option<String>,
}

/// 提前提醒的允许范围：1 分钟 ~ 30 天
const OFFSET_MIN: i64 = 1;
const OFFSET_MAX: i64 = 60 * 24 * 30;

/// 补发窗口的允许范围：0 分钟（不补发）~ 24 小时
const GRACE_MIN: i64 = 0;
const GRACE_MAX: i64 = 60 * 24;

/// 调度器运行状态，供 `app_info` 与设置页展示
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerStatus {
    /// 是否正在运行
    pub running: bool,
    /// 是否被用户暂停
    pub paused: bool,
    /// 补发窗口（分钟）
    pub missed_grace_minutes: i64,
    /// 已启用且未触发的提醒数量
    pub pending_count: i64,
    /// 因超出补发窗口而被标记过期的数量
    pub expired_count: i64,
}

/// 全局暂停标志由 `AppState` 持有（托盘菜单也会改它）；
/// 调度循环每 tick 检查一次，因此暂停是秒级生效的。
/// 计算某条提醒的绝对触发时刻。
///
/// 返回 `Ok(None)` 表示"该提醒当前无法确定时刻"（例如所依赖的时间字段为空），
/// 这种情况**不报错**：用户可能先设了"到期前提醒"，之后才填截止时间。
fn compute_remind_at(
    kind: ReminderKind,
    offset_minutes: Option<i64>,
    custom_at: Option<&str>,
    planned_at: Option<&str>,
    due_at: Option<&str>,
) -> AppResult<Option<String>> {
    let parse = |s: &str| -> AppResult<chrono::DateTime<chrono::Utc>> {
        chrono::DateTime::parse_from_rfc3339(s)
            .map(|d| d.with_timezone(&chrono::Utc))
            .map_err(|_| {
                AppError::validation(format!("时间格式不正确：{s}"))
                    .with_hint("请使用 ISO-8601 且带时区")
            })
    };

    let base = |t: Option<&str>| -> AppResult<Option<chrono::DateTime<chrono::Utc>>> {
        match t {
            Some(s) if !s.trim().is_empty() => Ok(Some(parse(s)?)),
            _ => Ok(None),
        }
    };

    let offset = |m: Option<i64>| -> AppResult<chrono::Duration> {
        let m =
            m.ok_or_else(|| AppError::validation("该提醒类型必须指定提前分钟数 offsetMinutes"))?;
        if !(OFFSET_MIN..=OFFSET_MAX).contains(&m) {
            return Err(AppError::validation(format!("提前分钟数超出范围：{m}"))
                .with_hint(format!("允许 {OFFSET_MIN}–{OFFSET_MAX} 分钟")));
        }
        Ok(chrono::Duration::minutes(m))
    };

    // 计算偏移量。注意不能写成 `.map(|d| d - offset(...)?)`——
    // `?` 只能用在返回 Result/Option 的闭包里，map 的闭包不是。
    // 因此先求出时长，再在外部做减法。
    let off = match kind {
        ReminderKind::BeforeDue | ReminderKind::BeforePlanned => Some(offset(offset_minutes)?),
        _ => None,
    };

    let result = match kind {
        ReminderKind::AtDue => base(due_at)?,
        ReminderKind::AtPlanned => base(planned_at)?,
        ReminderKind::BeforeDue => base(due_at)?.map(|d| d - off.unwrap_or_default()),
        ReminderKind::BeforePlanned => base(planned_at)?.map(|d| d - off.unwrap_or_default()),
        ReminderKind::Custom => match custom_at {
            Some(s) if !s.trim().is_empty() => Some(parse(s)?),
            _ => return Err(AppError::validation("自定义提醒必须提供 remindAt 时间")),
        },
    };

    Ok(result.map(to_db_time))
}

/// 创建提醒
#[tauri::command]
pub async fn reminder_create(
    app: AppHandle,
    state: State<'_, AppState>,
    input: CreateReminderInput,
) -> AppResult<Reminder> {
    let db = &state.db;

    // 读取任务的时间字段，用于推导绝对触发时刻
    let row = sqlx::query("SELECT planned_at, due_at, status, deleted_at FROM tasks WHERE id = ?1")
        .bind(&input.task_id)
        .fetch_optional(db.pool())
        .await?;

    let Some(row) = row else {
        return Err(AppError::not_found("任务", &input.task_id));
    };
    let deleted: Option<String> = row.try_get("deleted_at")?;
    if deleted.is_some() {
        return Err(AppError::conflict("任务在回收站中，无法添加提醒"));
    }

    let planned: Option<String> = row.try_get("planned_at")?;
    let due: Option<String> = row.try_get("due_at")?;

    let remind_at = compute_remind_at(
        input.kind,
        input.offset_minutes,
        input.remind_at.as_deref(),
        planned.as_deref(),
        due.as_deref(),
    )?;

    // 依赖的时间字段为空：明确告知用户缺什么，而不是静默不创建
    let Some(remind_at) = remind_at else {
        let missing = match input.kind {
            ReminderKind::AtDue | ReminderKind::BeforeDue => "截止时间",
            ReminderKind::AtPlanned | ReminderKind::BeforePlanned => "计划时间",
            ReminderKind::Custom => "提醒时间",
        };
        return Err(
            AppError::validation(format!("该任务尚未设置{missing}，无法创建此提醒"))
                .with_hint(format!("请先为任务填写{missing}，或改用「自定义时间」提醒")),
        );
    };

    let id = uuid::Uuid::now_v7().to_string();
    let now = to_db_time(utc_now());

    sqlx::query(
        "INSERT INTO reminders (id, task_id, kind, offset_minutes, remind_at, is_enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)",
    )
    .bind(&id)
    .bind(&input.task_id)
    .bind(input.kind.as_db())
    .bind(input.offset_minutes)
    .bind(&remind_at)
    .bind(&now)
    .execute(db.pool())
    .await?;

    // 通知前端刷新提醒列表；调度循环每个 tick 都会重新查库，
    // 因此不需要额外的"唤醒"机制。
    let _ = app.emit("reminders-changed", ());

    get_reminder(&state, &id).await
}

async fn get_reminder(state: &AppState, id: &str) -> AppResult<Reminder> {
    let r = sqlx::query_as::<_, Reminder>("SELECT * FROM reminders WHERE id = ?1")
        .bind(id)
        .fetch_optional(state.db.pool())
        .await?;
    r.ok_or_else(|| AppError::not_found("提醒", id))
}

/// 列出某任务的提醒
#[tauri::command]
pub async fn reminder_list(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<Vec<Reminder>> {
    let rows = sqlx::query_as::<_, Reminder>(
        "SELECT * FROM reminders WHERE task_id = ?1 ORDER BY remind_at ASC",
    )
    .bind(&task_id)
    .fetch_all(state.db.pool())
    .await?;
    Ok(rows)
}

/// 启用 / 停用提醒
#[tauri::command]
pub async fn reminder_set_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> AppResult<Reminder> {
    get_reminder(&state, &id).await?;
    let now = to_db_time(utc_now());
    // 用户的操作必须把"停用原因"一并写清楚（整改任务书 §6.2）：
    // - 用户打开 → 清空原因（恢复正常启用）；
    // - 用户关闭 → 记成 `user`，这样将来任务时间变了，
    //   重算逻辑也不会"好心"把它自动打开。
    // 如果这里不写原因，重算逻辑就无法区分"用户关的"和"系统关的"。
    let reason: Option<&str> = if enabled {
        None
    } else {
        Some(DISABLED_BY_USER)
    };
    sqlx::query(
        "UPDATE reminders SET is_enabled = ?1, disabled_reason = ?2, updated_at = ?3 WHERE id = ?4",
    )
    .bind(enabled as i64)
    .bind(reason)
    .bind(&now)
    .bind(&id)
    .execute(state.db.pool())
    .await?;
    get_reminder(&state, &id).await
}

/// 删除提醒
#[tauri::command]
pub async fn reminder_delete(state: State<'_, AppState>, id: String) -> AppResult<i64> {
    let n = sqlx::query("DELETE FROM reminders WHERE id = ?1")
        .bind(&id)
        .execute(state.db.pool())
        .await?
        .rows_affected() as i64;
    if n == 0 {
        return Err(AppError::not_found("提醒", &id));
    }
    Ok(n)
}

/// 稍后提醒：把触发时刻推迟指定分钟，并清除已触发标记
#[tauri::command]
pub async fn reminder_snooze(
    state: State<'_, AppState>,
    id: String,
    minutes: i64,
) -> AppResult<Reminder> {
    if !(1..=OFFSET_MAX).contains(&minutes) {
        return Err(
            AppError::validation(format!("稍后提醒的分钟数超出范围：{minutes}"))
                .with_hint(format!("允许 1–{OFFSET_MAX} 分钟")),
        );
    }
    let r = get_reminder(&state, &id).await?;
    let base = chrono::DateTime::parse_from_rfc3339(&r.remind_at)
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| utc_now());
    let now = utc_now();
    // 若原时刻已过，从"现在"起算更符合直觉
    let from = if base < now { now } else { base };
    let next = to_db_time(from + chrono::Duration::minutes(minutes));

    let ts = to_db_time(now);
    sqlx::query(
        "UPDATE reminders SET remind_at = ?1, snoozed_until = ?1, fired_at = NULL, updated_at = ?2
         WHERE id = ?3",
    )
    .bind(&next)
    .bind(&ts)
    .bind(&id)
    .execute(state.db.pool())
    .await?;

    get_reminder(&state, &id).await
}

/// 读取调度器状态（设置页展示用）
#[tauri::command]
pub async fn reminder_scheduler_status(state: State<'_, AppState>) -> AppResult<SchedulerStatus> {
    let db = &state.db;
    let pending: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM reminders WHERE is_enabled = 1 AND fired_at IS NULL",
    )
    .fetch_one(db.pool())
    .await?
    .try_get("n")?;

    let expired: i64 =
        sqlx::query("SELECT COUNT(*) AS n FROM reminders WHERE fired_at = 'expired'")
            .fetch_one(db.pool())
            .await?
            .try_get("n")?;

    Ok(SchedulerStatus {
        running: !state.scheduler_stopped.load(Ordering::Relaxed),
        paused: state.reminders_paused.load(Ordering::Relaxed),
        missed_grace_minutes: state.missed_grace_minutes.load(Ordering::Relaxed),
        pending_count: pending,
        expired_count: expired,
    })
}

/// 设置补发窗口（分钟）。0 表示不补发程序关闭期间到期的提醒。
#[tauri::command]
pub async fn reminder_set_grace(state: State<'_, AppState>, minutes: i64) -> AppResult<i64> {
    if !(GRACE_MIN..=GRACE_MAX).contains(&minutes) {
        return Err(
            AppError::validation(format!("补发窗口超出范围：{minutes} 分钟")).with_hint(format!(
                "允许 {GRACE_MIN}–{GRACE_MAX} 分钟（0 表示不补发关闭期间错过的提醒）"
            )),
        );
    }
    state.missed_grace_minutes.store(minutes, Ordering::Relaxed);
    Ok(minutes)
}

// =============================================================================
// 调度循环
// =============================================================================

/// 启动调度循环。由 `lib.rs` 在 setup 中调用一次。
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // 启动时先做一次"补发/过期"整理，避免开机后延迟一个 tick 才处理
        if let Err(e) = settle_missed(&app).await {
            log::warn!("提醒补发整理失败：{e}");
        }

        loop {
            tokio::time::sleep(TICK).await;

            // 用户暂停提醒时跳过（§8.6 托盘「暂停提醒」）
            let paused = app
                .try_state::<AppState>()
                .map(|s| s.reminders_paused.load(Ordering::Relaxed))
                .unwrap_or(false);
            if paused {
                continue;
            }

            // 调度器自身被要求停止（退出流程）
            let stopped = app
                .try_state::<AppState>()
                .map(|s| s.scheduler_stopped.load(Ordering::Relaxed))
                .unwrap_or(true);
            if stopped {
                log::info!("提醒调度器已停止");
                break;
            }

            if let Err(e) = tick(&app).await {
                // 单次失败不能终止循环，否则一次数据库抖动就永久失聪
                log::error!("提醒调度 tick 失败（将继续重试）：{e}");
            }
        }
    });
}

/// 处理"程序关闭期间到期"的提醒。
///
/// 在窗口期内的：保留，等待 tick 发出（补发）。
/// 超出窗口的：标记 `fired_at = 'expired'`，在设置页可见——
/// **绝不静默丢弃**，否则用户永远不知道提醒曾经存在过。
async fn settle_missed(app: &AppHandle) -> AppResult<()> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| AppError::internal("应用状态未就绪"))?;
    let grace = state.missed_grace_minutes.load(Ordering::Relaxed);
    let now = utc_now();
    let cutoff = to_db_time(now - chrono::Duration::minutes(grace));
    let now_s = to_db_time(now);

    let n = sqlx::query(
        "UPDATE reminders SET fired_at = 'expired', updated_at = ?1
         WHERE is_enabled = 1 AND fired_at IS NULL AND remind_at < ?2",
    )
    .bind(&now_s)
    .bind(&cutoff)
    .execute(state.db.pool())
    .await?
    .rows_affected();

    if n > 0 {
        log::info!("已将 {n} 条超出补发窗口的提醒标记为过期（窗口 {grace} 分钟）");
        let _ = app.emit("reminders-expired", n);
    }
    Ok(())
}

/// 一次调度：查出到期的提醒并发出通知。
async fn tick(app: &AppHandle) -> AppResult<()> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| AppError::internal("应用状态未就绪"))?;
    let db = &state.db;
    let now = to_db_time(utc_now());
    let due = due_reminders(db, &now, MAX_PER_TICK).await?;

    for d in due {
        if d.task_done {
            continue;
        }
        fire(app, &d).await;
    }
    Ok(())
}

/// 查出"这一刻应当发出"的提醒（供调度循环与测试共用）。
///
/// 抽出来的原因：这段过滤条件就是 §15 里"已完成任务不再提醒、回收站任务
/// 不再提醒、已触发的绝不重复"的**唯一实现**，必须能被测试直接验证，
/// 而不是只能靠"跑一遍看看有没有弹窗"。
pub(crate) async fn due_reminders(
    db: &crate::db::Db,
    now: &str,
    limit: i64,
) -> AppResult<Vec<DueReminder>> {
    // 只看未触发、已启用、且已到点的提醒。
    // 已完成任务不再打扰（用户已经做完了，提醒没有意义），
    // 回收站里的任务同理（deleted_at IS NOT NULL 不算数）。
    let rows = sqlx::query(
        "SELECT r.id, r.task_id, r.remind_at, t.title, t.status
         FROM reminders r
         JOIN tasks t ON t.id = r.task_id
         WHERE r.is_enabled = 1
           AND r.fired_at IS NULL
           AND r.remind_at <= ?1
           AND t.deleted_at IS NULL
           AND t.status NOT IN ('done', 'archived')
         ORDER BY r.remind_at ASC
         LIMIT ?2",
    )
    .bind(now)
    .bind(limit.clamp(1, MAX_PER_TICK))
    .fetch_all(db.pool())
    .await?;

    let mut due = Vec::with_capacity(rows.len());
    for r in rows {
        let status: String = r.try_get("status")?;
        due.push(DueReminder {
            id: r.try_get("id")?,
            task_id: r.try_get("task_id")?,
            title: r.try_get("title")?,
            remind_at: r.try_get("remind_at")?,
            task_done: status == "done" || status == "archived",
        });
    }
    Ok(due)
}

/// 发出单条提醒：先写 `fired_at`（去重的关键），再发通知。
///
/// 顺序很重要：**先落库再通知**。若先通知后落库，程序在两者之间崩溃
/// 会导致下次启动重复通知；反过来最坏情况是漏掉一次通知，
/// 而"宁可漏发一次也不重复轰炸"正是 §4.3 的要求。
async fn fire(app: &AppHandle, d: &DueReminder) {
    let state = match app.try_state::<AppState>() {
        Some(s) => s,
        None => return,
    };
    let now = to_db_time(utc_now());

    // 条件里再带一次 fired_at IS NULL，防止并发 tick 重复触发
    let claimed = sqlx::query(
        "UPDATE reminders SET fired_at = ?1, updated_at = ?1 WHERE id = ?2 AND fired_at IS NULL",
    )
    .bind(&now)
    .bind(&d.id)
    .execute(state.db.pool())
    .await
    .map(|r| r.rows_affected())
    .unwrap_or(0);

    if claimed == 0 {
        // 已被其他 tick 处理，直接返回，避免重复通知
        return;
    }

    // 记录发出日志：既是对"提醒确实发出过"的可诊断证据（§10），
    // 也便于用户排查"为什么没收到通知"。
    // 只记任务标题与计划时刻，不记任何私密正文（§10 日志要求）。
    log::info!(
        "发出提醒 {}：任务「{}」，计划时刻 {}",
        d.id,
        d.title,
        d.remind_at
    );

    if let Err(e) = send_notification(app, &d.title, &d.remind_at).await {
        // 通知失败不回滚 fired_at：否则一次通知失败会导致每次 tick 重试，
        // 变成更严重的重复轰炸。改为记录日志并在界面提示。
        log::error!("发送提醒通知失败（提醒 {}）：{e}", d.id);
        let _ = app.emit("notification-failed", d.id.clone());
    } else {
        log::info!("通知已提交给 Windows 通知中心（提醒 {}）", d.id);
    }

    let _ = app.emit(
        "reminder-fired",
        serde_json::json!({
            "id": d.id,
            "taskId": d.task_id,
            "title": d.title,
        }),
    );
}

/// 通过系统通知中心发送提醒。
///
/// 已知限制（已核实，写入文档与验收表）：桌面端**不支持通知点击回调**，
/// 因此无法做到"点击通知直接跳到该任务"。替代方案是通知正文中带上任务
/// 标题，并让主界面在获得焦点时高亮最近触发提醒的任务。
async fn send_notification(app: &AppHandle, title: &str, remind_at: &str) -> AppResult<()> {
    use tauri_plugin_notification::NotificationExt;

    let (body_time, _) = remind_at.split_at(remind_at.len().min(16));
    let notification = app
        .notification()
        .builder()
        .title("Lumen 提醒")
        .body(format!("{title}\n计划时间：{body_time}"));

    notification.show().map_err(|e| {
        AppError::new(
            crate::error::ErrorCode::Internal,
            format!("系统通知发送失败：{e}"),
        )
        .with_hint("请检查 Windows 通知设置是否允许 Lumen 发送通知")
    })?;
    Ok(())
}

/// 手动触发一次补发整理（设置页的"立即检查错过的提醒"按钮）
#[tauri::command]
pub async fn reminder_check_missed(state: State<'_, AppState>) -> AppResult<i64> {
    let grace = state.missed_grace_minutes.load(Ordering::Relaxed);
    let now = utc_now();
    let cutoff = to_db_time(now - chrono::Duration::minutes(grace));
    let now_s = to_db_time(now);
    let n = sqlx::query(
        "UPDATE reminders SET fired_at = 'expired', updated_at = ?1
         WHERE is_enabled = 1 AND fired_at IS NULL AND remind_at < ?2",
    )
    .bind(&now_s)
    .bind(&cutoff)
    .execute(state.db.pool())
    .await?
    .rows_affected() as i64;
    Ok(n)
}

/// 任务时间变更后重算其"相对型"提醒的绝对时刻（自带事务）。
///
/// 这是 §4.3「重算待提醒项」的核心：用户改了截止时间，
/// 所有"到期前 30 分钟"的提醒都必须跟着走，否则会在错误时间响起。
///
/// **任务时间与提醒时刻必须原子更新**（整改任务书 §5）：本函数会自己开一个
/// 事务；而 `task_update` 这类"改任务的同时要重算提醒"的场景，必须改用
/// [`recompute_task_reminders_tx`]，让两步落在同一个事务里，
/// 否则一旦重算失败就会出现"任务时间是新值、提醒时刻还是旧值"的静默不一致。
pub async fn recompute_task_reminders(db: &crate::db::Db, task_id: &str) -> AppResult<i64> {
    let mut tx = db.pool().begin().await?;
    let updated = recompute_task_reminders_tx(&mut tx, task_id).await?;
    tx.commit().await?;
    Ok(updated)
}

/// 在调用方给定的事务里重算提醒（整改任务书 §5.3）。
///
/// 事务版本与独立版本共用同一段逻辑，避免两处实现漂移。
pub async fn recompute_task_reminders_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
) -> AppResult<i64> {
    let row = sqlx::query("SELECT planned_at, due_at FROM tasks WHERE id = ?1")
        .bind(task_id)
        .fetch_optional(&mut **tx)
        .await?;
    let Some(row) = row else { return Ok(0) };
    let planned: Option<String> = row.try_get("planned_at")?;
    let due: Option<String> = row.try_get("due_at")?;

    let reminders: Vec<Reminder> = sqlx::query_as::<_, Reminder>(
        "SELECT * FROM reminders WHERE task_id = ?1 AND kind <> 'custom'",
    )
    .bind(task_id)
    .fetch_all(&mut **tx)
    .await?;

    let now = to_db_time(utc_now());
    let mut updated = 0i64;

    for r in reminders {
        let kind = ReminderKind::from_db(&r.kind);
        let new_at = compute_remind_at(
            kind,
            r.offset_minutes,
            None,
            planned.as_deref(),
            due.as_deref(),
        )?;
        let auto_disabled = r.disabled_reason.as_deref() == Some(DISABLED_MISSING_BASE_TIME);

        match new_at {
            Some(at) => {
                if auto_disabled {
                    // 依赖时间补回来了：自动恢复启用（§6 的产品规则）。
                    // 注意只恢复"系统停用"的，用户自己关的绝不碰。
                    sqlx::query(
                        "UPDATE reminders SET remind_at = ?1, is_enabled = 1, disabled_reason = NULL,
                                              fired_at = NULL, updated_at = ?2
                         WHERE id = ?3",
                    )
                    .bind(&at)
                    .bind(&now)
                    .bind(&r.id)
                    .execute(&mut **tx)
                    .await?;
                    updated += 1;
                } else if at != r.remind_at {
                    // 时间变了：重置 fired_at，让提醒在新时刻重新生效。
                    // 若不清空，用户改完时间后提醒将永远不会再触发。
                    // `is_enabled` 与 `disabled_reason` 保持原样：
                    // 用户主动关掉的提醒，只更新时间、不擅自打开。
                    sqlx::query(
                        "UPDATE reminders SET remind_at = ?1, fired_at = NULL, updated_at = ?2 WHERE id = ?3",
                    )
                    .bind(&at)
                    .bind(&now)
                    .bind(&r.id)
                    .execute(&mut **tx)
                    .await?;
                    updated += 1;
                }
            }
            None => {
                // 依赖的时间字段被清空了：停用该提醒并保留记录，
                // 而不是删除——用户重新填上时间后它还能被恢复。
                // 只有"当前是启用状态"的才需要改；已经是用户停用的保持不动。
                if r.is_enabled == 1 {
                    sqlx::query(
                        "UPDATE reminders SET is_enabled = 0, disabled_reason = ?1, updated_at = ?2
                         WHERE id = ?3",
                    )
                    .bind(DISABLED_MISSING_BASE_TIME)
                    .bind(&now)
                    .bind(&r.id)
                    .execute(&mut **tx)
                    .await?;
                    updated += 1;
                }
            }
        }
    }

    Ok(updated)
}

/// 批量列出未触发提醒（设置页"待提醒"列表）
#[tauri::command]
pub async fn reminder_list_pending(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> AppResult<Vec<serde_json::Value>> {
    let lim = limit.unwrap_or(100).clamp(1, 500);
    let rows = sqlx::query(
        "SELECT r.id, r.task_id, r.kind, r.remind_at, r.fired_at, t.title
         FROM reminders r
         JOIN tasks t ON t.id = r.task_id
         WHERE r.is_enabled = 1 AND r.fired_at IS NULL AND t.deleted_at IS NULL
         ORDER BY r.remind_at ASC
         LIMIT ?1",
    )
    .bind(lim)
    .fetch_all(state.db.pool())
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(serde_json::json!({
            "id": r.try_get::<String, _>("id")?,
            "taskId": r.try_get::<String, _>("task_id")?,
            "taskTitle": r.try_get::<String, _>("title")?,
            "kind": r.try_get::<String, _>("kind")?,
            "remindAt": r.try_get::<String, _>("remind_at")?,
        }));
    }
    Ok(out)
}

/// 供"任务时间变化后重算提醒"的调用方使用（自带事务）。
///
/// **错误必须向上传播**（整改任务书 §5.4）：曾经这里是
/// `if let Err(e) = ... { log::warn!(...) }`，也就是"写条日志然后假装成功"，
/// 结果库里会出现"任务时间是新值、提醒时刻还是旧值"，而界面上显示保存成功。
/// 对数据一致性来说这不是合法的恢复策略，因此现在返回 `AppResult`。
///
/// 注意：`task_update` **不**走这里——它在自己的事务里调用
/// [`recompute_task_reminders_tx`]，保证两步原子。
pub async fn on_task_time_changed(db: &crate::db::Db, task_id: &str) -> AppResult<i64> {
    recompute_task_reminders(db, task_id).await
}

#[cfg(test)]
mod disable_reason_tests {
    //! 整改任务书 §6.4：自动暂停 vs 用户主动关闭，两者不能混为一谈。
    //!
    //! 这两条场景是**产品规则**的直接映射，必须用真实的数据库事务跑，
    //! 因为它们验的是"离散状态机"（启用 → 暂停 → 恢复）而不是某个纯函数。

    use super::*;
    use crate::db::Db;
    use sqlx::Row;

    async fn setup(name: &str) -> (Db, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("lumen-rem-{name}-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.expect("初始化数据库");
        (db, dir)
    }

    /// 建一个带截止时间的任务，并挂一条"到期前 30 分钟"的相对提醒
    async fn task_with_relative_reminder(db: &Db, due: Option<&str>) -> (String, String) {
        let task_id = uuid::Uuid::now_v7().to_string();
        let now = to_db_time(utc_now());
        sqlx::query(
            "INSERT INTO tasks (id, title, status, priority, due_at, has_due_time,
                                created_at, updated_at, sort_order, is_pinned, is_favorite,
                                has_planned_time, actual_minutes, occurrence_kind, is_exception, period_type)
             VALUES (?1, '带提醒的任务', 'todo', 0, ?2, 1, ?3, ?3, 1, 0, 0, 0, 0, 'single', 0, 'none')",
        )
        .bind(&task_id)
        .bind(due)
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

        let reminder_id = uuid::Uuid::now_v7().to_string();
        let remind_at = due.unwrap_or(&now).to_string();
        sqlx::query(
            "INSERT INTO reminders (id, task_id, kind, offset_minutes, remind_at, is_enabled,
                                    created_at, updated_at)
             VALUES (?1, ?2, 'before_due', 30, ?3, 1, ?4, ?4)",
        )
        .bind(&reminder_id)
        .bind(&task_id)
        .bind(&remind_at)
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

        (task_id, reminder_id)
    }

    async fn reminder_row(db: &Db, id: &str) -> (i64, Option<String>, String) {
        let row = sqlx::query(
            "SELECT is_enabled, disabled_reason, remind_at FROM reminders WHERE id = ?1",
        )
        .bind(id)
        .fetch_one(db.pool())
        .await
        .unwrap();
        (
            row.try_get("is_enabled").unwrap(),
            row.try_get("disabled_reason").unwrap(),
            row.try_get("remind_at").unwrap(),
        )
    }

    /// 场景 A：清空 due → 自动暂停（标记为 missing_base_time）；恢复 due → 自动恢复启用
    #[tokio::test]
    async fn scenario_a_auto_pause_then_auto_resume() {
        let (db, dir) = setup("a").await;
        let due = "2026-10-01T10:00:00.000Z";
        let (task_id, reminder_id) = task_with_relative_reminder(&db, Some(due)).await;

        // 建好之后先重算一次（等价于"通过命令创建提醒"的流程），
        // 让 remind_at 从占位值变成真正的"到期前 30 分钟"。
        recompute_task_reminders(&db, &task_id).await.unwrap();

        // 初始：启用，remind_at = 09:30
        let (enabled, reason, at) = reminder_row(&db, &reminder_id).await;
        assert_eq!(enabled, 1);
        assert!(reason.is_none());
        assert_eq!(at, "2026-10-01T09:30:00.000Z");

        // 清空截止时间 → 系统暂停，并记下原因
        sqlx::query("UPDATE tasks SET due_at = NULL, has_due_time = 0 WHERE id = ?1")
            .bind(&task_id)
            .execute(db.pool())
            .await
            .unwrap();
        recompute_task_reminders(&db, &task_id).await.unwrap();

        let (enabled, reason, _) = reminder_row(&db, &reminder_id).await;
        assert_eq!(enabled, 0, "缺少依赖时间时应被自动暂停");
        assert_eq!(
            reason.as_deref(),
            Some(DISABLED_MISSING_BASE_TIME),
            "必须记下是「系统因缺少时间」暂停的，而不是当成用户关闭"
        );

        // 恢复截止时间（改成另一天）→ 自动恢复启用，且 remind_at 跟着更新
        sqlx::query("UPDATE tasks SET due_at = ?1, has_due_time = 1 WHERE id = ?2")
            .bind("2026-10-02T12:00:00.000Z")
            .bind(&task_id)
            .execute(db.pool())
            .await
            .unwrap();
        recompute_task_reminders(&db, &task_id).await.unwrap();

        let (enabled, reason, at) = reminder_row(&db, &reminder_id).await;
        assert_eq!(
            enabled, 1,
            "依赖时间恢复后应自动重新启用（§6.2 的产品规则）"
        );
        assert!(reason.is_none(), "恢复后不应再带停用原因");
        assert_eq!(at, "2026-10-02T11:30:00.000Z", "提醒时刻要跟着新截止时间走");

        let _ = db.pool().close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 场景 B：用户主动关闭 → 改 due → **不得**自动帮用户打开
    #[tokio::test]
    async fn scenario_b_user_disabled_stays_disabled() {
        let (db, dir) = setup("b").await;
        let (task_id, reminder_id) =
            task_with_relative_reminder(&db, Some("2026-10-01T10:00:00.000Z")).await;

        // 用户主动关闭（等价于 reminder_set_enabled(false)，这里直接写库，
        // 因为命令函数要 State 而测试只有 Db；语义与命令实现保持一致）
        let now = to_db_time(utc_now());
        sqlx::query(
            "UPDATE reminders SET is_enabled = 0, disabled_reason = ?1, updated_at = ?2 WHERE id = ?3",
        )
        .bind(DISABLED_BY_USER)
        .bind(&now)
        .bind(&reminder_id)
        .execute(db.pool())
        .await
        .unwrap();

        // 改截止时间
        sqlx::query("UPDATE tasks SET due_at = ?1 WHERE id = ?2")
            .bind("2026-10-05T08:00:00.000Z")
            .bind(&task_id)
            .execute(db.pool())
            .await
            .unwrap();
        recompute_task_reminders(&db, &task_id).await.unwrap();

        let (enabled, reason, at) = reminder_row(&db, &reminder_id).await;
        assert_eq!(enabled, 0, "用户关掉的提醒不能被系统自动打开");
        assert_eq!(reason.as_deref(), Some(DISABLED_BY_USER));
        assert_eq!(
            at, "2026-10-05T07:30:00.000Z",
            "时刻仍应跟着任务更新，这样用户手动打开时就是对的"
        );

        // 清空再填回时间也不得改变用户的选择
        sqlx::query("UPDATE tasks SET due_at = NULL WHERE id = ?1")
            .bind(&task_id)
            .execute(db.pool())
            .await
            .unwrap();
        recompute_task_reminders(&db, &task_id).await.unwrap();
        let (enabled, reason, _) = reminder_row(&db, &reminder_id).await;
        assert_eq!(enabled, 0);
        assert_eq!(
            reason.as_deref(),
            Some(DISABLED_BY_USER),
            "用户关闭的原因不能被系统原因覆盖"
        );

        let _ = db.pool().close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 自定义（absolute）提醒不受任务时间影响，重算时必须跳过
    #[tokio::test]
    async fn custom_reminders_are_not_touched_by_recompute() {
        let (db, dir) = setup("custom").await;
        let (task_id, _) = task_with_relative_reminder(&db, Some("2026-10-01T10:00:00.000Z")).await;

        let custom_id = uuid::Uuid::now_v7().to_string();
        let now = to_db_time(utc_now());
        sqlx::query(
            "INSERT INTO reminders (id, task_id, kind, remind_at, is_enabled, created_at, updated_at)
             VALUES (?1, ?2, 'custom', '2026-12-31T00:00:00.000Z', 1, ?3, ?3)",
        )
        .bind(&custom_id)
        .bind(&task_id)
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

        sqlx::query("UPDATE tasks SET due_at = '2026-11-01T00:00:00.000Z' WHERE id = ?1")
            .bind(&task_id)
            .execute(db.pool())
            .await
            .unwrap();
        recompute_task_reminders(&db, &task_id).await.unwrap();

        let (enabled, _, at) = reminder_row(&db, &custom_id).await;
        assert_eq!(enabled, 1, "自定义提醒不应被停用");
        assert_eq!(at, "2026-12-31T00:00:00.000Z", "自定义提醒的时刻不该被改");

        let _ = db.pool().close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 迁移 0005 的兼容性：老的 `is_enabled = 0` 记录必须被当作「用户关闭」，
    /// 而不是被自动恢复——宁可让用户手动打开，也不要擅自打开他关掉的提醒。
    #[tokio::test]
    async fn migration_marks_legacy_disabled_as_user_disabled() {
        let (db, dir) = setup("legacy").await;
        let (_, reminder_id) =
            task_with_relative_reminder(&db, Some("2026-10-01T10:00:00.000Z")).await;

        // 模拟"升级前就是停用状态"的老数据
        sqlx::query("UPDATE reminders SET is_enabled = 0, disabled_reason = NULL WHERE id = ?1")
            .bind(&reminder_id)
            .execute(db.pool())
            .await
            .unwrap();

        // 迁移脚本里的那条 UPDATE 语句：
        sqlx::query("UPDATE reminders SET disabled_reason = 'user' WHERE is_enabled = 0")
            .execute(db.pool())
            .await
            .unwrap();

        let (enabled, reason, _) = reminder_row(&db, &reminder_id).await;
        assert_eq!(enabled, 0);
        assert_eq!(
            reason.as_deref(),
            Some(DISABLED_BY_USER),
            "老数据应保守地视为用户关闭"
        );

        let _ = db.pool().close().await;
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod due_selection_tests {
    //! 整改任务书 §15：提醒的"该不该发"必须能被自动测试直接验证。
    //!
    //! 这些用例覆盖的是**过滤条件**本身（调度循环的唯一判定依据）：
    //! 已完成/已归档/回收站里的任务不提醒、已触发的不重复、
    //! 被停用的不发、到点的按时间顺序、单次有上限。

    use super::*;
    use crate::db::Db;

    async fn setup(name: &str) -> (Db, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("lumen-due-{name}-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.expect("初始化数据库");
        (db, dir)
    }

    /// 建一个任务 + 一条提醒，返回 (task_id, reminder_id)
    async fn add(
        db: &Db,
        title: &str,
        status: &str,
        deleted: bool,
        remind_at: &str,
        enabled: bool,
        fired_at: Option<&str>,
    ) -> (String, String) {
        let task_id = uuid::Uuid::now_v7().to_string();
        let rid = uuid::Uuid::now_v7().to_string();
        let now = to_db_time(utc_now());
        sqlx::query(
            "INSERT INTO tasks (id, title, status, priority, deleted_at, created_at, updated_at,
                                sort_order, is_pinned, is_favorite, has_planned_time, has_due_time,
                                actual_minutes, occurrence_kind, is_exception, period_type)
             VALUES (?1, ?2, ?3, 0, ?4, ?5, ?5, 1, 0, 0, 0, 0, 0, 'single', 0, 'none')",
        )
        .bind(&task_id)
        .bind(title)
        .bind(status)
        .bind(if deleted { Some(now.clone()) } else { None })
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO reminders (id, task_id, kind, remind_at, is_enabled, fired_at, created_at, updated_at)
             VALUES (?1, ?2, 'custom', ?3, ?4, ?5, ?6, ?6)",
        )
        .bind(&rid)
        .bind(&task_id)
        .bind(remind_at)
        .bind(enabled as i64)
        .bind(fired_at)
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

        (task_id, rid)
    }

    #[tokio::test]
    async fn only_unfired_enabled_and_not_done_tasks_are_due() {
        let (db, dir) = setup("filter").await;
        let past = "2026-01-01T00:00:00.000Z";
        let now = "2026-06-01T00:00:00.000Z";

        add(&db, "正常待办", "todo", false, past, true, None).await;
        add(&db, "已完成的", "done", false, past, true, None).await;
        add(&db, "已归档的", "archived", false, past, true, None).await;
        add(&db, "在回收站的", "todo", true, past, true, None).await;
        add(&db, "已停用的", "todo", false, past, false, None).await;
        add(&db, "已经发过的", "todo", false, past, true, Some(past)).await;
        add(
            &db,
            "还没到点",
            "todo",
            false,
            "2026-12-01T00:00:00.000Z",
            true,
            None,
        )
        .await;

        let due = due_reminders(&db, now, MAX_PER_TICK).await.unwrap();
        let titles: Vec<&str> = due.iter().map(|d| d.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["正常待办"],
            "只应发出「到点 + 启用 + 未触发 + 任务未完成未删除」的那一条"
        );

        let _ = db.pool().close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn due_list_is_ordered_and_capped() {
        let (db, dir) = setup("order").await;
        let now = "2026-06-01T00:00:00.000Z";
        // 故意乱序插入，验证按 remind_at 升序返回
        for (t, at) in [
            ("第三", "2026-05-03T00:00:00.000Z"),
            ("第一", "2026-05-01T00:00:00.000Z"),
            ("第二", "2026-05-02T00:00:00.000Z"),
        ] {
            add(&db, t, "todo", false, at, true, None).await;
        }
        let due = due_reminders(&db, now, MAX_PER_TICK).await.unwrap();
        let titles: Vec<&str> = due.iter().map(|d| d.title.as_str()).collect();
        assert_eq!(titles, vec!["第一", "第二", "第三"], "必须按时间先后发");

        // 上限必须生效，避免异常数据一次性弹满屏幕
        let capped = due_reminders(&db, now, 2).await.unwrap();
        assert_eq!(capped.len(), 2, "limit 应生效");

        let _ = db.pool().close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 超过补发窗口的提醒要能被标记成 expired，而不是无限期地"待补发"
    #[tokio::test]
    async fn missed_reminders_beyond_grace_are_marked_expired() {
        let (db, dir) = setup("expired").await;
        let now = to_db_time(utc_now());

        // 很久以前的一条（超出补发窗口）
        let (_, old_id) = add(
            &db,
            "很久以前",
            "todo",
            false,
            "2020-01-01T00:00:00.000Z",
            true,
            None,
        )
        .await;
        // 刚刚错过的（在窗口内，不该被标记为过期）
        let recent = to_db_time(utc_now() - chrono::Duration::minutes(5));
        let (_, recent_id) = add(&db, "刚刚错过", "todo", false, &recent, true, None).await;

        // 与 reminder_check_missed 相同的判定：超过 grace 的标记为 expired
        let cutoff = to_db_time(utc_now() - chrono::Duration::minutes(360));
        sqlx::query(
            "UPDATE reminders SET fired_at = 'expired', updated_at = ?1
             WHERE is_enabled = 1 AND fired_at IS NULL AND remind_at < ?2",
        )
        .bind(&now)
        .bind(&cutoff)
        .execute(db.pool())
        .await
        .unwrap();

        let old_state: Option<String> =
            sqlx::query_scalar("SELECT fired_at FROM reminders WHERE id = ?1")
                .bind(&old_id)
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(old_state.as_deref(), Some("expired"), "超窗的应被明确作废");

        let recent_state: Option<String> =
            sqlx::query_scalar("SELECT fired_at FROM reminders WHERE id = ?1")
                .bind(&recent_id)
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert!(recent_state.is_none(), "窗口内的不应被作废，仍要补发");

        // 作废后不应再出现在待发列表里（不会"过期了还弹"）
        let due = due_reminders(&db, &now, MAX_PER_TICK).await.unwrap();
        assert!(
            due.iter().all(|d| d.id != old_id),
            "标记 expired 之后不应再被发出"
        );

        let _ = db.pool().close().await;
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLANNED: &str = "2026-09-23T01:00:00.000Z";
    const DUE: &str = "2026-09-24T09:30:00.000Z";

    #[test]
    fn at_due_uses_due_time_exactly() {
        let got = compute_remind_at(ReminderKind::AtDue, None, None, Some(PLANNED), Some(DUE))
            .unwrap()
            .unwrap();
        assert_eq!(got, DUE);
    }

    #[test]
    fn at_planned_uses_planned_time_exactly() {
        let got = compute_remind_at(
            ReminderKind::AtPlanned,
            None,
            None,
            Some(PLANNED),
            Some(DUE),
        )
        .unwrap()
        .unwrap();
        assert_eq!(got, PLANNED);
    }

    #[test]
    fn before_due_subtracts_offset() {
        // 提前 30 分钟：09:30 → 09:00
        let got = compute_remind_at(ReminderKind::BeforeDue, Some(30), None, None, Some(DUE))
            .unwrap()
            .unwrap();
        assert_eq!(got, "2026-09-24T09:00:00.000Z");
    }

    #[test]
    fn before_planned_subtracts_offset_across_day() {
        // 提前 120 分钟：UTC 01:00 → 前一天 23:00，跨日必须正确
        let got = compute_remind_at(
            ReminderKind::BeforePlanned,
            Some(120),
            None,
            Some(PLANNED),
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(got, "2026-09-22T23:00:00.000Z");
    }

    #[test]
    fn custom_uses_given_time() {
        let got = compute_remind_at(
            ReminderKind::Custom,
            None,
            Some("2026-09-25T00:00:00.000Z"),
            Some(PLANNED),
            Some(DUE),
        )
        .unwrap()
        .unwrap();
        assert_eq!(got, "2026-09-25T00:00:00.000Z");
    }

    /// 任务还没填截止时间时，不应报错也不应造出提醒——
    /// 用户可能先建提醒再填时间，这是合法的操作顺序。
    #[test]
    fn missing_base_time_yields_none_not_error() {
        assert!(
            compute_remind_at(ReminderKind::AtDue, None, None, None, None)
                .unwrap()
                .is_none()
        );
        assert!(
            compute_remind_at(ReminderKind::BeforeDue, Some(30), None, None, None)
                .unwrap()
                .is_none()
        );
        // 计划时间缺失但截止时间存在时，AtDue 仍可正常推导
        assert!(
            compute_remind_at(ReminderKind::AtDue, None, None, None, Some(DUE))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn before_kind_requires_offset() {
        let e = compute_remind_at(ReminderKind::BeforeDue, None, None, None, Some(DUE));
        assert!(e.is_err(), "相对提醒必须提供提前分钟数");
    }

    #[test]
    fn offset_range_is_enforced() {
        assert!(
            compute_remind_at(ReminderKind::BeforeDue, Some(0), None, None, Some(DUE)).is_err()
        );
        assert!(
            compute_remind_at(ReminderKind::BeforeDue, Some(-5), None, None, Some(DUE)).is_err()
        );
        assert!(compute_remind_at(
            ReminderKind::BeforeDue,
            Some(OFFSET_MAX + 1),
            None,
            None,
            Some(DUE)
        )
        .is_err());
        // 边界值应当可用
        assert!(compute_remind_at(
            ReminderKind::BeforeDue,
            Some(OFFSET_MIN),
            None,
            None,
            Some(DUE)
        )
        .is_ok());
        assert!(compute_remind_at(
            ReminderKind::BeforeDue,
            Some(OFFSET_MAX),
            None,
            None,
            Some(DUE)
        )
        .is_ok());
    }

    #[test]
    fn custom_requires_time() {
        assert!(compute_remind_at(ReminderKind::Custom, None, None, None, None).is_err());
        let empty = String::new();
        assert!(compute_remind_at(ReminderKind::Custom, None, Some(&empty), None, None).is_err());
    }

    #[test]
    fn invalid_time_string_is_rejected() {
        assert!(
            compute_remind_at(ReminderKind::AtDue, None, None, None, Some("not-a-time")).is_err()
        );
        // 只有日期没有时区，语义不明确，必须拒绝
        assert!(
            compute_remind_at(ReminderKind::AtDue, None, None, None, Some("2026-09-24")).is_err()
        );
    }

    /// 产出必须是固定 24 字符宽度，与全库时间格式一致
    #[test]
    fn output_is_fixed_width_utc() {
        let got = compute_remind_at(ReminderKind::BeforeDue, Some(45), None, None, Some(DUE))
            .unwrap()
            .unwrap();
        assert_eq!(got.len(), 24);
        assert!(got.ends_with('Z'));
    }

    #[test]
    fn kind_roundtrips_through_db_values() {
        for k in [
            ReminderKind::AtDue,
            ReminderKind::BeforeDue,
            ReminderKind::AtPlanned,
            ReminderKind::BeforePlanned,
            ReminderKind::Custom,
        ] {
            assert_eq!(ReminderKind::from_db(k.as_db()), k);
        }
        // 未知值安全回落为 Custom，不 panic
        assert_eq!(ReminderKind::from_db("weird"), ReminderKind::Custom);
    }
}
