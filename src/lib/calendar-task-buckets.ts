import { fromUtcIso } from './datetime'
import type { PeriodType, Task } from './types'

const DAILY_PERIOD_TYPES = new Set<PeriodType>(['week', 'month', 'quarter', 'year'])

function dayKey(date: Date): string {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

function periodRange(date: Date, period: PeriodType): { start: string; end: string } | null {
  let start: Date
  let end: Date
  if (period === 'week') {
    const mondayOffset = (date.getDay() + 6) % 7
    start = new Date(date.getFullYear(), date.getMonth(), date.getDate() - mondayOffset)
    end = new Date(start.getFullYear(), start.getMonth(), start.getDate() + 7)
  } else if (period === 'month') {
    start = new Date(date.getFullYear(), date.getMonth(), 1)
    end = new Date(date.getFullYear(), date.getMonth() + 1, 1)
  } else if (period === 'quarter') {
    const firstMonth = Math.floor(date.getMonth() / 3) * 3
    start = new Date(date.getFullYear(), firstMonth, 1)
    end = new Date(date.getFullYear(), firstMonth + 3, 1)
  } else if (period === 'year') {
    start = new Date(date.getFullYear(), 0, 1)
    end = new Date(date.getFullYear() + 1, 0, 1)
  } else {
    return null
  }
  return { start: dayKey(start), end: dayKey(end) }
}

/**
 * 普通任务按计划 / 截止日期显示。周、月、季度、年跨度任务按其日期锚点
 * 所属的自然周期逐日显示；没有明确日期时，以创建日期为锚点。
 * 完成任务在完成日显示为已完成，下一天起隐藏。
 */
export function calendarTaskBuckets(tasks: Task[], days: Date[]): Map<string, Task[]> {
  const buckets = new Map<string, Task[]>()
  const append = (date: string, task: Task) => {
    const existing = buckets.get(date)
    if (existing) existing.push(task)
    else buckets.set(date, [task])
  }

  for (const task of tasks) {
    const scheduledAt = fromUtcIso(task.plannedAt) ?? fromUtcIso(task.dueAt)
    if (!DAILY_PERIOD_TYPES.has(task.periodType)) {
      if (scheduledAt) append(dayKey(scheduledAt), task)
      continue
    }

    const createdAt = fromUtcIso(task.createdAt)
    const anchor = scheduledAt ?? createdAt
    if (!anchor) continue
    const range = periodRange(anchor, task.periodType)
    if (!range) continue
    const visibleStart = scheduledAt ? range.start : dayKey(createdAt!)

    const done = task.status === 'done'
    const completedAt = done
      ? fromUtcIso(task.completedAt) ?? fromUtcIso(task.updatedAt) ?? fromUtcIso(task.createdAt)
      : null
    const completedDay = completedAt ? dayKey(completedAt) : null
    for (const date of days) {
      const key = dayKey(date)
      const inPeriod = key >= range.start && key < range.end
      // A completion can be outside the original period; keep its completion
      // day visible as requested, while still hiding all following days.
      if ((!inPeriod || key < visibleStart) && !(done && key === completedDay)) continue
      if (done && completedDay && key > completedDay) continue
      const asOfTask = done && completedDay && key < completedDay
        ? { ...task, status: 'todo' as const, completedAt: null }
        : task
      append(key, asOfTask)
    }
  }

  return buckets
}
