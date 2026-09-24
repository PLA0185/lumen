//! 专注模式与番茄钟（任务书 §4.4）。
//!
//! ## 关键要求：退出或断电后能恢复计时状态，或明确说明中断
//!
//! 因此计时**不以内存中的定时器为准**，而是每次操作都落库：
//! - 会话存 `started_at`、`elapsed_seconds`（已累计）、`last_resumed_at`；
//! - 实际经过时间 = `elapsed_seconds + (now - last_resumed_at)`（运行中时）；
//! - 程序被杀掉后重新打开，可以从数据库恢复到"仍在运行"或"已中断"。
//!
//! 一个刻意的判断：**不做"假装时间没过去"的处理**。
//! 如果程序在运行中被强杀，重新打开时会话仍是 running 状态——
//! 这时我们把从 last_resumed_at 到现在的全部时间都算进去，
//! 因为用户很可能确实一直在工作。但如果中断时间超过番茄钟时长，
//! 会标记为 `interrupted` 并要求用户确认，而不是默默给一个虚高的时长。
//!
//! ## 与任务的关联
//!
//! 会话可以绑定任务（`task_id`）也可以不绑定（纯计时）。
//! 完成/结束时把实际秒数累加到任务的 `actual_minutes`，
//! 这样统计页的"实际耗时"才反映真实投入（§7）。

use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::commands::AppState;
use crate::db::{to_db_time, utc_now, Db};
use crate::error::{AppError, AppResult};

/// 默认番茄钟时长（分钟）
pub const DEFAULT_POMODORO_MINUTES: i64 = 25;

/// 允许的计时时长范围（分钟）
const MIN_MINUTES: i64 = 1;
const MAX_MINUTES: i64 = 8 * 60;

/// 判定"中断过久"的阈值：超过预期时长的 2 倍即认为用户已经离开
const INTERRUPT_FACTOR: i64 = 2;

/// 专注会话
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct FocusSession {
    pub id: String,
    pub task_id: Option<String>,
    /// pomodoro = 倒计时；stopwatch = 正计时
    pub kind: String,
    /// idle / running / paused / finished / interrupted
    pub state: String,
    pub planned_seconds: i64,
    pub elapsed_seconds: i64,
    pub started_at: Option<String>,
    pub last_resumed_at: Option<String>,
    pub ended_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 会话的实时视图（前端据此渲染计时器）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusView {
    #[serde(flatten)]
    pub session: FocusSession,
    /// 当前已进行的秒数（运行中含从上次恢复到现在的时间）
    pub current_seconds: i64,
    /// 剩余秒数（正计时时为 None）
    pub remaining_seconds: Option<i64>,
    /// 进度百分比 0–100
    pub progress: i64,
    /// 是否已到时间
    pub is_due: bool,
    /// 关联任务标题
    pub task_title: Option<String>,
}

/// 开始计时的输入
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartFocusInput {
    /// 绑定任务（可选）
    #[serde(default)]
    pub task_id: Option<String>,
    /// pomodoro / stopwatch
    #[serde(default)]
    pub kind: Option<String>,
    /// 番茄钟时长（分钟）；正计时忽略
    #[serde(default)]
    pub minutes: Option<i64>,
}

/// 结束计时的输入
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndFocusInput {
    pub session_id: String,
    /// 是否把时长累加到任务的 actual_minutes（默认是）
    #[serde(default = "default_true")]
    pub record_to_task: bool,
    /// 用户备注（可空）
    #[serde(default)]
    pub note: Option<String>,
}

fn default_true() -> bool {
    true
}

/// 休息记录（§4.4 的番茄钟通常包含休息，但任务书未强制；
/// 这里只记录"完成了几轮"，不做复杂的休息调度）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusSummary {
    /// 今天完成的专注轮数
    pub today_sessions: i64,
    /// 今天累计专注秒数
    pub today_seconds: i64,
    /// 最近 7 天累计秒数
    pub week_seconds: i64,
    /// 当前是否有进行中的会话
    pub active_session: Option<FocusView>,
}

// =============================================================================
// 时间计算
// =============================================================================

/// 解析 UTC 时间字符串
fn parse_utc(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

/// 计算会话当前已进行的秒数。
///
/// 这是整个模块的核心算式，单独抽出来以便测试：
/// - running：已累计 + 从 last_resumed_at 到现在的真实耗时；
/// - paused / finished / interrupted：只算已累计的部分。
fn session_seconds(s: &FocusSession, now: chrono::DateTime<chrono::Utc>) -> i64 {
    let base = s.elapsed_seconds.max(0);
    if s.state != "running" {
        return base;
    }
    let Some(last) = s.last_resumed_at.as_deref().and_then(parse_utc) else {
        return base;
    };
    // 防御时钟回拨：负数按 0 处理，不让计时倒退
    let delta = (now - last).num_seconds().max(0);
    base + delta
}

/// 构造会话的实时视图
fn to_view(
    s: FocusSession,
    now: chrono::DateTime<chrono::Utc>,
    task_title: Option<String>,
) -> FocusView {
    let current = session_seconds(&s, now);
    let is_pomodoro = s.kind == "pomodoro";
    let remaining = if is_pomodoro {
        Some((s.planned_seconds - current).max(0))
    } else {
        None
    };
    let progress = if s.planned_seconds > 0 {
        ((current * 100) / s.planned_seconds).clamp(0, 100)
    } else {
        0
    };
    FocusView {
        is_due: is_pomodoro && current >= s.planned_seconds,
        session: s,
        current_seconds: current,
        remaining_seconds: remaining,
        progress,
        task_title,
    }
}

// =============================================================================
// 命令
// =============================================================================

/// 开始一次专注。
///
/// 若已有进行中的会话，会先把它结束（记为 interrupted），
/// 避免出现两个同时运行的计时器——那会让"今天专注了多久"无法计算。
#[tauri::command]
pub async fn focus_start(
    state: State<'_, AppState>,
    input: StartFocusInput,
) -> AppResult<FocusView> {
    let db = &state.db;

    // 1) 处理已存在的进行中会话
    if let Some(existing) = active_session(db).await? {
        log::info!(
            "开始新专注前，先把上一个仍在运行的会话 {} 标记为中断",
            existing.id
        );
        let now = to_db_time(utc_now());
        sqlx::query(
            "UPDATE focus_sessions SET state = 'interrupted', ended_at = ?1, updated_at = ?1,
                    elapsed_seconds = ?2
             WHERE id = ?3",
        )
        .bind(&now)
        .bind(session_seconds(&existing, utc_now()))
        .bind(&existing.id)
        .execute(db.pool())
        .await?;
    }

    let kind = input.kind.as_deref().unwrap_or("pomodoro").to_string();
    if kind != "pomodoro" && kind != "stopwatch" {
        return Err(AppError::validation(format!("不支持的计时类型：{kind}"))
            .with_hint("允许值：pomodoro（倒计时）/ stopwatch（正计时）"));
    }

    let minutes = input.minutes.unwrap_or(DEFAULT_POMODORO_MINUTES);
    if kind == "pomodoro" && !(MIN_MINUTES..=MAX_MINUTES).contains(&minutes) {
        return Err(
            AppError::validation(format!("时长超出范围：{minutes} 分钟"))
                .with_hint(format!("允许 {MIN_MINUTES}–{MAX_MINUTES} 分钟")),
        );
    }

    // 2) 校验任务存在（若有绑定）
    if let Some(tid) = input.task_id.as_deref().filter(|s| !s.is_empty()) {
        let n: i64 =
            sqlx::query("SELECT COUNT(*) AS n FROM tasks WHERE id = ?1 AND deleted_at IS NULL")
                .bind(tid)
                .fetch_one(db.pool())
                .await?
                .try_get("n")?;
        if n == 0 {
            return Err(AppError::not_found("任务", tid));
        }
    }

    let id = uuid::Uuid::now_v7().to_string();
    let now_dt = utc_now();
    let now = to_db_time(now_dt);

    sqlx::query(
        "INSERT INTO focus_sessions
            (id, task_id, kind, state, planned_seconds, elapsed_seconds,
             started_at, last_resumed_at, ended_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'running', ?4, 0, ?5, ?5, NULL, ?5, ?5)",
    )
    .bind(&id)
    .bind(input.task_id.as_deref().filter(|s| !s.is_empty()))
    .bind(&kind)
    .bind(minutes * 60)
    .bind(&now)
    .execute(db.pool())
    .await?;

    log::info!(
        "开始专注会话 {id}（{kind}，{} 分钟）",
        if kind == "pomodoro" { minutes } else { 0 }
    );

    load_view(db, &id).await
}

/// 读取进行中的会话（没有则返回 None）
async fn active_session(db: &Db) -> AppResult<Option<FocusSession>> {
    let s = sqlx::query_as::<_, FocusSession>(
        "SELECT * FROM focus_sessions WHERE state = 'running' ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_optional(db.pool())
    .await?;
    Ok(s)
}

/// 读取会话并组装视图
async fn load_view(db: &Db, id: &str) -> AppResult<FocusView> {
    let s = sqlx::query_as::<_, FocusSession>("SELECT * FROM focus_sessions WHERE id = ?1")
        .bind(id)
        .fetch_optional(db.pool())
        .await?;
    let Some(s) = s else {
        return Err(AppError::not_found("专注会话", id));
    };

    let title: Option<String> = match s.task_id.as_deref() {
        Some(tid) => sqlx::query("SELECT title FROM tasks WHERE id = ?1")
            .bind(tid)
            .fetch_optional(db.pool())
            .await?
            .and_then(|r| r.try_get("title").ok()),
        None => None,
    };

    Ok(to_view(s, utc_now(), title))
}

/// 查询当前会话状态；同时处理"程序被杀掉后残留的 running 会话"
#[tauri::command]
pub async fn focus_current(state: State<'_, AppState>) -> AppResult<Option<FocusView>> {
    let db = &state.db;
    let Some(s) = active_session(db).await? else {
        return Ok(None);
    };

    let now = utc_now();
    let secs = session_seconds(&s, now);

    // 倒计时会话已远超预期时长 → 判定为用户已离开，标记为中断并告知，
    // 而不是继续显示一个"还在计时"的假象，也不是默默给一个虚高的时长。
    if s.kind == "pomodoro" && s.planned_seconds > 0 && secs > s.planned_seconds * INTERRUPT_FACTOR
    {
        let ts = to_db_time(now);
        sqlx::query(
            "UPDATE focus_sessions SET state = 'interrupted', elapsed_seconds = ?1,
                    ended_at = ?2, updated_at = ?2 WHERE id = ?3",
        )
        .bind(s.planned_seconds) // 只记到预期时长，不把离开的时间也算成专注
        .bind(&ts)
        .bind(&s.id)
        .execute(db.pool())
        .await?;

        log::info!(
            "专注会话 {} 中断过久（已过 {} 秒，预期 {} 秒），标记为 interrupted",
            s.id,
            secs,
            s.planned_seconds
        );

        // 返回更新后的状态，让界面明确显示"已中断"
        return Ok(Some(load_view(db, &s.id).await?));
    }

    Ok(Some(load_view(db, &s.id).await?))
}

/// 暂停
#[tauri::command]
pub async fn focus_pause(state: State<'_, AppState>, session_id: String) -> AppResult<FocusView> {
    let db = &state.db;
    let s = get_session(db, &session_id).await?;
    if s.state != "running" {
        return Err(AppError::conflict(format!(
            "当前状态是「{}」，无法暂停",
            state_label(&s.state)
        ))
        .with_hint("只有进行中的专注可以暂停"));
    }

    let now_dt = utc_now();
    let secs = session_seconds(&s, now_dt);
    let ts = to_db_time(now_dt);

    sqlx::query(
        "UPDATE focus_sessions SET state = 'paused', elapsed_seconds = ?1,
                last_resumed_at = NULL, updated_at = ?2 WHERE id = ?3",
    )
    .bind(secs)
    .bind(&ts)
    .bind(&session_id)
    .execute(db.pool())
    .await?;

    load_view(db, &session_id).await
}

/// 恢复
#[tauri::command]
pub async fn focus_resume(state: State<'_, AppState>, session_id: String) -> AppResult<FocusView> {
    let db = &state.db;
    let s = get_session(db, &session_id).await?;
    if s.state != "paused" && s.state != "interrupted" {
        return Err(AppError::conflict(format!(
            "当前状态是「{}」，无法恢复",
            state_label(&s.state)
        ))
        .with_hint("只有已暂停或已中断的专注可以恢复"));
    }

    let now = to_db_time(utc_now());
    sqlx::query(
        "UPDATE focus_sessions SET state = 'running', last_resumed_at = ?1,
                ended_at = NULL, updated_at = ?1 WHERE id = ?2",
    )
    .bind(&now)
    .bind(&session_id)
    .execute(db.pool())
    .await?;

    load_view(db, &session_id).await
}

/// 结束并记录
#[tauri::command]
pub async fn focus_end(state: State<'_, AppState>, input: EndFocusInput) -> AppResult<FocusView> {
    let db = &state.db;
    let s = get_session(db, &input.session_id).await?;

    if s.state == "finished" {
        return Err(AppError::conflict("该专注已经结束过了"));
    }

    let now_dt = utc_now();
    let secs = session_seconds(&s, now_dt);
    let ts = to_db_time(now_dt);

    // 结束后把累计时长写入会话
    sqlx::query(
        "UPDATE focus_sessions SET state = 'finished', elapsed_seconds = ?1,
                last_resumed_at = NULL, ended_at = ?2, updated_at = ?2 WHERE id = ?3",
    )
    .bind(secs)
    .bind(&ts)
    .bind(&input.session_id)
    .execute(db.pool())
    .await?;

    // 累加到任务的 actual_minutes（§7 的"实际耗时"数据来源）
    if input.record_to_task {
        if let Some(tid) = s.task_id.as_deref() {
            let minutes = (secs / 60).max(0);
            if minutes > 0 {
                sqlx::query(
                    "UPDATE tasks SET actual_minutes = COALESCE(actual_minutes, 0) + ?1,
                            updated_at = ?2 WHERE id = ?3",
                )
                .bind(minutes)
                .bind(&ts)
                .bind(tid)
                .execute(db.pool())
                .await?;
                log::info!("专注 {secs} 秒已累加到任务 {tid} 的实际耗时（{minutes} 分钟）");
            }
        }
    }

    let _ = input.note; // 备注字段预留给后续的会话笔记功能

    load_view(db, &input.session_id).await
}

/// 放弃当前专注（不记录时长）
#[tauri::command]
pub async fn focus_cancel(state: State<'_, AppState>, session_id: String) -> AppResult<bool> {
    let db = &state.db;
    let s = get_session(db, &session_id).await?;
    let now = to_db_time(utc_now());
    sqlx::query(
        "UPDATE focus_sessions SET state = 'interrupted', ended_at = ?1, updated_at = ?1 WHERE id = ?2",
    )
    .bind(&now)
    .bind(&session_id)
    .execute(db.pool())
    .await?;
    log::info!("专注会话 {} 已放弃（不记录时长）", s.id);
    Ok(true)
}

async fn get_session(db: &Db, id: &str) -> AppResult<FocusSession> {
    let s = sqlx::query_as::<_, FocusSession>("SELECT * FROM focus_sessions WHERE id = ?1")
        .bind(id)
        .fetch_optional(db.pool())
        .await?;
    s.ok_or_else(|| AppError::not_found("专注会话", id))
}

fn state_label(s: &str) -> &str {
    match s {
        "idle" => "空闲",
        "running" => "进行中",
        "paused" => "已暂停",
        "finished" => "已结束",
        "interrupted" => "已中断",
        other => other,
    }
}

/// 今日与本周的专注汇总
#[tauri::command]
pub async fn focus_summary(state: State<'_, AppState>) -> AppResult<FocusSummary> {
    let db = &state.db;
    let today = chrono::Local::now().date_naive();
    let today_str = today.format("%Y-%m-%d").to_string();
    let week_start = today
        - chrono::Duration::days((today.format("%u").to_string().parse::<i64>().unwrap_or(1)) - 1);

    // 只统计"已完成"的会话——中断的时长不应算作有效专注
    let today_row = sqlx::query(
        "SELECT COUNT(*) AS n, COALESCE(SUM(elapsed_seconds), 0) AS secs
         FROM focus_sessions
         WHERE state = 'finished'
           AND strftime('%Y-%m-%d', ended_at, 'localtime') = ?1",
    )
    .bind(&today_str)
    .fetch_one(db.pool())
    .await?;

    let week_secs: i64 = sqlx::query(
        "SELECT COALESCE(SUM(elapsed_seconds), 0) AS secs FROM focus_sessions
         WHERE state = 'finished'
           AND strftime('%Y-%m-%d', ended_at, 'localtime') >= ?1",
    )
    .bind(week_start.format("%Y-%m-%d").to_string())
    .fetch_one(db.pool())
    .await?
    .try_get("secs")?;

    let active = match active_session(db).await? {
        Some(s) => {
            let title: Option<String> = match s.task_id.as_deref() {
                Some(tid) => sqlx::query("SELECT title FROM tasks WHERE id = ?1")
                    .bind(tid)
                    .fetch_optional(db.pool())
                    .await?
                    .and_then(|r| r.try_get("title").ok()),
                None => None,
            };
            Some(to_view(s, utc_now(), title))
        }
        None => None,
    };

    Ok(FocusSummary {
        today_sessions: today_row.try_get("n")?,
        today_seconds: today_row.try_get("secs")?,
        week_seconds: week_secs,
        active_session: active,
    })
}

/// 把秒数格式化为 `HH:MM:SS` 或 `MM:SS`（前端也用同一规则，便于对照）
#[tauri::command]
pub async fn focus_format_seconds(seconds: i64) -> AppResult<String> {
    Ok(format_hms(seconds))
}

/// 秒 → `MM:SS` / `HH:MM:SS`
pub fn format_hms(seconds: i64) -> String {
    let s = seconds.max(0);
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 {
        format!("{h:02}:{m:02}:{sec:02}")
    } else {
        format!("{m:02}:{sec:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(
        state: &str,
        elapsed: i64,
        resumed: Option<&str>,
        planned: i64,
        kind: &str,
    ) -> FocusSession {
        FocusSession {
            id: "s1".into(),
            task_id: None,
            kind: kind.into(),
            state: state.into(),
            planned_seconds: planned,
            elapsed_seconds: elapsed,
            started_at: Some("2026-09-23T01:00:00.000Z".into()),
            last_resumed_at: resumed.map(|s| s.to_string()),
            ended_at: None,
            created_at: "2026-09-23T01:00:00.000Z".into(),
            updated_at: "2026-09-23T01:00:00.000Z".into(),
        }
    }

    fn at(s: &str) -> chrono::DateTime<chrono::Utc> {
        parse_utc(s).unwrap()
    }

    // ------------------------- 时长计算 -------------------------

    /// 进行中的会话要算上"从上次恢复到现在"的真实时间
    #[test]
    fn running_session_accumulates_realtime() {
        let s = mk(
            "running",
            300,
            Some("2026-09-23T01:10:00.000Z"),
            1500,
            "pomodoro",
        );
        let now = at("2026-09-23T01:12:00.000Z");
        assert_eq!(session_seconds(&s, now), 300 + 120);
    }

    /// 暂停后时间必须停止累积，否则用户会发现自己"被计时"了
    #[test]
    fn paused_session_does_not_accumulate() {
        let s = mk(
            "paused",
            300,
            Some("2026-09-23T01:10:00.000Z"),
            1500,
            "pomodoro",
        );
        let far_later = at("2026-09-23T05:00:00.000Z");
        assert_eq!(
            session_seconds(&s, far_later),
            300,
            "暂停后不管过多久都应是 300 秒"
        );
    }

    #[test]
    fn finished_and_interrupted_use_stored_elapsed() {
        for st in ["finished", "interrupted", "idle"] {
            let s = mk(st, 600, Some("2026-09-23T01:10:00.000Z"), 1500, "pomodoro");
            assert_eq!(session_seconds(&s, at("2026-09-23T09:00:00.000Z")), 600);
        }
    }

    /// 时钟回拨（用户改了系统时间）不能让计时倒退
    #[test]
    fn clock_going_backwards_does_not_reduce_elapsed() {
        let s = mk(
            "running",
            300,
            Some("2026-09-23T01:10:00.000Z"),
            1500,
            "pomodoro",
        );
        // "现在"早于 last_resumed_at
        let now = at("2026-09-23T01:05:00.000Z");
        assert_eq!(session_seconds(&s, now), 300, "时钟回拨时应保持已累计值");
    }

    #[test]
    fn running_without_resume_timestamp_falls_back_to_elapsed() {
        let s = mk("running", 120, None, 1500, "pomodoro");
        assert_eq!(session_seconds(&s, at("2026-09-23T02:00:00.000Z")), 120);
    }

    #[test]
    fn negative_stored_elapsed_is_clamped() {
        let s = mk("paused", -50, None, 1500, "pomodoro");
        assert_eq!(session_seconds(&s, at("2026-09-23T02:00:00.000Z")), 0);
    }

    // ------------------------- 视图 -------------------------

    #[test]
    fn pomodoro_view_has_remaining_and_due() {
        let s = mk(
            "running",
            1400,
            Some("2026-09-23T01:00:00.000Z"),
            1500,
            "pomodoro",
        );
        let v = to_view(s, at("2026-09-23T01:01:40.000Z"), None);
        assert_eq!(v.current_seconds, 1500);
        assert_eq!(v.remaining_seconds, Some(0));
        assert!(v.is_due, "到时后 is_due 应为 true");
        assert_eq!(v.progress, 100);
    }

    /// 正计时没有"剩余时间"，界面不应显示倒计时
    #[test]
    fn stopwatch_view_has_no_remaining() {
        let s = mk(
            "running",
            100,
            Some("2026-09-23T01:00:00.000Z"),
            0,
            "stopwatch",
        );
        let v = to_view(s, at("2026-09-23T01:00:50.000Z"), None);
        assert_eq!(v.current_seconds, 150);
        assert_eq!(v.remaining_seconds, None);
        // planned_seconds 为 0 时不能出现除零
        assert_eq!(v.progress, 0);
        assert!(!v.is_due, "正计时永不 is_due");
    }

    #[test]
    fn progress_is_capped_at_hundred() {
        let s = mk(
            "running",
            3000,
            Some("2026-09-23T01:00:00.000Z"),
            1500,
            "pomodoro",
        );
        let v = to_view(s, at("2026-09-23T01:10:00.000Z"), None);
        assert_eq!(v.progress, 100, "进度不应超过 100");
        assert_eq!(v.remaining_seconds, Some(0), "剩余不应为负");
    }

    #[test]
    fn view_carries_task_title() {
        let s = mk(
            "running",
            0,
            Some("2026-09-23T01:00:00.000Z"),
            1500,
            "pomodoro",
        );
        let v = to_view(s, at("2026-09-23T01:00:00.000Z"), Some("写周报".into()));
        assert_eq!(v.task_title.as_deref(), Some("写周报"));
    }

    // ------------------------- 格式化 -------------------------

    #[test]
    fn formats_seconds_below_an_hour() {
        assert_eq!(format_hms(0), "00:00");
        assert_eq!(format_hms(59), "00:59");
        assert_eq!(format_hms(60), "01:00");
        assert_eq!(format_hms(1500), "25:00");
    }

    #[test]
    fn formats_seconds_above_an_hour() {
        assert_eq!(format_hms(3600), "01:00:00");
        assert_eq!(format_hms(3661), "01:01:01");
        assert_eq!(format_hms(7200), "02:00:00");
    }

    #[test]
    fn formats_negative_as_zero() {
        assert_eq!(format_hms(-5), "00:00");
    }

    // ------------------------- 状态标签 -------------------------

    #[test]
    fn state_labels_cover_all_states() {
        for (s, expected) in [
            ("idle", "空闲"),
            ("running", "进行中"),
            ("paused", "已暂停"),
            ("finished", "已结束"),
            ("interrupted", "已中断"),
        ] {
            assert_eq!(state_label(s), expected);
        }
        // 未知状态原样返回，不能 panic
        assert_eq!(state_label("weird"), "weird");
    }

    // ------------------------- 常量合理性 -------------------------

    #[test]
    fn limits_are_sane() {
        assert_eq!(DEFAULT_POMODORO_MINUTES, 25, "默认番茄钟应为 25 分钟");
        const { assert!(MIN_MINUTES >= 1) };
        const { assert!(MAX_MINUTES <= 24 * 60) };
        // 中断判定系数必须大于 1，否则正常完成的会话会被误判为中断
        const { assert!(INTERRUPT_FACTOR > 1) };
    }

    /// 中断判定阈值要足够宽松：一个 25 分钟的番茄钟不应因为多跑了
    /// 几分钟就被判为"用户已离开"
    #[test]
    fn interrupt_threshold_is_lenient_enough() {
        let planned = DEFAULT_POMODORO_MINUTES * 60;
        let threshold = planned * INTERRUPT_FACTOR;
        assert!(
            threshold >= planned + 10 * 60,
            "阈值应至少比预期多出 10 分钟，避免正常超时被误判"
        );
    }
}
