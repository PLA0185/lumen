/**
 * 时间与时区工具测试（§4.3 / §4.2）。
 *
 * 重点验证两个容易出错的语义：
 * 1. 「仅日期」任务不能被当作"当天凌晨到期"，否则全天任务一到凌晨就显示逾期。
 * 2. 时区边界必须按本地日历计算，且往返转换无损。
 */

import { describe, it, expect } from 'vitest'
import {
  toUtcIso,
  fromUtcIso,
  combineDateTime,
  todayRange,
  tomorrowRange,
  weekRange,
  bucketOf,
  isOverdue,
  formatTaskTime,
} from './datetime'
import type { Task } from './types'

/** 构造一个测试任务，只需覆盖被测字段 */
function makeTask(over: Partial<Task> = {}): Task {
  return {
    id: 't1',
    title: '测试任务',
    description: '',
    noteMd: '',
    linkUrl: null,
    status: 'todo',
    priority: 0,
    projectId: null,
    categoryId: null,
    plannedAt: null,
    hasPlannedTime: 0,
    dueAt: null,
    hasDueTime: 0,
    estimatedMinutes: null,
    actualMinutes: 0,
    completedAt: null,
    createdAt: '2026-09-01T00:00:00.000Z',
    updatedAt: '2026-09-01T00:00:00.000Z',
    deletedAt: null,
    sortOrder: 0,
    isPinned: 0,
    isFavorite: 0,
    seriesId: null,
    occurrenceKey: null,
    occurrenceIndex: null,
    occurrenceKind: 'single',
    isException: 0,
    ...over,
  }
}

describe('UTC 往返', () => {
  it('toUtcIso / fromUtcIso 互逆', () => {
    const d = new Date(2026, 8, 23, 14, 30, 15, 250)
    const back = fromUtcIso(toUtcIso(d))
    expect(back!.getTime()).toBe(d.getTime())
  })

  it('输出为后端要求的 ISO-8601 带 Z 格式', () => {
    const s = toUtcIso(new Date(Date.UTC(2026, 8, 23, 1, 0, 0)))
    expect(s).toBe('2026-09-23T01:00:00.000Z')
  })

  it('非法或空输入返回 null，而不是 Invalid Date', () => {
    expect(fromUtcIso(null)).toBeNull()
    expect(fromUtcIso(undefined)).toBeNull()
    expect(fromUtcIso('')).toBeNull()
    expect(fromUtcIso('不是日期')).toBeNull()
  })

  it('固定宽度 UTC 字符串可直接字典序比较（后端依赖此性质排序）', () => {
    const a = toUtcIso(new Date(Date.UTC(2026, 8, 23, 1, 0, 0)))
    const b = toUtcIso(new Date(Date.UTC(2026, 8, 23, 2, 0, 0)))
    expect(a < b).toBe(true)
  })
})

describe('combineDateTime', () => {
  it('日期 + 时间组合后按本地时区解释', () => {
    const r = combineDateTime('2026-09-23', '14:30')
    expect(r).not.toBeNull()
    expect(r!.hasTime).toBe(true)
    const back = fromUtcIso(r!.utc)!
    // 无论本机处于哪个时区，往返后本地时刻都应等于输入
    expect(back.getFullYear()).toBe(2026)
    expect(back.getMonth()).toBe(8)
    expect(back.getDate()).toBe(23)
    expect(back.getHours()).toBe(14)
    expect(back.getMinutes()).toBe(30)
  })

  it('只给日期时 hasTime 为 false 且时刻归零（不默认为凌晨到期）', () => {
    const r = combineDateTime('2026-09-23', '')
    expect(r!.hasTime).toBe(false)
    const back = fromUtcIso(r!.utc)!
    expect(back.getHours()).toBe(0)
    expect(back.getMinutes()).toBe(0)
  })

  it('空日期返回 null（调用方据此判定"未设置"）', () => {
    expect(combineDateTime('', '10:00')).toBeNull()
  })

  it('非法日期返回 null', () => {
    expect(combineDateTime('abc', '10:00')).toBeNull()
    expect(combineDateTime('2026-13-45', '10:00')).toBeNull()
  })
})

describe('日历区间', () => {
  const now = new Date(2026, 8, 23, 14, 30)

  it('今天的区间起点不晚于现在、终点晚于现在', () => {
    const r = todayRange(now)
    const start = fromUtcIso(r.start)!
    const end = fromUtcIso(r.end)!
    expect(start.getTime()).toBeLessThanOrEqual(now.getTime())
    expect(end.getTime()).toBeGreaterThan(now.getTime())
  })

  it('今天的本地日期就是当天', () => {
    const r = todayRange(now)
    const start = fromUtcIso(r.start)!
    expect(start.getDate()).toBe(23)
    expect(start.getHours()).toBe(0)
  })

  it('明天的区间起点是次日 00:00', () => {
    const r = tomorrowRange(now)
    const start = fromUtcIso(r.start)!
    expect(start.getDate()).toBe(24)
    expect(start.getHours()).toBe(0)
  })

  it('本周按周一起算（2026-09-23 是周三 → 本周一 09-21，周日 09-27）', () => {
    const r = weekRange(now)
    const start = fromUtcIso(r.start)!
    const end = fromUtcIso(r.end)!
    expect(start.getDate()).toBe(21)
    expect(start.getDay()).toBe(1) // 周一
    expect(end.getDate()).toBe(27)
    expect(end.getDay()).toBe(0) // 周日
  })

  it('区间为左闭右闭（含当天全部时刻）', () => {
    const r = todayRange(now)
    expect(fromUtcIso(r.start)!.getHours()).toBe(0)
    expect(fromUtcIso(r.end)!.getHours()).toBe(23)
    expect(fromUtcIso(r.end)!.getMinutes()).toBe(59)
  })
})

describe('bucketOf 归属判定', () => {
  const now = new Date(2026, 8, 23, 14, 30)

  it('无计划与截止时间 → unscheduled', () => {
    expect(bucketOf(makeTask(), now)).toBe('unscheduled')
  })

  it('计划时间在今天的任务 → today', () => {
    const d = new Date(2026, 8, 23, 9, 0)
    expect(bucketOf(makeTask({ plannedAt: toUtcIso(d), hasPlannedTime: 1 }), now)).toBe('today')
  })

  it('计划时间在明天的任务 → tomorrow', () => {
    const d = new Date(2026, 8, 24, 9, 0)
    expect(bucketOf(makeTask({ plannedAt: toUtcIso(d), hasPlannedTime: 1 }), now)).toBe('tomorrow')
  })

  it('计划时间在过去的任务 → overdue', () => {
    const d = new Date(2026, 8, 20, 9, 0)
    expect(bucketOf(makeTask({ plannedAt: toUtcIso(d), hasPlannedTime: 1 }), now)).toBe('overdue')
  })

  it('截止时间已过且未完成 → overdue', () => {
    const d = new Date(2026, 8, 22, 18, 0)
    expect(bucketOf(makeTask({ dueAt: toUtcIso(d), hasDueTime: 1 }), now)).toBe('overdue')
  })

  it('已完成任务即使截止已过也不归入 overdue', () => {
    const d = new Date(2026, 8, 22, 18, 0)
    const t = makeTask({ dueAt: toUtcIso(d), hasDueTime: 1, status: 'done' })
    expect(bucketOf(t, now)).not.toBe('overdue')
  })
})

describe('isOverdue', () => {
  const now = new Date(2026, 8, 23, 14, 30)

  it('无截止时间永不逾期（§4.1 截止时间可为空）', () => {
    expect(isOverdue(makeTask(), now)).toBe(false)
  })

  it('已完成不算逾期', () => {
    const d = new Date(2026, 0, 1)
    expect(isOverdue(makeTask({ dueAt: toUtcIso(d), hasDueTime: 1, status: 'done' }), now)).toBe(false)
  })

  it('已归档不算逾期', () => {
    const d = new Date(2026, 0, 1)
    expect(isOverdue(makeTask({ dueAt: toUtcIso(d), hasDueTime: 1, status: 'archived' }), now)).toBe(false)
  })

  it('精确时间：当天 09:00 截止，14:30 时已逾期', () => {
    const d = new Date(2026, 8, 23, 9, 0)
    expect(isOverdue(makeTask({ dueAt: toUtcIso(d), hasDueTime: 1 }), now)).toBe(true)
  })

  it('仅日期：当天到期，在当天 14:30 时**未**逾期（关键：不能按凌晨判断）', () => {
    const d = new Date(2026, 8, 23, 0, 0)
    expect(isOverdue(makeTask({ dueAt: toUtcIso(d), hasDueTime: 0 }), now)).toBe(false)
  })

  it('仅日期：过了当天 23:59 才算逾期', () => {
    const d = new Date(2026, 8, 23, 0, 0)
    const later = new Date(2026, 8, 24, 0, 30)
    expect(isOverdue(makeTask({ dueAt: toUtcIso(d), hasDueTime: 0 }), later)).toBe(true)
  })
})

describe('formatTaskTime', () => {
  const now = new Date(2026, 8, 23, 14, 30)

  it('今天带时刻显示为「今天 HH:mm」', () => {
    const d = new Date(2026, 8, 23, 9, 5)
    const s = formatTaskTime(makeTask({ plannedAt: toUtcIso(d), hasPlannedTime: 1 }), now)
    expect(s).toBe('今天 09:05')
  })

  it('仅日期不显示时刻', () => {
    const d = new Date(2026, 8, 23, 0, 0)
    const s = formatTaskTime(makeTask({ plannedAt: toUtcIso(d), hasPlannedTime: 0 }), now)
    expect(s).toBe('今天')
  })

  it('计划与截止同时存在时都展示，且用「截止」区分（§4.1 三字段含义不同）', () => {
    const p = new Date(2026, 8, 23, 9, 0)
    const due = new Date(2026, 8, 25, 18, 0)
    const s = formatTaskTime(
      makeTask({ plannedAt: toUtcIso(p), hasPlannedTime: 1, dueAt: toUtcIso(due), hasDueTime: 1 }),
      now,
    )
    expect(s).toContain('今天 09:00')
    expect(s).toContain('截止')
    expect(s).toContain('18:00')
  })

  it('两者都为空时返回空字符串（卡片不显示时间行）', () => {
    expect(formatTaskTime(makeTask(), now)).toBe('')
  })

  it('跨年日期带上年份，避免歧义', () => {
    const d = new Date(2027, 0, 5, 0, 0)
    const s = formatTaskTime(makeTask({ plannedAt: toUtcIso(d), hasPlannedTime: 0 }), now)
    expect(s).toContain('2027')
  })
})
