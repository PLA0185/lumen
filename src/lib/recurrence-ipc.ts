/**
 * 重复任务 IPC 封装（任务书 §5）。
 */

import { invokeData as invoke } from './data-change'
import { IpcError } from './ipc'
import type { ErrorCode, Task } from './types'

/** 频率 */
export type Freq = 'daily' | 'weekly' | 'monthly' | 'yearly'

/** 结束条件 */
export type EndCondition =
  | { kind: 'never' }
  | { kind: 'until'; date: string }
  | { kind: 'count'; count: number }

/** 每月第 N 个星期 X */
export interface SetPos {
  /** 1–5 表示第几个，-1 表示最后一个 */
  nth: number
  /** 1=周一 … 7=周日 */
  weekday: number
}

/** 创建重复任务输入 */
export interface CreateRecurringInput {
  title: string
  description?: string
  priority?: number
  projectId?: string | null
  categoryId?: string | null
  estimatedMinutes?: number | null
  tagIds?: string[]
  rrule: string
  tzid?: string
  dtstartLocal: string
  hasStartTime?: boolean
  dueLocal?: string | null
  materializeDays?: number
}

export interface CreateRecurringResult {
  seriesId: string
  createdCount: number
  description: string
  /** 边界策略说明（涉及月末等情形时存在） */
  edgeNote: string | null
  warning: string | null
  needsRepair: boolean
}

/** 范围语义（§5 三选一） */
export type EditScope = 'this_only' | 'this_and_future' | 'whole_series'

/** 删除范围 */
export type DeleteMode = 'this_only' | 'this_and_future' | 'whole_series'

/** 单次编辑的字段补丁 */
export interface InstancePatch {
  tzid?: string
  title?: string
  description?: string
  noteMd?: string
  linkUrl?: string
  projectId?: string
  categoryId?: string
  tagIds?: string[]
  periodType?: string
  priority?: number
  plannedAt?: string
  hasPlannedTime?: boolean
  dueAt?: string
  hasDueTime?: boolean
  estimatedMinutes?: number
  clearPlannedAt?: boolean
  clearDueAt?: boolean
  clearLink?: boolean
  clearProject?: boolean
  clearCategory?: boolean
  clearEstimatedMinutes?: boolean
}

export interface ScopeActionResult {
  affected: number
  affectedHistory: number
  regenerated: number
  message: string
}

export interface DeleteResult {
  affected: number
  affectedHistory: number
  message: string
}

/** 系列实例统计 */
export interface SeriesStats {
  seriesId: string
  totalInstances: number
  completed: number
  exceptions: number
  skipped: number
  segments: number
}

/** 范围信息（界面据此禁用不适用的选项并说明原因） */
export interface ScopeInfo {
  isRecurring: boolean
  reason?: string
  seriesId?: string
  occurrenceKey?: string | null
  occurrenceIndex?: number | null
  isException?: boolean
  totalInstances?: number
  completed?: number
  exceptions?: number
  segments?: number
  /** 该次之前已完成的历史数量（"整个系列"会影响它） */
  completedBefore?: number
  availableScopes?: { thisOnly: boolean; thisAndFuture: boolean; wholeSeries: boolean }
  notes?: { thisOnly: string; thisAndFuture: string; wholeSeries: string }
}

/** 系列详情 */
export interface SeriesDetail {
  series: {
    id: string
    rrule: string
    tzid: string
    dtstartLocal: string
    hasStartTime: number
    recurrenceEndKind: string
    recurrenceUntil: string | null
    recurrenceCount: number | null
    terminatedFromOccurrenceKey: string | null
    ruleVersion: number
  }
  description: string
  edgeNote: string | null
  nextFutureOccurrenceKey: string | null
  segments: Array<{
    id: string
    seriesId: string
    ruleVersion: number
    effectiveFromOccurrence: string
    newRrule: string | null
    newTzid: string | null
    overrideTitle: string | null
    overridePriority: number | null
  }>
  skippedOccurrences: string[]
}

export const REC_CMD = {
  create: 'recurring_create',
  preview: 'recurring_preview',
  materialize: 'recurring_materialize',
  ensureRange: 'recurring_ensure_range',
  get: 'recurring_get',
  occurrences: 'recurring_occurrences',
  stats: 'recurring_stats',
  editInstance: 'recurring_edit_instance',
  skipOccurrence: 'recurring_skip_occurrence',
  delete: 'recurring_delete',
  scopeInfo: 'recurring_scope_info',
} as const

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
    throw new IpcError('internal', '当前不在 Lumen 桌面程序内运行', null, null)
  }
  try {
    return await invoke<T>(cmd, args)
  } catch (e) {
    if (e && typeof e === 'object' && 'message' in e && 'code' in e) {
      const err = e as { code: string; message: string; hint: string | null }
      throw new IpcError(err.code as ErrorCode, err.message, err.hint ?? null, e)
    }
    if (typeof e === 'string') throw new IpcError('internal', e, null, e)
    throw new IpcError('internal', '发生了未预期的错误', null, e)
  }
}

/** 创建重复任务 */
export const recurringCreate = (input: CreateRecurringInput): Promise<CreateRecurringResult> =>
  call(REC_CMD.create, { input })

/**
 * 规则预览：接下来 N 次发生。
 *
 * 解析与展开都在 Rust 侧完成，前端不重复实现——否则两处的月末策略
 * 可能不一致，用户看到的预览与实际生成的发生就会不同。
 */
export const recurringPreview = (args: {
  rrule: string
  tzid?: string
  dtstartLocal: string
  hasStartTime?: boolean
  count?: number
}): Promise<string[]> => call(REC_CMD.preview, args)

export const recurringMaterialize = (
  seriesId: string,
  rangeStartUtc: string,
  rangeEndUtc: string,
): Promise<number> => call(REC_CMD.materialize, { seriesId, rangeStartUtc, rangeEndUtc })

export const recurringEnsureRange = (rangeStartUtc: string, rangeEndUtc: string): Promise<number> =>
  call(REC_CMD.ensureRange, { rangeStartUtc, rangeEndUtc })

export const recurringGet = (seriesId: string): Promise<SeriesDetail> =>
  call(REC_CMD.get, { seriesId })

export const recurringOccurrences = (
  seriesId: string,
  count?: number,
): Promise<Array<{ index: number; local: string; utc: string }>> =>
  call(REC_CMD.occurrences, { seriesId, count: count ?? null })

export const recurringStats = (seriesId: string): Promise<SeriesStats> =>
  call(REC_CMD.stats, { seriesId })

/** 按范围编辑某次发生 */
export const recurringEditInstance = (args: {
  taskId: string
  scope: EditScope
  patch: InstancePatch
  newRrule?: string | null
  confirmHistory?: boolean
}): Promise<ScopeActionResult> =>
  call(REC_CMD.editInstance, {
    taskId: args.taskId,
    scope: args.scope,
    patch: args.patch,
    newRrule: args.newRrule ?? null,
    confirmHistory: args.confirmHistory ?? false,
  })

/** 跳过某一次（单次取消，系列继续） */
export const recurringSkipOccurrence = (taskId: string): Promise<ScopeActionResult> =>
  call(REC_CMD.skipOccurrence, { taskId })

/** 按范围删除 */
export const recurringDelete = (
  taskId: string,
  mode: DeleteMode,
  confirmHistory = false,
): Promise<DeleteResult> => call(REC_CMD.delete, { taskId, mode, confirmHistory })

export const recurringScopeInfo = (taskId: string): Promise<ScopeInfo> =>
  call(REC_CMD.scopeInfo, { taskId })

// =============================================================================
// 规则构造辅助（纯函数，便于单测）
// =============================================================================

/** 星期名（1=周一 … 7=周日） */
export const WEEKDAY_LABELS: Record<number, string> = {
  1: '一',
  2: '二',
  3: '三',
  4: '四',
  5: '五',
  6: '六',
  7: '日',
}

/** 频率的中文名 */
export const FREQ_LABELS: Record<Freq, string> = {
  daily: '天',
  weekly: '周',
  monthly: '月',
  yearly: '年',
}

/**
 * 由界面选择构造 RRULE 字符串。
 *
 * 这是**唯一**的规则构造出口：避免界面各处各拼一次导致格式不一致
 * （例如有的地方加 INTERVAL 有的忘加）。
 */
export function buildRrule(opts: {
  freq: Freq
  interval: number
  /** 每周：指定的星期（空表示沿用起始日的星期） */
  byWeekday?: number[]
  /** 每周：是否只取工作日 */
  weekdaysOnly?: boolean
  /** 每月：指定日期（1–31） */
  byMonthday?: number[]
  /** 每月：第 N 个星期 X */
  setPos?: SetPos | null
  /** 每年：指定月份 */
  byMonth?: number[]
  end: EndCondition
}): string {
  const parts: string[] = [`FREQ=${opts.freq.toUpperCase()}`]

  if (opts.interval > 1) parts.push(`INTERVAL=${opts.interval}`)

  if (opts.freq === 'weekly') {
    if (opts.weekdaysOnly) {
      parts.push('BYDAY=MO,TU,WE,TH,FR')
    } else if (opts.byWeekday && opts.byWeekday.length > 0) {
      const codes = ['MO', 'TU', 'WE', 'TH', 'FR', 'SA', 'SU']
      const list = [...opts.byWeekday]
        .sort((a, b) => a - b)
        .map((w) => codes[w - 1])
        .filter(Boolean)
      if (list.length > 0) parts.push(`BYDAY=${list.join(',')}`)
    }
  }

  if (opts.freq === 'monthly') {
    if (opts.setPos) {
      const codes = ['MO', 'TU', 'WE', 'TH', 'FR', 'SA', 'SU']
      const code = codes[opts.setPos.weekday - 1] ?? 'MO'
      parts.push(`BYDAY=${code}`)
      parts.push(`BYSETPOS=${opts.setPos.nth}`)
    } else if (opts.byMonthday && opts.byMonthday.length > 0) {
      const list = [...opts.byMonthday].sort((a, b) => a - b)
      parts.push(`BYMONTHDAY=${list.join(',')}`)
    }
  }

  if (opts.freq === 'yearly') {
    if (opts.byMonth && opts.byMonth.length > 0) {
      const list = [...opts.byMonth].sort((a, b) => a - b)
      parts.push(`BYMONTH=${list.join(',')}`)
    }
    if (opts.byMonthday && opts.byMonthday.length > 0) {
      const days = [...opts.byMonthday].sort((a, b) => a - b)
      parts.push(`BYMONTHDAY=${days.join(',')}`)
    }
  }

  switch (opts.end.kind) {
    case 'until':
      // RRULE 的 UNTIL 用紧凑格式
      parts.push(`UNTIL=${opts.end.date.replace(/-/g, '')}`)
      break
    case 'count':
      parts.push(`COUNT=${opts.end.count}`)
      break
    case 'never':
      break
  }

  return parts.join(';')
}

/**
 * 该规则是否涉及月末/闰年等需要向用户解释的边界情况。
 *
 * 与 Rust 侧 `edge_policy_note()` 的判断保持一致：涉及月末的规则
 * 必须把"不存在的日期跳过该月"这一策略展示给用户（§5 要求）。
 */
export function touchesMonthEdge(opts: {
  freq: Freq
  byMonthday?: number[]
  setPos?: SetPos | null
}): boolean {
  if (opts.freq === 'monthly') return true
  // 负数（-1 = 最后一天）同样触及月末策略，与后端 edge_policy_note 保持一致
  if (opts.byMonthday?.some((d) => d < 0 || d > 28)) return true
  if (opts.setPos && (opts.setPos.nth === 5 || opts.setPos.nth === -1)) return true
  return false
}

/**
 * 规则的可读描述（前端即时预览用）。
 *
 * 注意：**最终以 Rust 侧返回的 description 为准**。这里只是让用户在
 * 调整控件时立刻看到文案，避免每次改动都要往返一次后端。
 */
export function describeRule(opts: {
  freq: Freq
  interval: number
  byWeekday?: number[]
  weekdaysOnly?: boolean
  byMonthday?: number[]
  setPos?: SetPos | null
  byMonth?: number[]
  end: EndCondition
}): string {
  const every = opts.interval > 1 ? `每 ${opts.interval} ` : '每'
  let s = `${every}${FREQ_LABELS[opts.freq]}`

  if (opts.freq === 'weekly') {
    if (opts.weekdaysOnly) {
      s += '的工作日（周一至周五）'
    } else if (opts.byWeekday && opts.byWeekday.length > 0) {
      const names = [...opts.byWeekday].sort((a, b) => a - b).map((w) => `周${WEEKDAY_LABELS[w]}`)
      s += `的${names.join('、')}`
    }
  }

  if (opts.freq === 'monthly') {
    if (opts.setPos) {
      const nth = opts.setPos.nth === -1 ? '最后' : `第${opts.setPos.nth}`
      s += `的${nth}个周${WEEKDAY_LABELS[opts.setPos.weekday]}`
    } else if (opts.byMonthday && opts.byMonthday.length > 0) {
      s += `的${[...opts.byMonthday].sort((a, b) => a - b).join('、')}日`
    }
  }

  if (opts.freq === 'yearly') {
    if (opts.byMonth && opts.byMonth.length > 0) {
      s += `的${[...opts.byMonth].sort((a, b) => a - b).join('、')}月`
    }
    if (opts.byMonthday && opts.byMonthday.length > 0) {
      s += `${[...opts.byMonthday].sort((a, b) => a - b).join('、')}日`
    }
  }

  switch (opts.end.kind) {
    case 'until':
      s += `，直到 ${opts.end.date}`
      break
    case 'count':
      s += `，共 ${opts.end.count} 次`
      break
    case 'never':
      break
  }

  return s
}

/** 判断某任务是否属于重复系列（界面据此决定是否显示范围选择） */
export function isRecurring(task: Task): boolean {
  return task.seriesId !== null
}
