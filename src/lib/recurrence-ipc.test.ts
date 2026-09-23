/**
 * 重复规则前端辅助的单元测试（任务书 §5）。
 *
 * 这些函数负责把界面控件翻译成 RRULE 字符串。它们与 Rust 侧的解析器
 * 是一份**隐式契约**：拼错一个分号或漏掉 INTERVAL，用户设置就会静默失效。
 * 因此这里把常见组合逐一锁定。
 */

import { describe, it, expect } from 'vitest'
import {
  buildRrule,
  describeRule,
  touchesMonthEdge,
  isRecurring,
  WEEKDAY_LABELS,
} from './recurrence-ipc'
import type { Task } from './types'

describe('buildRrule', () => {
  it('每天', () => {
    expect(buildRrule({ freq: 'daily', interval: 1, end: { kind: 'never' } })).toBe('FREQ=DAILY')
  })

  it('每 3 天带 INTERVAL', () => {
    expect(buildRrule({ freq: 'daily', interval: 3, end: { kind: 'never' } })).toBe(
      'FREQ=DAILY;INTERVAL=3',
    )
  })

  it('每周指定星期一三五', () => {
    expect(
      buildRrule({
        freq: 'weekly',
        interval: 1,
        byWeekday: [1, 3, 5],
        end: { kind: 'never' },
      }),
    ).toBe('FREQ=WEEKLY;BYDAY=MO,WE,FR')
  })

  it('每周的星期按编号排序，避免生成 MO,FR,WE 这种不稳定顺序', () => {
    expect(
      buildRrule({
        freq: 'weekly',
        interval: 1,
        byWeekday: [5, 1, 3],
        end: { kind: 'never' },
      }),
    ).toBe('FREQ=WEEKLY;BYDAY=MO,WE,FR')
  })

  it('仅工作日用简写形式', () => {
    expect(
      buildRrule({ freq: 'weekly', interval: 1, weekdaysOnly: true, end: { kind: 'never' } }),
    ).toBe('FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR')
  })

  it('每周未选星期时不输出 BYDAY（由后端沿用起始日的星期）', () => {
    expect(buildRrule({ freq: 'weekly', interval: 1, byWeekday: [], end: { kind: 'never' } })).toBe(
      'FREQ=WEEKLY',
    )
  })

  it('每月指定日期', () => {
    expect(
      buildRrule({ freq: 'monthly', interval: 1, byMonthday: [1, 15, 31], end: { kind: 'never' } }),
    ).toBe('FREQ=MONTHLY;BYMONTHDAY=1,15,31')
  })

  it('每月第 3 个星期五', () => {
    expect(
      buildRrule({
        freq: 'monthly',
        interval: 1,
        setPos: { nth: 3, weekday: 5 },
        end: { kind: 'never' },
      }),
    ).toBe('FREQ=MONTHLY;BYDAY=FR;BYSETPOS=3')
  })

  it('每月最后一个星期五', () => {
    expect(
      buildRrule({
        freq: 'monthly',
        interval: 1,
        setPos: { nth: -1, weekday: 5 },
        end: { kind: 'never' },
      }),
    ).toBe('FREQ=MONTHLY;BYDAY=FR;BYSETPOS=-1')
  })

  it('setpos 与 byMonthday 互斥：给了 setPos 就不输出 BYMONTHDAY', () => {
    const s = buildRrule({
      freq: 'monthly',
      interval: 1,
      setPos: { nth: 2, weekday: 2 },
      byMonthday: [10],
      end: { kind: 'never' },
    })
    expect(s).not.toContain('BYMONTHDAY')
    expect(s).toContain('BYSETPOS=2')
  })

  it('结束条件：直到某日（紧凑格式）', () => {
    expect(
      buildRrule({ freq: 'daily', interval: 1, end: { kind: 'until', date: '2026-12-31' } }),
    ).toBe('FREQ=DAILY;UNTIL=20261231')
  })

  it('结束条件：共 N 次', () => {
    expect(buildRrule({ freq: 'daily', interval: 1, end: { kind: 'count', count: 10 } })).toBe(
      'FREQ=DAILY;COUNT=10',
    )
  })

  it('结束条件互斥：never 不输出 UNTIL/COUNT', () => {
    const s = buildRrule({ freq: 'daily', interval: 1, end: { kind: 'never' } })
    expect(s).not.toContain('UNTIL')
    expect(s).not.toContain('COUNT')
  })

  it('每年指定月份', () => {
    expect(
      buildRrule({ freq: 'yearly', interval: 1, byMonth: [3, 9], end: { kind: 'never' } }),
    ).toBe('FREQ=YEARLY;BYMONTH=3,9')
  })

  it('组合：每 2 周的周二与周四，共 20 次', () => {
    expect(
      buildRrule({
        freq: 'weekly',
        interval: 2,
        byWeekday: [2, 4],
        end: { kind: 'count', count: 20 },
      }),
    ).toBe('FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,TH;COUNT=20')
  })

  it('所有频率都能产出以 FREQ= 开头的合法片段', () => {
    for (const freq of ['daily', 'weekly', 'monthly', 'yearly'] as const) {
      const s = buildRrule({ freq, interval: 1, end: { kind: 'never' } })
      expect(s.startsWith('FREQ=')).toBe(true)
    }
  })
})

describe('describeRule', () => {
  it('描述每天', () => {
    expect(describeRule({ freq: 'daily', interval: 1, end: { kind: 'never' } })).toBe('每天')
  })

  it('描述间隔', () => {
    expect(describeRule({ freq: 'daily', interval: 3, end: { kind: 'never' } })).toBe('每 3 天')
  })

  it('描述指定的星期', () => {
    const s = describeRule({
      freq: 'weekly',
      interval: 1,
      byWeekday: [1, 3, 5],
      end: { kind: 'never' },
    })
    expect(s).toContain('周一')
    expect(s).toContain('周三')
    expect(s).toContain('周五')
  })

  it('描述仅工作日', () => {
    const s = describeRule({
      freq: 'weekly',
      interval: 1,
      weekdaysOnly: true,
      end: { kind: 'never' },
    })
    expect(s).toContain('工作日')
  })

  it('描述每月第几个星期几（不留多余空格）', () => {
    const s = describeRule({
      freq: 'monthly',
      interval: 1,
      setPos: { nth: 3, weekday: 5 },
      end: { kind: 'never' },
    })
    expect(s).toContain('第3个')
    expect(s).toContain('周五')
    expect(s).not.toContain('第 3')
  })

  it('描述最后一个星期几', () => {
    const s = describeRule({
      freq: 'monthly',
      interval: 1,
      setPos: { nth: -1, weekday: 5 },
      end: { kind: 'never' },
    })
    expect(s).toContain('最后')
    expect(s).toContain('周五')
  })

  it('描述结束条件', () => {
    expect(
      describeRule({ freq: 'daily', interval: 1, end: { kind: 'until', date: '2026-12-31' } }),
    ).toContain('直到 2026-12-31')
    expect(
      describeRule({ freq: 'daily', interval: 1, end: { kind: 'count', count: 5 } }),
    ).toContain('共 5 次')
  })

  it('描述每月指定日期', () => {
    const s = describeRule({
      freq: 'monthly',
      interval: 1,
      byMonthday: [1, 15],
      end: { kind: 'never' },
    })
    expect(s).toContain('1')
    expect(s).toContain('15')
  })
})

describe('touchesMonthEdge', () => {
  it('每月重复都需要说明月末策略', () => {
    expect(touchesMonthEdge({ freq: 'monthly' })).toBe(true)
  })

  it('每月 31 日需要说明', () => {
    expect(touchesMonthEdge({ freq: 'weekly', byMonthday: [31] })).toBe(true)
  })

  it('每月 1–28 日不涉及"不存在的日期"', () => {
    expect(touchesMonthEdge({ freq: 'weekly', byMonthday: [1, 15, 28] })).toBe(false)
  })

  it('第 5 个星期几可能不存在，需要说明', () => {
    expect(touchesMonthEdge({ freq: 'weekly', setPos: { nth: 5, weekday: 5 } })).toBe(true)
  })

  it('最后一个星期几同样涉及月末天数，需要说明', () => {
    // "最后一个周五"本身就是靠"这个月有几个周五"定位的，
    // 与该月天数强相关，因此必须把月末策略讲清楚。
    expect(touchesMonthEdge({ freq: 'weekly', setPos: { nth: -1, weekday: 5 } })).toBe(true)
  })

  it('每月的 setPos 规则一律需要说明（与 Rust 侧 edge_policy_note 一致）', () => {
    expect(touchesMonthEdge({ freq: 'monthly', setPos: { nth: 2, weekday: 2 } })).toBe(true)
  })

  it('每天与每周不涉及月末', () => {
    expect(touchesMonthEdge({ freq: 'daily' })).toBe(false)
    expect(touchesMonthEdge({ freq: 'weekly' })).toBe(false)
  })
})

describe('isRecurring', () => {
  const base: Task = {
    id: 't1',
    title: 'x',
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
    createdAt: '2026-01-01T00:00:00.000Z',
    updatedAt: '2026-01-01T00:00:00.000Z',
    deletedAt: null,
    sortOrder: 0,
    isPinned: 0,
    isFavorite: 0,
    periodType: 'none',
    seriesId: null,
    occurrenceKey: null,
    occurrenceIndex: null,
    occurrenceKind: 'single',
    isException: 0,
  }

  it('seriesId 为空表示普通任务', () => {
    expect(isRecurring(base)).toBe(false)
  })

  it('seriesId 非空表示重复实例', () => {
    expect(isRecurring({ ...base, seriesId: 's1' })).toBe(true)
  })
})

describe('WEEKDAY_LABELS', () => {
  it('覆盖 1–7 且周一对应"一"', () => {
    expect(WEEKDAY_LABELS[1]).toBe('一')
    expect(WEEKDAY_LABELS[7]).toBe('日')
    for (let i = 1; i <= 7; i++) {
      expect(WEEKDAY_LABELS[i]).toBeTruthy()
    }
  })
})
