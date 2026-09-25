import { describe, expect, it } from 'vitest'
import { recurrenceSeriesSummary } from './scope-copy'

describe('重复系列范围摘要', () => {
  it('按次数结束时区分计划次数和已生成次数', () => {
    expect(recurrenceSeriesSummary({
      isRecurring: true,
      recurrenceEndKind: 'count',
      recurrenceCount: 12,
      totalInstances: 5,
    })).toBe('计划共 12 次 · 已生成 5 次')
  })

  it('按日期结束时显示截止日和已生成次数', () => {
    expect(recurrenceSeriesSummary({
      isRecurring: true,
      recurrenceEndKind: 'until',
      recurrenceUntil: '2026-10-08T00:00:00Z',
      totalInstances: 4,
    })).toBe('重复至 2026-10-08 · 已生成 4 次')
  })

  it('无限重复只陈述当前已经生成的数量', () => {
    expect(recurrenceSeriesSummary({
      isRecurring: true,
      recurrenceEndKind: 'never',
      totalInstances: 3,
    })).toBe('无限重复 · 当前已生成 3 次')
  })
})
