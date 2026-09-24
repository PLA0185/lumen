/**
 * 前端类型定义。
 *
 * 与 Rust 侧保持一致：结构体使用 `#[serde(rename_all = "camelCase")]`，
 * 因此这里的字段名是 camelCase。凡改动 Rust 侧模型，必须同步本文件。
 *
 * 字段语义严格对齐任务书 §4.1：**plannedAt / dueAt / 提醒时间是三个不同字段**。
 */

/** 任务状态（§4.1：待办、进行中、等待、完成、归档） */
export type TaskStatus = 'todo' | 'doing' | 'waiting' | 'done' | 'archived'

/** 优先级：0 无 / 1 低 / 2 中 / 3 高（§4.1 四级） */
export type Priority = 0 | 1 | 2 | 3

/**
 * 任务的周期跨度（§4.1）。
 *
 * 用于表达「这件事这周做完就行，不用定到某一天」这类需求。
 * 与 `plannedAt` / `dueAt` 互补，三者语义不同：
 *
 * | 字段 | 含义 | 刚性 |
 * | --- | --- | --- |
 * | `periodType` | 我在这个周期内完成 | 柔性，允许没有具体日期 |
 * | `plannedAt`  | 我打算这一天的这个时刻做 | 刚性，决定日历位置 |
 * | `dueAt`      | 我必须在此之前交 | 硬约束，决定是否逾期 |
 *
 * 关键规则：period 型任务若没有 `plannedAt`，**不会出现在「今天」视图**。
 * 否则"这周做完就行"的任务会每天弹出提示，反而比不定时间更烦人。
 */
export type PeriodType = 'none' | 'day' | 'week' | 'month' | 'quarter' | 'year'

/** 周期类型的中文标签 */
export const PERIOD_LABELS: Record<PeriodType, string> = {
  none: '不限',
  day: '今日',
  week: '本周',
  month: '本月',
  quarter: '本季度',
  year: '今年',
}

/** 周期类型的短标签（用于任务卡片徽标） */
export const PERIOD_BADGES: Record<PeriodType, string> = {
  none: '',
  day: '今日内',
  week: '本周内',
  month: '本月内',
  quarter: '本季度内',
  year: '今年内',
}

/** 任务完整视图，与 Rust `models::Task` 一一对应 */
export interface Task {
  id: string
  title: string
  description: string
  noteMd: string
  linkUrl: string | null

  status: TaskStatus
  priority: number

  projectId: string | null
  categoryId: string | null

  /** 计划执行时间（UTC ISO-8601），决定在「今天」和日历中的位置 */
  plannedAt: string | null
  /** 计划时间是否含具体时刻；0 表示"仅日期"（§4.3） */
  hasPlannedTime: number
  /** 截止时间（UTC ISO-8601），用于逾期判断，可单独为空 */
  dueAt: string | null
  /** 截止时间是否含具体时刻 */
  hasDueTime: number

  estimatedMinutes: number | null
  actualMinutes: number

  completedAt: string | null
  createdAt: string
  updatedAt: string
  deletedAt: string | null

  sortOrder: number
  isPinned: number
  isFavorite: number

  /** 周期跨度：我在这个周期内完成（§4.1） */
  periodType: PeriodType

  /** 所属重复系列（§5）；为 null 表示非重复任务 */
  seriesId: string | null
  /** 原始计划发生时间，稳定身份的一部分 */
  occurrenceKey: string | null
  occurrenceIndex: number | null
  occurrenceKind: 'single' | 'generated' | 'exception' | null
  isException: number
}

/** 创建任务输入，与 Rust `CreateTaskInput` 对应 */
export interface CreateTaskInput {
  title: string
  description?: string
  noteMd?: string
  linkUrl?: string | null
  priority?: number
  projectId?: string | null
  categoryId?: string | null
  plannedAt?: string | null
  hasPlannedTime?: boolean
  dueAt?: string | null
  hasDueTime?: boolean
  estimatedMinutes?: number | null
  status?: TaskStatus
  isPinned?: boolean
  isFavorite?: boolean
  /** 周期跨度（可选） */
  periodType?: PeriodType
  tagIds?: string[]
}

/** 更新任务输入，与 Rust `UpdateTaskInput` 对应。
 *
 * 语义提醒：`undefined` 表示"不修改"，`clearXxx: true` 表示"清空"。
 * 不要把"清空日期"写成 `plannedAt: null`——那不会生效。
 */
export interface UpdateTaskInput {
  title?: string
  description?: string
  noteMd?: string
  linkUrl?: string
  status?: TaskStatus
  priority?: number
  projectId?: string | null
  categoryId?: string | null
  plannedAt?: string | null
  hasPlannedTime?: boolean
  dueAt?: string | null
  hasDueTime?: boolean
  estimatedMinutes?: number | null
  actualMinutes?: number
  isPinned?: boolean
  isFavorite?: boolean
  /** 周期跨度；传 "none" 表示取消周期 */
  periodType?: PeriodType
  clearPlannedAt?: boolean
  clearDueAt?: boolean
  clearProject?: boolean
  clearCategory?: boolean
  clearLink?: boolean
}

/** 列表查询条件（§4.1 组合筛选 + 搜索） */
export interface TaskQuery {
  statuses?: TaskStatus[]
  projectId?: string | null
  /**
   * 只看**没有归属项目**的任务。
   *
   * 别用 `projectId: null` 表达这个意思：null 经过 IPC 会变成 Rust 的 `None`，
   * 与"不限制项目"无法区分，收件箱会退化成"所有未完成任务"。
   */
  withoutProject?: boolean
  categoryId?: string | null
  tagIds?: string[]
  priorities?: number[]
  plannedFrom?: string | null
  plannedTo?: string | null
  dueFrom?: string | null
  dueTo?: string | null
  search?: string | null
  isRecurring?: boolean | null
  /** 按周期跨度筛选；周任务/月任务等视图使用 */
  periodTypes?: PeriodType[]
  overdueOnly?: boolean
  includeDeleted?: boolean
  deletedOnly?: boolean
  sortBy?: 'manual' | 'due' | 'priority' | 'created' | 'planned' | 'title'
  sortDesc?: boolean
  limit?: number
  offset?: number
  nowUtc?: string | null
}

/** 批量操作动作 */
export type BulkAction =
  | 'complete'
  | 'uncomplete'
  | 'delete'
  | 'restore'
  | 'archive'
  | 'set_priority'
  | 'move_project'
  | 'add_tag'
  | 'remove_tag'

export interface BulkActionInput {
  ids: string[]
  action: BulkAction
  priority?: number
  projectId?: string | null
  tagId?: string | null
}

/** 今日概览（托盘、悬浮窗、主界面共用同一口径） */
export interface TodayOverview {
  plannedTotal: number
  plannedDone: number
  dueTotal: number
  overdue: number
  openTotal: number
}

/** 数据目录信息（§9 数据目录查看） */
export interface DataPaths {
  dataDir: string
  dbPath: string
  backupDir: string
  schemaVersion: string
}

/** 应用健康信息 */
export interface AppInfo {
  version: string
  status: string
  taskCount: number
  trashCount: number
  remindersPaused: boolean
}

/** 软删除结果 */
export interface SoftDeleteResult {
  id: string
  deletedAt: string
  movableToTrash: boolean
}

/** 永久删除结果 */
export interface PurgeResult {
  purged: number
}

/** 后端错误码，与 Rust `error::ErrorCode` 一一对应 */
export type ErrorCode =
  | 'validation'
  | 'not_found'
  | 'conflict'
  | 'database'
  | 'io'
  | 'network'
  | 'unauthorized'
  | 'quota_exceeded'
  | 'rate_limited'
  | 'timeout'
  | 'not_configured'
  | 'internal'

/** 结构化错误对象（Rust `AppError` 的序列化形态） */
export interface BackendError {
  code: ErrorCode
  message: string
  hint: string | null
}

/** 侧边栏导航项标识（§3：主界面至少包含这些入口） */
export type ViewId =
  | 'inbox'
  | 'today'
  | 'tomorrow'
  | 'week'
  | 'all'
  | 'calendar'
  | 'board'
  | 'period-week'
  | 'period-month'
  | 'period-quarter'
  | 'period-year'
  | 'projects'
  | 'tags'
  | 'completed'
  | 'stats'
  | 'trash'
  | 'settings'
