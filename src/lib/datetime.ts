/**
 * 时间与时区工具。
 *
 * 本文件是"本地日历"与"UTC 存储"之间的唯一边界（任务书 §4.3 / §5）：
 * - 数据库一律存 UTC ISO-8601；用户看到的一切都是本地时间。
 * - "今天/明天/本周"的归属必须按**用户本地时区**计算，再换算成 UTC
 *   边界传给后端。后端不做任何本地时区假设。
 * - 「仅日期」与「精确时间」严格区分：仅日期任务的边界取当日本地
 *   00:00–24:00，绝不能被当作"当天凌晨到期"（§4.3 明确禁止）。
 */

import { format, startOfDay, endOfDay, startOfWeek, endOfWeek, addDays } from 'date-fns'
import type { Task } from './types'

/** 把 Date 转为后端要求的 UTC ISO-8601（毫秒精度，带 Z） */
export function toUtcIso(d: Date): string {
  return d.toISOString()
}

/** 把后端返回的 UTC ISO 字符串解析为本地 Date；非法输入返回 null */
export function fromUtcIso(s: string | null | undefined): Date | null {
  if (!s) return null
  const d = new Date(s)
  return Number.isNaN(d.getTime()) ? null : d
}

/** 本地日期 → `yyyy-MM-dd`，用于 `<input type="date">` */
export function toDateInput(d: Date | null): string {
  return d ? format(d, 'yyyy-MM-dd') : ''
}

/** 本地时间 → `HH:mm`，用于 `<input type="time">` */
export function toTimeInput(d: Date | null): string {
  return d ? format(d, 'HH:mm') : ''
}

/**
 * 把 `<input type="date">` + `<input type="time">` 组合成本地时间再转 UTC。
 *
 * @param dateStr `yyyy-MM-dd`，必填
 * @param timeStr `HH:mm`；为空表示「仅日期」，此时取本地 00:00:00
 * @returns `{ utc, hasTime }`；日期非法时返回 null
 */
export function combineDateTime(
  dateStr: string,
  timeStr: string,
): { utc: string; hasTime: boolean } | null {
  if (!dateStr) return null
  const [y, m, d] = dateStr.split('-').map(Number)
  if (!y || !m || !d) return null
  if (m < 1 || m > 12 || d < 1 || d > 31) return null

  const hasTime = /^\d{1,2}:\d{2}/.test(timeStr)
  let hh = 0
  let mm = 0
  if (hasTime) {
    const parts = timeStr.split(':').map(Number)
    hh = parts[0] ?? 0
    mm = parts[1] ?? 0
    if (hh < 0 || hh > 23 || mm < 0 || mm > 59) return null
  }

  // 用本地时间构造，Date 会按本机时区解释这些分量
  const local = new Date(y, m - 1, d, hh, mm, 0, 0)
  if (Number.isNaN(local.getTime())) return null

  // 关键：JS 的 Date 会把越界值**静默滚动**（2026-13-45 → 2027-02-14，
  // 2026-02-30 → 2026-03-02）。这会让用户输入的日期与实际保存的不一致，
  // 属于必须拦住的静默数据错误。做法是回读各分量，要求完全一致。
  if (
    local.getFullYear() !== y ||
    local.getMonth() !== m - 1 ||
    local.getDate() !== d ||
    local.getHours() !== hh ||
    local.getMinutes() !== mm
  ) {
    return null
  }

  return { utc: toUtcIso(local), hasTime }
}

// =============================================================================
// 日历区间（全部返回 UTC ISO，可直接传给后端）
// =============================================================================

/** 本地某天的起止（[00:00:00.000, 24:00:00.000)） */
export function localDayRange(base: Date = new Date()): { start: string; end: string } {
  return { start: toUtcIso(startOfDay(base)), end: toUtcIso(endOfDay(base)) }
}

/** 本地"今天"的起止 */
export function todayRange(now: Date = new Date()): { start: string; end: string } {
  return localDayRange(now)
}

/** 本地"明天"的起止 */
export function tomorrowRange(now: Date = new Date()): { start: string; end: string } {
  return localDayRange(addDays(now, 1))
}

/**
 * 本地"本周"的起止。
 *
 * 周起始取周一（weekStartsOn: 1）——与中国用户习惯一致。
 * 注意：本周范围是**含今天在内**的整周，因此往前可能包含已经过去的日子；
 * 「本周」视图会把已过期的部分标注出来，而不是悄悄丢弃。
 */
export function weekRange(now: Date = new Date()): { start: string; end: string } {
  return {
    start: toUtcIso(startOfWeek(now, { weekStartsOn: 1 })),
    end: toUtcIso(endOfWeek(now, { weekStartsOn: 1 })),
  }
}

/** 某个月份网格的范围（日历视图用：从当月首个周一到末个周日） */
export function monthGridRange(month: Date): { start: string; end: string } {
  const first = new Date(month.getFullYear(), month.getMonth(), 1)
  const last = new Date(month.getFullYear(), month.getMonth() + 1, 0)
  return {
    start: toUtcIso(startOfWeek(first, { weekStartsOn: 1 })),
    end: toUtcIso(endOfWeek(last, { weekStartsOn: 1 })),
  }
}

// =============================================================================
// 归属判定
// =============================================================================

/** 任务的时间归属桶 */
export type TimeBucket = 'overdue' | 'today' | 'tomorrow' | 'thisWeek' | 'later' | 'unscheduled'

/**
 * 判断任务落在哪个时间桶。
 *
 * 取时间字段的优先级：`plannedAt`（计划）→ `dueAt`（截止）。
 * 这与「今天」视图筛选器口径一致（§4.2 要求界面明确说明规则）。
 *
 * 已完成/已归档的任务**永不归入 overdue**——它们已经结束，
 * 再把它们显示成"逾期"会污染用户对工作量的判断（§4.2、§7）。
 * 但它们仍按各自的计划/截止日期参与 today / tomorrow 等分桶，
 * 因为「今天」视图需要显示今天已完成的事项。
 */
export function bucketOf(task: Task, now: Date = new Date()): TimeBucket {
  const anchor = fromUtcIso(task.plannedAt) ?? fromUtcIso(task.dueAt)
  if (!anchor) return 'unscheduled'

  const isDone = task.status === 'done' || task.status === 'archived'
  const dayStart = startOfDay(now).getTime()
  const t = startOfDay(anchor).getTime()
  const dayMs = 86_400_000

  if (!isDone) {
    // 逾期只看截止时间，且仅限未完成任务
    const dueDate = fromUtcIso(task.dueAt)
    if (dueDate) {
      const dueBoundary = task.hasDueTime ? dueDate : endOfDay(dueDate)
      if (dueBoundary.getTime() < now.getTime()) return 'overdue'
    }
    // 计划时间已过且未完成，同样视为逾期
    if (t < dayStart) return 'overdue'
  }

  if (t === dayStart) return 'today'
  if (t === dayStart + dayMs) return 'tomorrow'

  const week = weekRange(now)
  if (anchor.getTime() <= new Date(week.end).getTime()) return 'thisWeek'
  return 'later'
}

/**
 * 逾期判定（与后端 `Task::is_overdue` 语义保持一致）。
 *
 * 仅日期型截止时间按"当日结束"判断，而不是当日 00:00——
 * 否则全天任务会在凌晨就被标成逾期。
 */
export function isOverdue(task: Task, now: Date = new Date()): boolean {
  if (task.status === 'done' || task.status === 'archived') return false
  const due = fromUtcIso(task.dueAt)
  if (!due) return false
  const boundary = task.hasDueTime ? due : endOfDay(due)
  return boundary.getTime() < now.getTime()
}

// =============================================================================
// 展示格式化
// =============================================================================

/** 人类可读的时间描述，用于任务卡片 */
export function formatTaskTime(task: Task, now: Date = new Date()): string {
  const planned = fromUtcIso(task.plannedAt)
  const due = fromUtcIso(task.dueAt)
  const parts: string[] = []

  const dayLabel = (d: Date): string => {
    const diff = Math.round(
      (startOfDay(d).getTime() - startOfDay(now).getTime()) / 86_400_000,
    )
    if (diff === 0) return '今天'
    if (diff === 1) return '明天'
    if (diff === -1) return '昨天'
    if (d.getFullYear() === now.getFullYear()) return format(d, 'M月d日')
    return format(d, 'yyyy年M月d日')
  }

  if (planned) {
    parts.push(
      task.hasPlannedTime ? `${dayLabel(planned)} ${format(planned, 'HH:mm')}` : dayLabel(planned),
    )
  }
  if (due) {
    parts.push(
      task.hasDueTime ? `截止 ${dayLabel(due)} ${format(due, 'HH:mm')}` : `截止 ${dayLabel(due)}`,
    )
  }
  return parts.join(' · ')
}

/** 进度文案：已完成子任务 / 总数 */
export function formatProgress(done: number, total: number): string {
  if (total <= 0) return ''
  return `${done}/${total}`
}
