//! 任务相关数据模型与 IPC 输入/输出类型。
//!
//! 命名与语义严格对齐任务书 §4.1：
//! **计划时间（planned_at）、截止时间（due_at）、提醒时间（reminders.remind_at）
//! 是三个不同字段**，任一可单独为空。前端、筛选器、日历、统计统一使用这里的语义。

use serde::{Deserialize, Serialize};

/// 任务状态（§4.1：至少有待办、进行中、等待、完成、归档）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// 待办
    Todo,
    /// 进行中
    Doing,
    /// 等待
    Waiting,
    /// 已完成
    Done,
    /// 已归档
    Archived,
}

impl TaskStatus {
    /// 数据库存储值
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Doing => "doing",
            Self::Waiting => "waiting",
            Self::Done => "done",
            Self::Archived => "archived",
        }
    }

    /// 从数据库值解析；未知值回落到 Todo 并可在日志中体现
    pub fn from_db(s: &str) -> Self {
        match s {
            "doing" => Self::Doing,
            "waiting" => Self::Waiting,
            "done" => Self::Done,
            "archived" => Self::Archived,
            _ => Self::Todo,
        }
    }

    /// 是否属于「未完成」集合，用于今日计数、逾期判断与托盘菜单显示
    pub fn is_open(self) -> bool {
        matches!(self, Self::Todo | Self::Doing | Self::Waiting)
    }
}

/// 任务完整视图（前端直接使用）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: String,
    pub note_md: String,
    pub link_url: Option<String>,

    pub status: String,
    pub priority: i64,

    pub project_id: Option<String>,
    pub category_id: Option<String>,

    /// 计划执行时间（UTC ISO-8601）
    pub planned_at: Option<String>,
    pub has_planned_time: i64,
    /// 截止时间（UTC ISO-8601），可单独为空
    pub due_at: Option<String>,
    pub has_due_time: i64,

    pub estimated_minutes: Option<i64>,
    pub actual_minutes: i64,

    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,

    pub sort_order: f64,
    pub is_pinned: i64,
    pub is_favorite: i64,

    /// 周期跨度：我在这个周期内完成（none/day/week/month/quarter/year）。
    /// 与 planned_at（哪一天做）、due_at（何时必须交）互补（§4.1）。
    pub period_type: String,

    pub series_id: Option<String>,
    pub occurrence_key: Option<String>,
    pub occurrence_index: Option<i64>,
    pub occurrence_kind: Option<String>,
    pub is_exception: i64,
}

impl Task {
    /// 状态枚举视图
    pub fn status_enum(&self) -> TaskStatus {
        TaskStatus::from_db(&self.status)
    }

    /// 是否已完成（用于完成率与进度显示）
    pub fn is_done(&self) -> bool {
        self.status_enum() == TaskStatus::Done
    }

    /// 是否逾期：有截止时间、未完成、且截止时间早于 now。
    ///
    /// 注意：仅日期型截止时间（has_due_time = 0）按"当天结束"判断，
    /// 不能把全天任务默认当作凌晨到期（§4.3）。这里由调用方传入
    /// `end_of_day_utc` 处理，避免在数据层做时区假设。
    pub fn is_overdue(&self, now_utc: &str, due_utc_for_compare: Option<&str>) -> bool {
        if self.is_done() || self.status_enum() == TaskStatus::Archived {
            return false;
        }
        let Some(due) = due_utc_for_compare.or(self.due_at.as_deref()) else {
            return false;
        };
        due < now_utc
    }
}

/// 创建任务的输入。
///
/// 所有可选字段都真正可选：不传即表示"该维度为空"，
/// 与 §4.1「任一字段都可以单独为空」一致。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskInput {
    /// 标题（必填，1–500 字符）
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub note_md: Option<String>,
    #[serde(default)]
    pub link_url: Option<String>,

    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub category_id: Option<String>,

    /// 计划执行时间（UTC ISO-8601）
    #[serde(default)]
    pub planned_at: Option<String>,
    /// 计划时间是否包含具体时刻（false = 仅日期）
    #[serde(default)]
    pub has_planned_time: Option<bool>,
    /// 截止时间（UTC ISO-8601）
    #[serde(default)]
    pub due_at: Option<String>,
    /// 截止时间是否包含具体时刻（false = 仅日期）
    #[serde(default)]
    pub has_due_time: Option<bool>,

    #[serde(default)]
    pub estimated_minutes: Option<i64>,
    #[serde(default = "default_status")]
    pub status: String,

    #[serde(default)]
    pub is_pinned: Option<bool>,
    #[serde(default)]
    pub is_favorite: Option<bool>,

    /// 周期跨度（可选）：none/day/week/month/quarter/year
    #[serde(default)]
    pub period_type: Option<String>,

    /// 标签 ID 列表
    #[serde(default)]
    pub tag_ids: Vec<String>,
}

fn default_status() -> String {
    "todo".to_string()
}

/// 更新任务的输入（部分更新：None 表示"不修改该字段"）。
///
/// 与创建的区别在于语义：这里 `None` 是"保持不变"，
/// 而"清空某字段"必须用 `clear_*` 显式表达，避免误清空用户数据。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskInput {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub note_md: Option<String>,
    #[serde(default)]
    pub link_url: Option<String>,

    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub category_id: Option<String>,

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
    pub actual_minutes: Option<i64>,

    #[serde(default)]
    pub is_pinned: Option<bool>,
    #[serde(default)]
    pub is_favorite: Option<bool>,
    /// 周期跨度（可选）：none/day/week/month/quarter/year
    #[serde(default)]
    pub period_type: Option<String>,

    /// 显式清空（对应 UI 上的"移除日期"操作）
    #[serde(default)]
    pub clear_planned_at: bool,
    #[serde(default)]
    pub clear_due_at: bool,
    #[serde(default)]
    pub clear_project: bool,
    #[serde(default)]
    pub clear_category: bool,
    #[serde(default)]
    pub clear_link: bool,
}

/// 任务列表查询条件（§4.1 组合筛选 + 搜索）
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQuery {
    /// 状态集合；空表示"全部未归档"
    #[serde(default)]
    pub statuses: Vec<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    /// 只看**没有归属项目**的任务（`project_id IS NULL`）。
    ///
    /// 为什么需要单独一个布尔字段：`projectId: null` 经过 IPC 会反序列化成
    /// `None`，与"压根没传这个条件"完全一样，于是
    /// 「收件箱 = 没有项目的未完成任务」会退化成「所有未完成任务」。
    /// 用一个显式字段把三种语义彻底分开：
    ///
    /// | 想要的效果 | 传法 |
    /// | --- | --- |
    /// | 不限项目 | 两个都不传 |
    /// | 只查没有项目的 | `withoutProject = true` |
    /// | 查指定项目 | `projectId = "..."` |
    ///
    /// `without_project` 优先于 `project_id`：同时传是调用方的错，
    /// 但后端必须给出确定行为，而不是取决于判断顺序。
    #[serde(default)]
    pub without_project: bool,
    #[serde(default)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub tag_ids: Vec<String>,
    #[serde(default)]
    pub priorities: Vec<i64>,
    /// 计划时间区间（UTC，含端点）
    #[serde(default)]
    pub planned_from: Option<String>,
    #[serde(default)]
    pub planned_to: Option<String>,
    /// 截止时间区间（UTC，含端点）
    #[serde(default)]
    pub due_from: Option<String>,
    #[serde(default)]
    pub due_to: Option<String>,
    /// 全文搜索词（标题/描述/Markdown 备注/链接）
    #[serde(default)]
    pub search: Option<String>,
    /// 是否只看重复任务：None = 不限，Some(true) = 只看重复，Some(false) = 只看非重复
    #[serde(default)]
    pub is_recurring: Option<bool>,
    /// 按周期跨度筛选（空表示不限）；周任务/月任务视图使用
    #[serde(default)]
    pub period_types: Vec<String>,
    /// 是否只看已逾期
    #[serde(default)]
    pub overdue_only: bool,
    /// 是否包含已删除（回收站视图）
    #[serde(default)]
    pub include_deleted: bool,
    /// 仅回收站
    #[serde(default)]
    pub deleted_only: bool,
    /// 排序：manual | due | priority | created | planned | title
    #[serde(default)]
    pub sort_by: Option<String>,
    #[serde(default)]
    pub sort_desc: Option<bool>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
    /// 参考"当前时刻"，用于逾期判断；缺省取服务器当前时间
    #[serde(default)]
    pub now_utc: Option<String>,
}

/// 批量操作请求（§4.1 批量操作）
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkActionInput {
    pub ids: Vec<String>,
    /// complete | uncomplete | delete | restore | archive | set_priority | move_project | add_tag | remove_tag
    pub action: String,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub tag_id: Option<String>,
}

/// 回收站清空结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurgeResult {
    /// 被永久删除的任务数
    pub purged: i64,
}

/// 符合条件的任务总数（整改任务书 §10）。
///
/// 与 `task_list` 共用同一套筛选条件，因此它就是"这个视图里一共有多少条"，
/// 与"当前已经加载了多少条"是两件事——分页与破坏性操作确认都必须看它。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCount {
    pub total: i64,
}

/// 删除操作的执行结果，便于 UI 提示"可在回收站恢复"
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoftDeleteResult {
    pub id: String,
    pub deleted_at: String,
    /// 是否已移入回收站（始终为 true；保留字段以明确语义）
    pub movable_to_trash: bool,
}

/// 今日概览（托盘菜单与悬浮窗使用同一份数据，避免两处口径不一致）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayOverview {
    /// 今日计划的任务总数
    pub planned_total: i64,
    /// 今日计划且已完成
    pub planned_done: i64,
    /// 今日截止的任务数
    pub due_total: i64,
    /// 逾期任务数（截止时间早于现在且未完成）
    pub overdue: i64,
    /// 今日未完成数（托盘菜单展示用）
    pub open_total: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_roundtrip_covers_all_variants() {
        for s in [
            TaskStatus::Todo,
            TaskStatus::Doing,
            TaskStatus::Waiting,
            TaskStatus::Done,
            TaskStatus::Archived,
        ] {
            assert_eq!(TaskStatus::from_db(s.as_db()), s);
        }
        // 未知值必须安全回落，不能 panic（避免旧数据导致启动崩溃）
        assert_eq!(TaskStatus::from_db("weird"), TaskStatus::Todo);
    }

    #[test]
    fn only_open_statuses_count_as_incomplete() {
        assert!(TaskStatus::Todo.is_open());
        assert!(TaskStatus::Doing.is_open());
        assert!(TaskStatus::Waiting.is_open());
        assert!(!TaskStatus::Done.is_open());
        assert!(!TaskStatus::Archived.is_open());
    }

    /// §4.1：截止时间可为空；为空时永远不算逾期。
    #[test]
    fn task_without_due_date_is_never_overdue() {
        let t = Task {
            id: "1".into(),
            title: "无截止".into(),
            description: String::new(),
            note_md: String::new(),
            link_url: None,
            status: "todo".into(),
            priority: 0,
            project_id: None,
            category_id: None,
            planned_at: None,
            has_planned_time: 0,
            due_at: None,
            has_due_time: 0,
            estimated_minutes: None,
            actual_minutes: 0,
            completed_at: None,
            created_at: "2026-01-01T00:00:00.000Z".into(),
            updated_at: "2026-01-01T00:00:00.000Z".into(),
            deleted_at: None,
            sort_order: 0.0,
            is_pinned: 0,
            is_favorite: 0,
            period_type: "none".into(),
            series_id: None,
            occurrence_key: None,
            occurrence_index: None,
            occurrence_kind: None,
            is_exception: 0,
        };
        assert!(!t.is_overdue("2030-01-01T00:00:00.000Z", None));
    }

    /// 已完成任务不算逾期，即使截止时间早已过去。
    #[test]
    fn completed_task_is_not_overdue() {
        let mut t = Task {
            id: "1".into(),
            title: "x".into(),
            description: String::new(),
            note_md: String::new(),
            link_url: None,
            status: "done".into(),
            priority: 0,
            project_id: None,
            category_id: None,
            planned_at: None,
            has_planned_time: 0,
            due_at: Some("2020-01-01T00:00:00.000Z".into()),
            has_due_time: 1,
            estimated_minutes: None,
            actual_minutes: 0,
            completed_at: Some("2020-01-02T00:00:00.000Z".into()),
            created_at: "2020-01-01T00:00:00.000Z".into(),
            updated_at: "2020-01-01T00:00:00.000Z".into(),
            deleted_at: None,
            sort_order: 0.0,
            is_pinned: 0,
            is_favorite: 0,
            period_type: "none".into(),
            series_id: None,
            occurrence_key: None,
            occurrence_index: None,
            occurrence_kind: None,
            is_exception: 0,
        };
        assert!(!t.is_overdue("2026-01-01T00:00:00.000Z", None));
        // 回到未完成状态后应重新算逾期
        t.status = "todo".into();
        assert!(t.is_overdue("2026-01-01T00:00:00.000Z", None));
    }
}
