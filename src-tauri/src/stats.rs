//! 统计、目标与成长（任务书 §7）。
//!
//! ## 一条贯穿全模块的要求：口径必须说清楚
//!
//! > 统计按日/周/月展示…**清楚标注分母、时间范围和重复实例的计数口径**。
//!
//! 因此每个统计结果都带 `scope` 字段，明确写出：
//! - 时间范围是哪个半开区间（`[start, end)`）；
//! - 完成率的分母是什么（"期间内到期的任务"还是"期间内创建的任务"）；
//! - 重复任务的实例如何计数（**按实例计**，一次发生算一项）。
//!
//! 界面上必须展示这段说明，否则"完成率 60%"可以被任何口径解释。
//!
//! ## 连续完成天数不能误导（§7 明确点名）
//!
//! > 连续完成天数不能因为没有安排任务的一天产生误导。
//!
//! 因此连续天数分两个指标：
//! - **active_streak**：连续有"完成任务"的天数；
//! - **scheduled_streak**：连续有"安排任务且全部完成"的天数。
//!
//! 关键规则：某天**没有任何安排**时，它不中断 active_streak
//! （那天本来就没有任务可完成），但也不计入。这样既不会因为周末没安排
//! 就归零，也不会把"没安排"算成"完成了"。
//! 反过来，某天有安排却一项没完成，**会**中断 scheduled_streak。

use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::commands::AppState;
use crate::db::{to_db_time, utc_now, Db};
use crate::error::{AppError, AppResult};

/// 统计区间的半开性质说明（界面直接展示）
pub const SCOPE_NOTE: &str = "统计区间为 [起始日 00:00, 结束日次日 00:00)，按本地时区计算。\
重复任务按**实例**计数——同一次重复任务的每一次发生都算一项。\
完成率的分母是「区间内计划或截止的任务数」，不含更早的历史任务。";

/// 一天的统计
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStat {
    /// 本地日期 `YYYY-MM-DD`
    pub date: String,
    /// 当天计划的任务数
    pub planned: i64,
    /// 当天完成的任务数（按 completed_at 归属）
    pub completed: i64,
    /// 当天计划且已完成
    pub planned_done: i64,
    /// 当天到期的任务数
    pub due: i64,
    /// 当天逾期未完成
    pub overdue: i64,
    /// 当天实际投入分钟数
    pub actual_minutes: i64,
    /// 当天预计分钟数
    pub estimated_minutes: i64,
}

/// 分类占比
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryShare {
    pub category_id: Option<String>,
    pub name: String,
    pub color: Option<String>,
    pub count: i64,
}

/// 项目分布
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectShare {
    pub project_id: Option<String>,
    pub name: String,
    pub color: Option<String>,
    pub count: i64,
}

/// 连续天数
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreakInfo {
    /// 连续"有完成任务"的天数
    pub active_streak: i64,
    /// 历史上最长的连续完成天数
    pub longest_active_streak: i64,
    /// 连续"有安排且全部完成"的天数
    pub perfect_streak: i64,
    /// 说明文字，直接展示给用户避免误读
    pub note: String,
}

/// 周期统计结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodStats {
    /// 区间说明
    pub scope: String,
    /// 天数
    pub days: i64,
    pub start_date: String,
    pub end_date: String,

    /// 每日明细
    pub daily: Vec<DayStat>,

    // ---------------- 汇总（全部带明确分母） ----------------
    /// 区间内计划的任务数（分母之一）
    pub planned_total: i64,
    /// 区间内到期的任务数（分母之一）
    pub due_total: i64,
    /// 区间内完成的任务数（分子）
    pub completed_total: i64,
    /// 完成率 = completed_total / planned_total（百分数，分母为 0 时为 None）
    pub completion_rate: Option<f64>,
    /// 逾期未完成数
    pub overdue_total: i64,
    /// 逾期率 = overdue_total / due_total
    pub overdue_rate: Option<f64>,

    /// 预计耗时合计（分钟）
    pub estimated_minutes: i64,
    /// 实际耗时合计（分钟）
    pub actual_minutes: i64,
    /// 估算偏差：实际/预计（无预计时为 None）。>1 表示低估了耗时
    pub estimate_ratio: Option<f64>,
    /// 平均每完成一项的实际耗时
    pub avg_minutes_per_task: Option<f64>,

    /// 分类占比
    pub by_category: Vec<CategoryShare>,
    /// 项目分布
    pub by_project: Vec<ProjectShare>,

    /// 连续天数
    pub streak: StreakInfo,

    /// 期间内创建的任务数（与 planned_total 不同，供对比）
    pub created_total: i64,
}

/// 按天聚合的结果行
#[derive(Debug, sqlx::FromRow)]
struct DayRow {
    day: String,
    completed: i64,
    actual_minutes: i64,
    estimated_minutes: i64,
}

// =============================================================================
// 时间范围
// =============================================================================

/// 计算最近 N 天的本地时间范围（含今天），返回 UTC 边界与本地日期串
fn recent_range(days: i64) -> AppResult<(String, String, String, String)> {
    use chrono::TimeZone;

    if !(1..=366).contains(&days) {
        return Err(AppError::validation(format!("统计天数超出范围：{days}"))
            .with_hint("允许 1–366 天"));
    }

    let today = chrono::Local::now().date_naive();
    let start_date = today - chrono::Duration::days(days - 1);
    let end_date = today + chrono::Duration::days(1); // 半开区间，含今天

    let local_dt = |d: chrono::NaiveDate| {
        d.and_hms_opt(0, 0, 0)
            .and_then(|ndt| chrono::Local.from_local_datetime(&ndt).single())
    };

    let s = local_dt(start_date)
        .ok_or_else(|| AppError::internal("无法构造统计起始时间"))?;
    let e = local_dt(end_date)
        .ok_or_else(|| AppError::internal("无法构造统计结束时间"))?;

    Ok((
        to_db_time(s.with_timezone(&chrono::Utc)),
        to_db_time(e.with_timezone(&chrono::Utc)),
        start_date.format("%Y-%m-%d").to_string(),
        end_date.format("%Y-%m-%d").to_string(),
    ))
}

// =============================================================================
// 采集
// =============================================================================

/// 采集最近 N 天的统计（供统计页与 AI 复盘共用，保证口径一致）。
pub async fn collect_period_stats(state: &AppState, days: i64) -> AppResult<serde_json::Value> {
    let p = period_stats(&state.db, days).await?;
    serde_json::to_value(&p).map_err(|e| AppError::internal(format!("序列化统计失败：{e}")))
}

/// 周期统计的主体实现
pub async fn period_stats(db: &Db, days: i64) -> AppResult<PeriodStats> {
    let (start_utc, end_utc, start_date, end_date) = recent_range(days)?;

    // ---------------- 按天聚合完成任务 ----------------
    // completed_at 用本地日期归属，与"今天完成了什么"的直觉一致
    let day_rows = sqlx::query_as::<_, DayRow>(
        "SELECT
            strftime('%Y-%m-%d', completed_at, 'localtime') AS day,
            COUNT(*) AS completed,
            COALESCE(SUM(actual_minutes), 0) AS actual_minutes,
            COALESCE(SUM(COALESCE(estimated_minutes, 0)), 0) AS estimated_minutes
         FROM tasks
         WHERE deleted_at IS NULL
           AND completed_at IS NOT NULL
           AND completed_at >= ?1 AND completed_at < ?2
         GROUP BY day
         ORDER BY day",
    )
    .bind(&start_utc)
    .bind(&end_utc)
    .fetch_all(db.pool())
    .await?;

    let mut done_by_day: std::collections::HashMap<String, (i64, i64, i64)> = Default::default();
    for r in day_rows {
        done_by_day.insert(r.day, (r.completed, r.actual_minutes, r.estimated_minutes));
    }

    // ---------------- 按天聚合计划与截止 ----------------
    // planned_at / due_at 也按本地日期归属（§4.2 要求界面明确"今天"的规则）
    let plan_rows = sqlx::query(
        "SELECT
            strftime('%Y-%m-%d', planned_at, 'localtime') AS day,
            COUNT(*) AS planned,
            SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END) AS planned_done
         FROM tasks
         WHERE deleted_at IS NULL
           AND status NOT IN ('archived')
           AND planned_at IS NOT NULL
           AND planned_at >= ?1 AND planned_at < ?2
         GROUP BY day",
    )
    .bind(&start_utc)
    .bind(&end_utc)
    .fetch_all(db.pool())
    .await?;

    let mut plan_by_day: std::collections::HashMap<String, (i64, i64)> = Default::default();
    for r in plan_rows {
        plan_by_day.insert(
            r.try_get("day")?,
            (r.try_get("planned")?, r.try_get("planned_done")?),
        );
    }

    let due_rows = sqlx::query(
        "SELECT
            strftime('%Y-%m-%d', due_at, 'localtime') AS day,
            COUNT(*) AS due,
            SUM(CASE WHEN status <> 'done' AND due_at < ?3 THEN 1 ELSE 0 END) AS overdue
         FROM tasks
         WHERE deleted_at IS NULL
           AND status NOT IN ('archived')
           AND due_at IS NOT NULL
           AND due_at >= ?1 AND due_at < ?2
         GROUP BY day",
    )
    .bind(&start_utc)
    .bind(&end_utc)
    .bind(to_db_time(utc_now()))
    .fetch_all(db.pool())
    .await?;

    let mut due_by_day: std::collections::HashMap<String, (i64, i64)> = Default::default();
    for r in due_rows {
        due_by_day.insert(
            r.try_get("day")?,
            (r.try_get("due")?, r.try_get("overdue")?),
        );
    }

    // ---------------- 组装每日明细（补齐没有数据的日期） ----------------
    let mut daily = Vec::with_capacity(days as usize);
    let mut cursor = chrono::NaiveDate::parse_from_str(&start_date, "%Y-%m-%d")
        .map_err(|e| AppError::internal(format!("起始日期解析失败：{e}")))?;
    let today = chrono::Local::now().date_naive();

    for _ in 0..days {
        let key = cursor.format("%Y-%m-%d").to_string();
        let (completed, actual, estimated_done) =
            done_by_day.get(&key).copied().unwrap_or((0, 0, 0));
        let (planned, planned_done) = plan_by_day.get(&key).copied().unwrap_or((0, 0));
        let (due, overdue) = due_by_day.get(&key).copied().unwrap_or((0, 0));

        // 当天计划任务的预计耗时（用于"计划投入"）
        let est: i64 = sqlx::query(
            "SELECT COALESCE(SUM(COALESCE(estimated_minutes, 0)), 0) AS m FROM tasks
             WHERE deleted_at IS NULL AND planned_at IS NOT NULL
               AND strftime('%Y-%m-%d', planned_at, 'localtime') = ?1",
        )
        .bind(&key)
        .fetch_one(db.pool())
        .await?
        .try_get("m")?;

        let _ = estimated_done;
        daily.push(DayStat {
            date: key.clone(),
            planned,
            completed,
            planned_done,
            due,
            overdue,
            actual_minutes: actual,
            estimated_minutes: est,
        });
        cursor += chrono::Duration::days(1);
    }

    // ---------------- 汇总 ----------------
    let planned_total: i64 = daily.iter().map(|d| d.planned).sum();
    let due_total: i64 = daily.iter().map(|d| d.due).sum();
    let completed_total: i64 = daily.iter().map(|d| d.completed).sum();
    let overdue_total: i64 = daily.iter().map(|d| d.overdue).sum();
    let actual_minutes: i64 = daily.iter().map(|d| d.actual_minutes).sum();
    let estimated_minutes: i64 = daily.iter().map(|d| d.estimated_minutes).sum();

    let created_total: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM tasks WHERE deleted_at IS NULL
           AND created_at >= ?1 AND created_at < ?2",
    )
    .bind(&start_utc)
    .bind(&end_utc)
    .fetch_one(db.pool())
    .await?
    .try_get("n")?;

    // 完成率分母用 planned_total；为 0 时返回 None 而不是 0%，
    // 因为"没有安排任务"与"安排了但没完成"是两件事
    let completion_rate = if planned_total > 0 {
        Some((completed_total as f64) * 100.0 / (planned_total as f64))
    } else {
        None
    };
    let overdue_rate = if due_total > 0 {
        Some((overdue_total as f64) * 100.0 / (due_total as f64))
    } else {
        None
    };
    // 估算偏差：实际/预计。样本太少时不给（容易误导）
    let estimate_ratio = if estimated_minutes > 0 && completed_total >= 3 {
        Some((actual_minutes as f64) / (estimated_minutes as f64))
    } else {
        None
    };
    let avg_minutes_per_task = if completed_total > 0 {
        Some((actual_minutes as f64) / (completed_total as f64))
    } else {
        None
    };

    // ---------------- 分类与项目分布 ----------------
    let cat_rows = sqlx::query(
        "SELECT c.id AS cid, COALESCE(c.name, '未分类') AS name, c.color AS color, COUNT(*) AS n
         FROM tasks t LEFT JOIN categories c ON c.id = t.category_id
         WHERE t.deleted_at IS NULL AND t.completed_at IS NOT NULL
           AND t.completed_at >= ?1 AND t.completed_at < ?2
         GROUP BY c.id ORDER BY n DESC LIMIT 20",
    )
    .bind(&start_utc)
    .bind(&end_utc)
    .fetch_all(db.pool())
    .await?;
    let by_category: Vec<CategoryShare> = cat_rows
        .iter()
        .map(|r| CategoryShare {
            category_id: r.try_get("cid").unwrap_or(None),
            name: r.try_get("name").unwrap_or_else(|_| "未分类".into()),
            color: r.try_get("color").unwrap_or(None),
            count: r.try_get("n").unwrap_or(0),
        })
        .collect();

    let proj_rows = sqlx::query(
        "SELECT p.id AS pid, COALESCE(p.name, '无项目') AS name, p.color AS color, COUNT(*) AS n
         FROM tasks t LEFT JOIN projects p ON p.id = t.project_id
         WHERE t.deleted_at IS NULL AND t.completed_at IS NOT NULL
           AND t.completed_at >= ?1 AND t.completed_at < ?2
         GROUP BY p.id ORDER BY n DESC LIMIT 20",
    )
    .bind(&start_utc)
    .bind(&end_utc)
    .fetch_all(db.pool())
    .await?;
    let by_project: Vec<ProjectShare> = proj_rows
        .iter()
        .map(|r| ProjectShare {
            project_id: r.try_get("pid").unwrap_or(None),
            name: r.try_get("name").unwrap_or_else(|_| "无项目".into()),
            color: r.try_get("color").unwrap_or(None),
            count: r.try_get("n").unwrap_or(0),
        })
        .collect();

    // ---------------- 连续天数 ----------------
    let streak = compute_streak(db, today).await?;

    Ok(PeriodStats {
        scope: SCOPE_NOTE.to_string(),
        days,
        start_date,
        end_date,
        daily,
        planned_total,
        due_total,
        completed_total,
        completion_rate,
        overdue_total,
        overdue_rate,
        estimated_minutes,
        actual_minutes,
        estimate_ratio,
        avg_minutes_per_task,
        by_category,
        by_project,
        streak,
        created_total,
    })
}

/// 计算连续完成天数。
///
/// 核心规则（§7 明确要求不能误导）：
/// - 从今天往前逐日检查；
/// - 某天**有完成任务** → 计入，继续往前；
/// - 某天**有安排但一项未完成** → 中断；
/// - 某天**没有任何安排** → **不中断**（那天本来就没有任务可完成），
///   但也不计入连续天数。这正是"不能因为没有安排任务的一天产生误导"的落实。
async fn compute_streak(db: &Db, today: chrono::NaiveDate) -> AppResult<StreakInfo> {
    // 只取最近两年，足够覆盖任何现实的连续记录，也避免全表扫描
    let limit_days = 730i64;
    let from = today - chrono::Duration::days(limit_days);

    let rows = sqlx::query(
        "SELECT day, completed, planned FROM (
            SELECT strftime('%Y-%m-%d', COALESCE(t.completed_at, t.planned_at), 'localtime') AS day,
                   SUM(CASE WHEN t.completed_at IS NOT NULL THEN 1 ELSE 0 END) AS completed,
                   SUM(CASE WHEN t.planned_at IS NOT NULL THEN 1 ELSE 0 END) AS planned
            FROM tasks t
            WHERE t.deleted_at IS NULL
              AND (t.completed_at IS NOT NULL OR t.planned_at IS NOT NULL)
              AND strftime('%Y-%m-%d', COALESCE(t.completed_at, t.planned_at), 'localtime') >= ?1
            GROUP BY day
         ) ORDER BY day DESC",
    )
    .bind(from.format("%Y-%m-%d").to_string())
    .fetch_all(db.pool())
    .await?;

    let mut by_day: std::collections::HashMap<String, (i64, i64)> = Default::default();
    for r in rows {
        by_day.insert(
            r.try_get("day")?,
            (r.try_get("completed")?, r.try_get("planned")?),
        );
    }

    // ---- 当前连续（active）：无安排的日子跳过不中断 ----
    let mut active = 0i64;
    let mut cursor = today;
    let mut skipped_empty = 0i64;
    for _ in 0..limit_days {
        let key = cursor.format("%Y-%m-%d").to_string();
        let (completed, planned) = by_day.get(&key).copied().unwrap_or((0, 0));

        if completed > 0 {
            active += 1;
            skipped_empty = 0;
        } else if planned > 0 {
            // 有安排却一项没完成 → 中断
            break;
        } else {
            // 完全没有安排 → 跳过，不中断。
            // 但也不能无限跳过（例如用户半年没打开过），
            // 连续跳过超过 30 天后就停止，避免算出一个虚高的数字。
            skipped_empty += 1;
            if skipped_empty > 30 {
                break;
            }
        }
        cursor -= chrono::Duration::days(1);
    }

    // ---- 历史最长连续 ----
    let mut longest = 0i64;
    let mut run = 0i64;
    let mut cursor2 = today;
    for _ in 0..limit_days {
        let key = cursor2.format("%Y-%m-%d").to_string();
        let (completed, planned) = by_day.get(&key).copied().unwrap_or((0, 0));
        if completed > 0 {
            run += 1;
            longest = longest.max(run);
        } else if planned > 0 {
            run = 0;
        }
        // 无安排的日子同样跳过
        cursor2 -= chrono::Duration::days(1);
    }

    // ---- 完美连续：有安排且全部完成 ----
    let mut perfect = 0i64;
    let mut cursor3 = today;
    for _ in 0..limit_days {
        let key = cursor3.format("%Y-%m-%d").to_string();
        match by_day.get(&key).copied() {
            Some((c, p)) if p > 0 && c >= p => perfect += 1,
            // 今天还没结束且已有完成，不应算中断
            Some((c, p)) if p > 0 && c < p && cursor3 == today && c > 0 => {
                let _ = (c, p);
                break;
            }
            Some((_, p)) if p > 0 => break,
            // 无安排的日子不影响"完美连续"的含义，但也不计入
            _ => {}
        }
        cursor3 -= chrono::Duration::days(1);
    }

    Ok(StreakInfo {
        active_streak: active,
        longest_active_streak: longest.max(active),
        perfect_streak: perfect,
        note: "「连续完成」只统计有完成任务的日子；**完全没有安排任务的日子不会中断它**\
               （那天本来就没有任务可完成），但也不会被计入天数。\
               「完美连续」要求当天有安排且全部完成，漏做一项即中断。"
            .to_string(),
    })
}

// =============================================================================
// 目标与游戏化（§7）
// =============================================================================

/// 成长设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrowthConfig {
    /// 是否启用经验值/等级/成就（默认关闭，§7 要求"允许完全关闭"）
    pub gamification_enabled: bool,
    /// 每天完成多少项算达标
    pub daily_goal: i64,
    /// 每周完成多少项算达标
    pub weekly_goal: i64,
    /// 是否在界面上显示连续天数
    pub show_streak: bool,
}

impl Default for GrowthConfig {
    fn default() -> Self {
        Self {
            // 默认关闭：游戏化是可选反馈，不该强加给所有用户（§7）
            gamification_enabled: false,
            daily_goal: 3,
            weekly_goal: 15,
            show_streak: true,
        }
    }
}

impl GrowthConfig {
    fn normalize(&mut self) {
        self.daily_goal = self.daily_goal.clamp(1, 100);
        self.weekly_goal = self.weekly_goal.clamp(1, 500);
    }
}

/// 等级信息
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelInfo {
    pub level: i64,
    pub title: String,
    /// 当前等级内的经验
    pub xp_in_level: i64,
    /// 升到下一级所需经验
    pub xp_for_next: i64,
    /// 总经验
    pub total_xp: i64,
    /// 进度百分比 0–100
    pub progress: i64,
}

/// 成就
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Achievement {
    pub id: String,
    pub name: String,
    pub description: String,
    pub achieved: bool,
    /// 进度（当前/目标）
    pub progress: i64,
    pub target: i64,
}

/// 成长总览
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrowthOverview {
    pub enabled: bool,
    pub level: LevelInfo,
    pub achievements: Vec<Achievement>,
    /// 今日完成数 / 目标
    pub today_done: i64,
    pub daily_goal: i64,
    /// 本周完成数 / 目标
    pub week_done: i64,
    pub weekly_goal: i64,
}

/// 每完成一项任务获得的经验。刻意用固定值：
/// 按耗时给经验会诱导用户"虚报工时"，那样统计就失去意义了。
const XP_PER_TASK: i64 = 10;
/// 完成一项高优先级任务的额外奖励
const XP_HIGH_PRIORITY_BONUS: i64 = 5;

/// 根据总经验算等级：每级所需经验递增，避免后期升级过快失去反馈感
fn level_of(total_xp: i64) -> LevelInfo {
    // 第 n 级所需累计经验 = 50 * n * (n+1) / 2 —— 二次增长
    let mut level = 1i64;
    let mut need = 0i64;
    loop {
        let next_need = 50 * level * (level + 1) / 2;
        if total_xp < next_need {
            let prev = need;
            let in_level = total_xp - prev;
            let span = (next_need - prev).max(1);
            return LevelInfo {
                level,
                title: level_title(level),
                xp_in_level: in_level,
                xp_for_next: next_need - prev,
                total_xp,
                progress: ((in_level * 100) / span).clamp(0, 100),
            };
        }
        need = next_need;
        level += 1;
        if level > 999 {
            break;
        }
    }
    LevelInfo {
        level,
        title: level_title(level),
        xp_in_level: total_xp,
        xp_for_next: 1,
        total_xp,
        progress: 100,
    }
}

/// 等级称号（纯本地文案，不做社交比较）
fn level_title(level: i64) -> String {
    match level {
        1..=2 => "起步",
        3..=5 => "渐入",
        6..=10 => "稳定",
        11..=20 => "熟练",
        21..=35 => "高效",
        36..=60 => "精进",
        _ => "长期主义",
    }
    .to_string()
}

/// 计算成长总览
pub async fn growth_overview(db: &Db) -> AppResult<GrowthOverview> {
    let mut cfg = load_growth_config(db).await?;
    cfg.normalize();

    let completed: i64 =
        sqlx::query("SELECT COUNT(*) AS n FROM tasks WHERE deleted_at IS NULL AND status = 'done'")
            .fetch_one(db.pool())
            .await?
            .try_get("n")?;

    let high_done: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM tasks WHERE deleted_at IS NULL AND status = 'done' AND priority = 3",
    )
    .fetch_one(db.pool())
    .await?
    .try_get("n")?;

    let total_xp = completed * XP_PER_TASK + high_done * XP_HIGH_PRIORITY_BONUS;

    // 今日与本周完成数
    let today = chrono::Local::now().date_naive();
    let today_str = today.format("%Y-%m-%d").to_string();
    let today_done: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM tasks
         WHERE deleted_at IS NULL AND completed_at IS NOT NULL
           AND strftime('%Y-%m-%d', completed_at, 'localtime') = ?1",
    )
    .bind(&today_str)
    .fetch_one(db.pool())
    .await?
    .try_get("n")?;

    let weekday_from_mon = today.format("%u").to_string().parse::<i64>().unwrap_or(1);
    let monday = today - chrono::Duration::days(weekday_from_mon - 1);
    let week_done: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM tasks
         WHERE deleted_at IS NULL AND completed_at IS NOT NULL
           AND strftime('%Y-%m-%d', completed_at, 'localtime') >= ?1",
    )
    .bind(monday.format("%Y-%m-%d").to_string())
    .fetch_one(db.pool())
    .await?
    .try_get("n")?;

    // 成就：全部基于真实数据，没有"点一下就能拿到"的空成就
    let longest = compute_streak(db, today).await?.longest_active_streak;
    let with_estimate: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM tasks
         WHERE deleted_at IS NULL AND status = 'done' AND actual_minutes > 0",
    )
    .fetch_one(db.pool())
    .await?
    .try_get("n")?;

    let achievements = vec![
        Achievement {
            id: "first_task".into(),
            name: "第一步".into(),
            description: "完成第一个任务".into(),
            achieved: completed >= 1,
            progress: completed.min(1),
            target: 1,
        },
        Achievement {
            id: "ten_tasks".into(),
            name: "渐入佳境".into(),
            description: "累计完成 10 个任务".into(),
            achieved: completed >= 10,
            progress: completed.min(10),
            target: 10,
        },
        Achievement {
            id: "hundred_tasks".into(),
            name: "百项达成".into(),
            description: "累计完成 100 个任务".into(),
            achieved: completed >= 100,
            progress: completed.min(100),
            target: 100,
        },
        Achievement {
            id: "streak_7".into(),
            name: "一周不断".into(),
            description: "连续 7 天有完成任务".into(),
            achieved: longest >= 7,
            progress: longest.min(7),
            target: 7,
        },
        Achievement {
            id: "streak_30".into(),
            name: "一月坚持".into(),
            description: "连续 30 天有完成任务".into(),
            achieved: longest >= 30,
            progress: longest.min(30),
            target: 30,
        },
        Achievement {
            id: "time_tracking".into(),
            name: "记录习惯".into(),
            description: "为 20 个已完成任务记录了实际耗时".into(),
            achieved: with_estimate >= 20,
            progress: with_estimate.min(20),
            target: 20,
        },
    ];

    Ok(GrowthOverview {
        enabled: cfg.gamification_enabled,
        level: level_of(total_xp),
        achievements,
        today_done,
        daily_goal: cfg.daily_goal,
        week_done,
        weekly_goal: cfg.weekly_goal,
    })
}

/// 读取成长配置
async fn load_growth_config(db: &Db) -> AppResult<GrowthConfig> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value_json FROM settings WHERE key = 'growth_config'")
            .fetch_optional(db.pool())
            .await?;
    Ok(row
        .and_then(|(j,)| serde_json::from_str::<GrowthConfig>(&j).ok())
        .unwrap_or_default())
}

// =============================================================================
// IPC 命令
// =============================================================================

/// 周期统计（供统计页）
#[tauri::command]
pub async fn stats_period(
    state: State<'_, AppState>,
    days: i64,
) -> AppResult<PeriodStats> {
    period_stats(&state.db, days).await
}

/// 成长总览
#[tauri::command]
pub async fn stats_growth(state: State<'_, AppState>) -> AppResult<GrowthOverview> {
    growth_overview(&state.db).await
}

/// 读取成长配置
#[tauri::command]
pub async fn growth_get_config(
    state: State<'_, AppState>,
) -> AppResult<GrowthConfig> {
    let mut cfg = load_growth_config(&state.db).await?;
    cfg.normalize();
    Ok(cfg)
}

/// 保存成长配置
#[tauri::command]
pub async fn growth_set_config(
    state: State<'_, AppState>,
    config: GrowthConfig,
) -> AppResult<GrowthConfig> {
    let mut cfg = config;
    cfg.normalize();
    let json = serde_json::to_string(&cfg)
        .map_err(|e| AppError::internal(format!("序列化成长配置失败：{e}")))?;
    let now = to_db_time(utc_now());
    sqlx::query(
        "INSERT INTO settings (key, value_json, updated_at) VALUES ('growth_config', ?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
    )
    .bind(&json)
    .bind(&now)
    .execute(state.db.pool())
    .await?;
    Ok(cfg)
}

/// 个人目标（可手动或按任务关联）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub id: String,
    pub title: String,
    /// 目标关联的任务数（按已完成任务数计算进度）
    pub target_count: i64,
    /// 截止日期 `YYYY-MM-DD`，可为空
    pub due_date: Option<String>,
    pub created_at: String,
}

/// 目标进度
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalProgress {
    #[serde(flatten)]
    pub goal: Goal,
    /// 已完成数（目标创建后完成的任务数）
    pub done_count: i64,
    pub percent: i64,
    /// 是否已达成
    pub achieved: bool,
}

/// 列出目标及进度
#[tauri::command]
pub async fn goals_list(state: State<'_, AppState>) -> AppResult<Vec<GoalProgress>> {
    let rows: Vec<(String, String, i64, Option<String>, String)> = sqlx::query_as(
        "SELECT id, title, target_count, due_date, created_at FROM goals ORDER BY created_at DESC",
    )
    .fetch_all(state.db.pool())
    .await
    .unwrap_or_default();

    let mut out = Vec::with_capacity(rows.len());
    for (id, title, target, due, created) in rows {
        // 目标进度 = 目标创建之后完成的任务数。
        // 用"创建之后"而不是"全部历史"，否则老用户新建目标会立刻显示 100%。
        let done: i64 = sqlx::query(
            "SELECT COUNT(*) AS n FROM tasks
             WHERE deleted_at IS NULL AND completed_at IS NOT NULL AND completed_at >= ?1",
        )
        .bind(&created)
        .fetch_one(state.db.pool())
        .await?
        .try_get("n")?;

        let percent = if target > 0 {
            ((done * 100) / target).clamp(0, 100)
        } else {
            0
        };
        out.push(GoalProgress {
            goal: Goal {
                id,
                title,
                target_count: target,
                due_date: due,
                created_at: created,
            },
            done_count: done,
            percent,
            achieved: target > 0 && done >= target,
        });
    }
    Ok(out)
}

/// 创建目标
#[tauri::command]
pub async fn goal_create(
    state: State<'_, AppState>,
    title: String,
    target_count: i64,
    due_date: Option<String>,
) -> AppResult<Goal> {
    let t = title.trim();
    if t.is_empty() {
        return Err(AppError::validation("目标名称不能为空"));
    }
    if target_count <= 0 || target_count > 100_000 {
        return Err(AppError::validation("目标数量应在 1–100000 之间"));
    }
    if let Some(d) = due_date.as_deref().filter(|s| !s.trim().is_empty()) {
        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|_| {
            AppError::validation(format!("日期格式不正确：{d}")).with_hint("应为 YYYY-MM-DD")
        })?;
    }

    let id = uuid::Uuid::now_v7().to_string();
    let now = to_db_time(utc_now());
    sqlx::query(
        "INSERT INTO goals (id, title, target_count, due_date, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(&id)
    .bind(t)
    .bind(target_count)
    .bind(due_date.as_deref().filter(|s| !s.trim().is_empty()))
    .bind(&now)
    .execute(state.db.pool())
    .await?;

    Ok(Goal {
        id,
        title: t.to_string(),
        target_count,
        due_date,
        created_at: now,
    })
}

/// 删除目标
#[tauri::command]
pub async fn goal_delete(state: State<'_, AppState>, id: String) -> AppResult<bool> {
    let n = sqlx::query("DELETE FROM goals WHERE id = ?1")
        .bind(&id)
        .execute(state.db.pool())
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::not_found("目标", &id));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------- 等级计算 -------------------------

    #[test]
    fn level_starts_at_one_with_zero_xp() {
        let l = level_of(0);
        assert_eq!(l.level, 1);
        assert_eq!(l.xp_in_level, 0);
        assert_eq!(l.progress, 0);
        assert!(l.xp_for_next > 0, "1 级也应显示到下一级的距离");
    }

    /// 经验增加必须单调提升等级，不能出现倒退或跳变异常
    #[test]
    fn level_is_monotonic_in_xp() {
        let mut prev = 0i64;
        let mut last_level = 0i64;
        for xp in (0..20000).step_by(37) {
            let l = level_of(xp);
            assert!(l.total_xp >= prev);
            assert!(
                l.level >= last_level,
                "经验 {xp} 时等级 {level} 低于之前的 {last_level}",
                level = l.level
            );
            last_level = l.level;
            prev = l.total_xp;
            // 进度必须落在 0–100
            assert!((0..=100).contains(&l.progress), "进度越界：{}", l.progress);
        }
    }

    #[test]
    fn level_progress_advances_within_level() {
        // 取 1 级区间内的两个点，后面的进度应更大
        let a = level_of(10);
        let b = level_of(40);
        assert_eq!(a.level, b.level, "两点应仍在同一级");
        assert!(b.progress > a.progress, "{} 应大于 {}", b.progress, a.progress);
    }

    #[test]
    fn level_title_is_never_empty() {
        for lv in [1, 3, 8, 15, 30, 50, 200] {
            let info = level_of(50 * lv * (lv + 1) / 2);
            assert!(!info.title.is_empty());
        }
    }

    // ------------------------- 成长配置 -------------------------

    /// §7 要求游戏化"允许完全关闭"，因此默认必须是关闭的
    #[test]
    fn gamification_is_off_by_default() {
        let c = GrowthConfig::default();
        assert!(
            !c.gamification_enabled,
            "游戏化默认必须关闭——它是可选反馈，不该强加给所有用户"
        );
        assert!(c.show_streak, "连续天数属于中性信息，默认可以显示");
    }

    #[test]
    fn growth_config_normalizes_out_of_range_goals() {
        let mut c = GrowthConfig {
            daily_goal: 0,
            weekly_goal: -5,
            ..Default::default()
        };
        c.normalize();
        assert!(c.daily_goal >= 1);
        assert!(c.weekly_goal >= 1);

        let mut c2 = GrowthConfig {
            daily_goal: 9999,
            weekly_goal: 99999,
            ..Default::default()
        };
        c2.normalize();
        assert!(c2.daily_goal <= 100);
        assert!(c2.weekly_goal <= 500);
    }

    // ------------------------- 区间计算 -------------------------

    #[test]
    fn recent_range_is_half_open_and_covers_today() {
        let (s, e, sd, ed) = recent_range(7).unwrap();
        assert!(s < e, "起始必须早于结束");
        assert!(s.ends_with('Z') && e.ends_with('Z'));
        assert_eq!(s.len(), 24);
        assert_eq!(e.len(), 24);
        // 结束日期是"明天"，因为区间是半开的，要包含今天
        assert!(sd < ed);
    }

    #[test]
    fn recent_range_rejects_invalid_days() {
        assert!(recent_range(0).is_err());
        assert!(recent_range(-1).is_err());
        assert!(recent_range(9999).is_err());
        assert!(recent_range(1).is_ok());
        assert!(recent_range(366).is_ok());
        assert!(recent_range(367).is_err());
    }

    #[test]
    fn recent_range_for_one_day_is_today_only() {
        let (s, e, sd, _ed) = recent_range(1).unwrap();
        assert!(s < e);
        // 起始日期就是今天（1 天区间 = 今天）
        let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
        assert_eq!(sd, today);
    }

    // ------------------------- 口径说明 -------------------------

    /// §7 要求"清楚标注分母、时间范围和重复实例的计数口径"，
    /// 因此这段说明必须覆盖这三件事。
    #[test]
    fn scope_note_covers_denominator_range_and_recurrence() {
        assert!(SCOPE_NOTE.contains("分母"), "必须说明分母");
        assert!(
            SCOPE_NOTE.contains("区间") || SCOPE_NOTE.contains("时间范围"),
            "必须说明时间范围"
        );
        assert!(SCOPE_NOTE.contains("实例"), "必须说明重复任务的计数口径");
        assert!(SCOPE_NOTE.contains("本地时区"), "必须说明时区口径");
    }

    // ------------------------- 经验值规则 -------------------------

    /// 经验值不应与耗时挂钩：那会诱导用户虚报工时，使统计失真
    #[test]
    fn xp_is_fixed_per_task_not_time_based() {
        assert_eq!(XP_PER_TASK, 10);
        assert!(XP_HIGH_PRIORITY_BONUS > 0);
        // 高优先级奖励应当显著小于基础值，避免用户只刷高优先级
        assert!(
            XP_HIGH_PRIORITY_BONUS < XP_PER_TASK,
            "高优先级奖励不应超过基础经验，否则会诱导用户把所有任务标成高优先级"
        );
    }
}
